use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use rusqlite::params;
use tokio::sync::Mutex;

use crate::catalog_sync::CatalogSync;
use crate::config::Config;
use crate::db::DbPool;
use crate::thumbnail;

const RAW_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "nef", "arw", "orf", "rw2", "dng", "raf", "pef", "srw", "x3f", "3fr", "mrw",
    "nrw", "raw",
];

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "heic", "heif",
];

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "avi", "mkv", "360", "webm"];

pub fn classify_file(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    if RAW_EXTENSIONS.contains(&ext.as_str()) {
        "raw"
    } else if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        "image"
    } else if VIDEO_EXTENSIONS.contains(&ext.as_str()) {
        "video"
    } else {
        "other"
    }
}

/// Entry that was indexed and needs to be published to the catalog.
pub struct IndexedEntry {
    pub cid: String,
    pub filename: String,
    pub dir: String,
    pub path: String,
    pub size: i64,
    pub mime: Option<String>,
    pub file_type: String,
    pub modified: Option<String>,
    pub thumbnail_bytes: Option<Vec<u8>>,
}

/// Spawn the background indexer that runs on startup and every 5 minutes.
pub fn spawn_indexer(
    config: Config,
    pool: DbPool,
    catalog: Option<Arc<Mutex<CatalogSync>>>,
) {
    tokio::spawn(async move {
        loop {
            let cfg = config.clone();
            let db = pool.clone();

            let result =
                tokio::task::spawn_blocking(move || run_index_cycle(&cfg, &db)).await;

            match result {
                Ok((_file_count, _thumb_count, entries_to_publish)) => {
                    // Publish new entries to catalog sync if available
                    if let Some(ref cat) = catalog {
                        for entry in entries_to_publish {
                            let sync = cat.lock().await;
                            if let Err(e) = sync
                                .publish_entry(
                                    &entry.cid,
                                    &entry.filename,
                                    &entry.dir,
                                    &entry.path,
                                    entry.size,
                                    entry.mime.as_deref(),
                                    &entry.file_type,
                                    entry.modified.as_deref(),
                                    entry.thumbnail_bytes.as_deref(),
                                )
                                .await
                            {
                                tracing::debug!("Failed to publish catalog entry: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Indexer task panicked: {e}");
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        }
    });
}

/// Run a single index cycle across all configured directories.
/// Returns (file_count, thumb_count, entries_to_publish).
fn run_index_cycle(
    config: &Config,
    pool: &DbPool,
) -> (u64, u64, Vec<IndexedEntry>) {
    let start = Instant::now();
    let mut file_count = 0u64;
    let mut thumb_count = 0u64;
    let mut to_publish = Vec::new();

    for dir_config in &config.directories {
        let base = config.resolve_directory(&dir_config.label);
        let base = match base {
            Some(b) if b.is_dir() => b,
            _ => {
                tracing::warn!(
                    "Directory not found: {} ({})",
                    dir_config.label,
                    dir_config.path
                );
                continue;
            }
        };

        let (f, t, mut entries) =
            index_directory(pool, &dir_config.label, &base, &base);
        file_count += f;
        thumb_count += t;
        to_publish.append(&mut entries);
    }

    let elapsed = start.elapsed();
    tracing::info!(
        "Index complete: {} files, {} thumbnails in {:.1}s",
        file_count,
        thumb_count,
        elapsed.as_secs_f64()
    );

    (file_count, thumb_count, to_publish)
}

/// Recursively index a directory, returning (files_indexed, thumbnails_generated, entries).
fn index_directory(
    pool: &DbPool,
    dir_label: &str,
    base: &Path,
    current: &Path,
) -> (u64, u64, Vec<IndexedEntry>) {
    let mut file_count = 0u64;
    let mut thumb_count = 0u64;
    let mut to_publish = Vec::new();

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read directory {}: {e}", current.display());
            return (0, 0, Vec::new());
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            let (f, t, mut sub_entries) =
                index_directory(pool, dir_label, base, &path);
            file_count += f;
            thumb_count += t;
            to_publish.append(&mut sub_entries);
            continue;
        }

        if !path.is_file() {
            continue;
        }

        // Background indexer: metadata only (hash + EXIF), NO thumbnails.
        // Thumbnails are generated on-demand when someone browses.
        match index_file_metadata_only(pool, dir_label, base, &path) {
            Ok(Some(indexed)) => {
                file_count += 1;
                to_publish.push(indexed);
            }
            Ok(None) => {
                // Already indexed, no changes
            }
            Err(e) => {
                tracing::debug!("Failed to index {}: {e}", path.display());
            }
        }

        // Yield CPU between files so we don't peg all cores
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    (file_count, thumb_count, to_publish)
}

/// Lightweight index: hash + EXIF date + metadata only, NO thumbnail.
/// Used by the background indexer to avoid CPU spikes.
fn index_file_metadata_only(
    pool: &DbPool,
    dir_label: &str,
    base: &Path,
    path: &Path,
) -> anyhow::Result<Option<IndexedEntry>> {
    let metadata = std::fs::metadata(path)?;
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let rel_path = path
        .strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let file_type = classify_file(&filename);

    let fs_modified = metadata.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                .map(|dt| dt.to_rfc3339())
        })
    });
    let modified = if file_type == "image" || file_type == "raw" {
        extract_exif_date(path).or(fs_modified)
    } else {
        fs_modified
    };

    let size = metadata.len() as i64;
    let mime = mime_guess::from_path(path)
        .first()
        .map(|m| m.to_string());

    // Check if already indexed
    let conn = pool.get()?;
    let existing: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT cid, modified FROM content_index WHERE dir = ?1 AND path = ?2",
            params![dir_label, rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    if let Some((_existing_cid, existing_modified)) = &existing {
        if existing_modified.as_deref() == modified.as_deref() {
            return Ok(None);
        }
    }

    // Compute BLAKE3 hash
    let cid = hash_file(path)?;

    conn.execute(
        "INSERT INTO content_index (cid, dir, path, filename, size, mime, file_type, modified, is_local)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
         ON CONFLICT(cid) DO UPDATE SET
           dir = excluded.dir,
           path = excluded.path,
           filename = excluded.filename,
           size = excluded.size,
           mime = excluded.mime,
           file_type = excluded.file_type,
           modified = excluded.modified,
           is_local = 1,
           indexed_at = datetime('now')
         ",
        params![cid, dir_label, rel_path, filename, size, mime, file_type, modified],
    )?;

    conn.execute(
        "DELETE FROM content_index WHERE dir = ?1 AND path = ?2 AND cid != ?3",
        params![dir_label, rel_path, cid],
    )?;

    Ok(Some(IndexedEntry {
        cid,
        filename,
        dir: dir_label.to_string(),
        path: rel_path,
        size,
        mime,
        file_type: file_type.to_string(),
        modified,
        thumbnail_bytes: None, // No thumbnail in background pass
    }))
}

/// Full index: hash + EXIF + thumbnail. Used by on-demand indexing endpoint.
pub fn index_file(
    pool: &DbPool,
    dir_label: &str,
    base: &Path,
    path: &Path,
) -> anyhow::Result<Option<IndexedEntry>> {
    let metadata = std::fs::metadata(path)?;
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let rel_path = path
        .strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let fs_modified = metadata.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| {
            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                .map(|dt| dt.to_rfc3339())
        })
    });

    let size = metadata.len() as i64;
    let file_type = classify_file(&filename);

    // For images, prefer EXIF DateTimeOriginal over filesystem mtime
    let modified = if file_type == "image" || file_type == "raw" {
        extract_exif_date(path).or(fs_modified)
    } else {
        fs_modified
    };
    let mime = mime_guess::from_path(path)
        .first()
        .map(|m| m.to_string());

    // Check if already indexed with same modified time
    let conn = pool.get()?;
    let existing: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT cid, modified FROM content_index WHERE dir = ?1 AND path = ?2",
            params![dir_label, rel_path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    if let Some((existing_cid, existing_modified)) = &existing {
        if existing_modified.as_deref() == modified.as_deref() {
            // Already indexed and unchanged — check if thumbnail exists for image/raw
            if file_type == "image" || file_type == "raw" {
                let has_thumb: bool = conn
                    .query_row(
                        "SELECT COUNT(*) > 0 FROM content_thumbnails WHERE cid = ?1",
                        params![existing_cid],
                        |row| row.get(0),
                    )
                    .unwrap_or(false);

                if has_thumb {
                    return Ok(None);
                }
                // Fall through to generate missing thumbnail
            } else {
                return Ok(None);
            }
        }
    }

    // Compute BLAKE3 hash
    let cid = hash_file(path)?;

    // Upsert content_index
    conn.execute(
        "INSERT INTO content_index (cid, dir, path, filename, size, mime, file_type, modified, is_local)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
         ON CONFLICT(cid) DO UPDATE SET
           dir = excluded.dir,
           path = excluded.path,
           filename = excluded.filename,
           size = excluded.size,
           mime = excluded.mime,
           file_type = excluded.file_type,
           modified = excluded.modified,
           is_local = 1,
           indexed_at = datetime('now')
         ",
        params![cid, dir_label, rel_path, filename, size, mime, file_type, modified],
    )?;

    // Also handle dir+path conflict (file moved but same path)
    conn.execute(
        "DELETE FROM content_index WHERE dir = ?1 AND path = ?2 AND cid != ?3",
        params![dir_label, rel_path, cid],
    )?;

    // Generate thumbnail for image/raw files
    let mut thumbnail_bytes = None;
    if file_type == "image" || file_type == "raw" {
        let has_thumb: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM content_thumbnails WHERE cid = ?1",
                params![cid],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_thumb {
            match generate_and_store_thumbnail(&conn, &cid, file_type, path) {
                Ok(bytes) => {
                    thumbnail_bytes = Some(bytes);
                }
                Err(e) => {
                    tracing::debug!(
                        "Thumbnail generation failed for {}: {e}",
                        path.display()
                    );
                }
            }
        }
    }

    Ok(Some(IndexedEntry {
        cid,
        filename,
        dir: dir_label.to_string(),
        path: rel_path,
        size,
        mime,
        file_type: file_type.to_string(),
        modified,
        thumbnail_bytes,
    }))
}

/// Extract EXIF DateTimeOriginal (or DateTimeDigitized, DateTime) from an image file.
/// Returns an RFC 3339 formatted date string, or None if no EXIF date is found.
fn extract_exif_date(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let exif = exif::Reader::new().read_from_container(&mut reader).ok()?;

    // Try DateTimeOriginal first, then DateTimeDigitized, then DateTime
    for tag in &[exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime] {
        if let Some(field) = exif.get_field(*tag, exif::In::PRIMARY) {
            let val = field.display_value().to_string();
            // EXIF dates are "YYYY:MM:DD HH:MM:SS" — convert to RFC 3339
            if val.len() >= 19 {
                let converted = format!(
                    "{}-{}-{}T{}",
                    &val[0..4],
                    &val[5..7],
                    &val[8..10],
                    &val[11..19]
                );
                // Validate it parses
                if chrono::NaiveDateTime::parse_from_str(&converted, "%Y-%m-%dT%H:%M:%S").is_ok() {
                    return Some(format!("{}+00:00", converted));
                }
            }
        }
    }

    None
}

/// Compute BLAKE3 hash of a file, streaming for efficiency.
fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(1 << 16, file); // 64KB buffer
    std::io::copy(&mut reader, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Generate a thumbnail and store it in the content_thumbnails table.
/// Returns the JPEG bytes for publishing to the catalog.
fn generate_and_store_thumbnail(
    conn: &rusqlite::Connection,
    cid: &str,
    file_type: &str,
    path: &Path,
) -> anyhow::Result<Vec<u8>> {
    let (max_w, max_h) = (300u32, 300u32);

    let jpeg_bytes = if file_type == "raw" {
        thumbnail::generate_raw_thumbnail(path, max_w, max_h)
            .map_err(|e| anyhow::anyhow!("RAW thumbnail error: {e}"))?
    } else {
        let bytes = std::fs::read(path)?;
        thumbnail::generate_image_thumbnail(&bytes, max_w, max_h)
            .map_err(|e| anyhow::anyhow!("Image thumbnail error: {e}"))?
    };

    // Decode to get actual dimensions
    let img = image::load_from_memory(&jpeg_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to read back thumbnail: {e}"))?;
    let width = img.width() as i32;
    let height = img.height() as i32;

    conn.execute(
        "INSERT OR REPLACE INTO content_thumbnails (cid, thumbnail, width, height)
         VALUES (?1, ?2, ?3, ?4)",
        params![cid, jpeg_bytes, width, height],
    )?;

    Ok(jpeg_bytes)
}
