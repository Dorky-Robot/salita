use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::http::HttpState;

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/api/v1/content/{cid}", get(serve_content))
        .route("/api/v1/content/{cid}/thumbnail", get(serve_thumbnail))
        .route("/api/v1/content/{cid}/info", get(content_info))
        .route("/api/v1/content/{cid}/preview", get(serve_preview))
        .route("/api/v1/catalog", get(catalog))
        .route("/api/v1/catalog/stats", get(catalog_stats))
        .route("/api/v1/index", post(index_on_demand))
}

// --- Serve file bytes by CID ---

async fn serve_content(
    State(state): State<HttpState>,
    Path(cid): Path<String>,
) -> AppResult<Response> {
    let conn = state.db.get()?;

    let (dir, path): (String, String) = conn
        .query_row(
            "SELECT dir, path FROM content_index WHERE cid = ?1",
            params![cid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| AppError::NotFound)?;

    let base = state
        .config
        .resolve_directory(&dir)
        .ok_or(AppError::NotFound)?;
    let file_path = base.join(&path);

    if !file_path.is_file() {
        return Err(AppError::NotFound);
    }

    let bytes = tokio::fs::read(&file_path).await.map_err(|e| {
        AppError::Internal(format!("Failed to read file: {e}"))
    })?;

    let content_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{}\"", filename),
        )
        .body(Body::from(bytes))
        .unwrap())
}

// --- Serve pre-generated thumbnail ---

#[derive(Deserialize)]
#[allow(dead_code)]
struct ThumbnailParams {
    #[serde(default = "default_thumb_size")]
    w: u32,
    #[serde(default = "default_thumb_size")]
    h: u32,
}

fn default_thumb_size() -> u32 {
    300
}

async fn serve_thumbnail(
    State(state): State<HttpState>,
    Path(cid): Path<String>,
    Query(_params): Query<ThumbnailParams>,
) -> AppResult<Response> {
    let db = state.db.clone();
    let cid_clone = cid.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.get()?;
        let thumbnail: Vec<u8> = conn
            .query_row(
                "SELECT thumbnail FROM content_thumbnails WHERE cid = ?1",
                params![cid_clone],
                |row| row.get(0),
            )
            .map_err(|_| AppError::NotFound)?;
        Ok::<Vec<u8>, AppError>(thumbnail)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task join error: {e}")))?;

    let thumbnail = result?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(thumbnail))
        .unwrap())
}

// --- Serve mid-res preview (1600px JPEG, generated on-demand) ---

async fn serve_preview(
    State(state): State<HttpState>,
    Path(cid): Path<String>,
) -> AppResult<Response> {
    let db = state.db.clone();
    let config = state.config.clone();
    let cid_clone = cid.clone();

    let result = tokio::task::spawn_blocking(move || {
        let conn = db.get()?;

        // Check if preview already cached in DB
        let existing: Option<Vec<u8>> = conn
            .query_row(
                "SELECT preview FROM content_previews WHERE cid = ?1",
                params![cid_clone],
                |row| row.get(0),
            )
            .ok();

        if let Some(preview) = existing {
            return Ok::<Vec<u8>, AppError>(preview);
        }

        // Generate on-demand: look up the file path
        let (dir, path, file_type): (String, String, String) = conn
            .query_row(
                "SELECT dir, path, file_type FROM content_index WHERE cid = ?1",
                params![cid_clone],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| AppError::NotFound)?;

        let base = config
            .resolve_directory(&dir)
            .ok_or(AppError::NotFound)?;
        let file_path = base.join(&path);

        if !file_path.is_file() {
            return Err(AppError::NotFound);
        }

        let preview_bytes = if file_type == "raw" {
            crate::thumbnail::generate_raw_preview(&file_path)
                .map_err(|e| AppError::Internal(format!("RAW preview error: {e}")))?
        } else {
            let bytes = std::fs::read(&file_path)
                .map_err(|e| AppError::Internal(format!("Read error: {e}")))?;
            crate::thumbnail::generate_image_preview(&bytes)
                .map_err(|e| AppError::Internal(format!("Preview error: {e}")))?
        };

        // Decode to get dimensions and cache
        if let Ok(img) = image::load_from_memory(&preview_bytes) {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO content_previews (cid, preview, width, height)
                 VALUES (?1, ?2, ?3, ?4)",
                params![cid_clone, preview_bytes, img.width() as i32, img.height() as i32],
            );
        }

        Ok(preview_bytes)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task join error: {e}")))?;

    let preview = result?;

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "image/jpeg")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(preview))
        .unwrap())
}

// --- Content metadata ---

#[derive(Serialize)]
struct ContentInfoResponse {
    cid: String,
    dir: String,
    path: String,
    filename: String,
    size: i64,
    mime: Option<String>,
    file_type: String,
    modified: Option<String>,
    indexed_at: String,
    has_thumbnail: bool,
}

async fn content_info(
    State(state): State<HttpState>,
    Path(cid): Path<String>,
) -> AppResult<impl IntoResponse> {
    let conn = state.db.get()?;

    let info = conn
        .query_row(
            "SELECT cid, dir, path, filename, size, mime, file_type, modified, indexed_at
             FROM content_index WHERE cid = ?1",
            params![cid],
            |row| {
                Ok(ContentInfoResponse {
                    cid: row.get(0)?,
                    dir: row.get(1)?,
                    path: row.get(2)?,
                    filename: row.get(3)?,
                    size: row.get(4)?,
                    mime: row.get(5)?,
                    file_type: row.get(6)?,
                    modified: row.get(7)?,
                    indexed_at: row.get(8)?,
                    has_thumbnail: false,
                })
            },
        )
        .map_err(|_| AppError::NotFound)?;

    let has_thumbnail: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM content_thumbnails WHERE cid = ?1",
            params![cid],
            |row| row.get(0),
        )
        .unwrap_or(false);

    let info = ContentInfoResponse {
        has_thumbnail,
        ..info
    };

    Ok(Json(info))
}

// --- Catalog stats ---

#[derive(Serialize)]
struct CatalogStats {
    total_files: i64,
    indexed_files: i64,
    with_thumbnails: i64,
    with_previews: i64,
    dirs: Vec<DirStats>,
}

#[derive(Serialize)]
struct DirStats {
    dir: String,
    total: i64,
    indexed: i64,
    thumbnails: i64,
}

async fn catalog_stats(
    State(state): State<HttpState>,
) -> AppResult<impl IntoResponse> {
    let config = state.config.clone();
    let pool = state.db.clone();

    let stats = tokio::task::spawn_blocking(move || {
        let conn = pool.get()?;

        // Count total files on disk per directory
        let mut dirs = Vec::new();
        for dir_config in &config.directories {
            let base = config.resolve_directory(&dir_config.label);
            let total = match base {
                Some(ref b) if b.is_dir() => count_files_recursive(b),
                _ => 0,
            };

            let indexed: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM content_index WHERE dir = ?1 AND is_local = 1",
                    params![dir_config.label],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let thumbnails: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM content_index ci
                     INNER JOIN content_thumbnails ct ON ci.cid = ct.cid
                     WHERE ci.dir = ?1 AND ci.is_local = 1",
                    params![dir_config.label],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            dirs.push(DirStats {
                dir: dir_config.label.clone(),
                total: total as i64,
                indexed,
                thumbnails,
            });
        }

        let total_files: i64 = dirs.iter().map(|d| d.total).sum();
        let indexed_files: i64 = dirs.iter().map(|d| d.indexed).sum();
        let with_thumbnails: i64 = dirs.iter().map(|d| d.thumbnails).sum();

        let with_previews: i64 = conn
            .query_row("SELECT COUNT(*) FROM content_previews", [], |row| row.get(0))
            .unwrap_or(0);

        Ok::<CatalogStats, AppError>(CatalogStats {
            total_files,
            indexed_files,
            with_thumbnails,
            with_previews,
            dirs,
        })
    })
    .await
    .map_err(|e| AppError::Internal(format!("Stats error: {e}")))?;

    Ok(Json(stats?))
}

fn count_files_recursive(dir: &std::path::Path) -> u64 {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                count += count_files_recursive(&path);
            } else if path.is_file() {
                count += 1;
            }
        }
    }
    count
}

// --- Paginated catalog ---

#[derive(Deserialize)]
struct CatalogParams {
    dir: Option<String>,
    file_type: Option<String>,
    since: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct CatalogEntry {
    cid: String,
    dir: String,
    path: String,
    filename: String,
    size: i64,
    mime: Option<String>,
    file_type: String,
    modified: Option<String>,
    has_thumbnail: bool,
    has_preview: bool,
}

async fn catalog(
    State(state): State<HttpState>,
    Query(params): Query<CatalogParams>,
) -> AppResult<impl IntoResponse> {
    let conn = state.db.get()?;

    let limit = params.limit.unwrap_or(100).min(50000);
    let offset = params.offset.unwrap_or(0);

    let mut sql = String::from(
        "SELECT ci.cid, ci.dir, ci.path, ci.filename, ci.size, ci.mime, ci.file_type, ci.modified,
                (ct.cid IS NOT NULL) as has_thumb,
                (cp.cid IS NOT NULL) as has_prev
         FROM content_index ci
         LEFT JOIN content_thumbnails ct ON ci.cid = ct.cid
         LEFT JOIN content_previews cp ON ci.cid = cp.cid
         WHERE 1=1",
    );
    let mut bind_values: Vec<String> = Vec::new();

    if let Some(ref dir) = params.dir {
        bind_values.push(dir.clone());
        sql.push_str(&format!(" AND ci.dir = ?{}", bind_values.len()));
    }
    if let Some(ref ft) = params.file_type {
        bind_values.push(ft.clone());
        sql.push_str(&format!(" AND ci.file_type = ?{}", bind_values.len()));
    }
    if let Some(ref since) = params.since {
        bind_values.push(since.clone());
        sql.push_str(&format!(" AND ci.indexed_at > ?{}", bind_values.len()));
    }

    sql.push_str(" ORDER BY ci.modified DESC NULLS LAST");
    bind_values.push(limit.to_string());
    sql.push_str(&format!(" LIMIT ?{}", bind_values.len()));
    bind_values.push(offset.to_string());
    sql.push_str(&format!(" OFFSET ?{}", bind_values.len()));

    let mut stmt = conn.prepare(&sql)?;

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = bind_values
        .iter()
        .map(|v| v as &dyn rusqlite::types::ToSql)
        .collect();

    let entries = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(CatalogEntry {
                cid: row.get(0)?,
                dir: row.get(1)?,
                path: row.get(2)?,
                filename: row.get(3)?,
                size: row.get(4)?,
                mime: row.get(5)?,
                file_type: row.get(6)?,
                modified: row.get(7)?,
                has_thumbnail: row.get(8)?,
                has_preview: row.get(9)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>();

    Ok(Json(entries))
}

// --- On-demand indexing ---

#[derive(Deserialize)]
struct IndexRequest {
    dir: String,
    paths: Vec<String>,
}

#[derive(Serialize)]
struct IndexResult {
    path: String,
    cid: Option<String>,
    has_thumbnail: bool,
}

/// Index specific files on demand. Called by gunita when a user browses a page
/// so that thumbnails for the visible files are prioritized.
async fn index_on_demand(
    State(state): State<HttpState>,
    Json(body): Json<IndexRequest>,
) -> AppResult<impl IntoResponse> {
    let config = state.config.clone();
    let pool = state.db.clone();
    let dir = body.dir;
    let paths = body.paths;

    let results = tokio::task::spawn_blocking(move || {
        let base = match config.resolve_directory(&dir) {
            Some(b) if b.is_dir() => b,
            _ => return Vec::new(),
        };

        let mut results = Vec::new();
        for rel_path in &paths {
            let file_path = base.join(rel_path);
            if !file_path.is_file() {
                results.push(IndexResult {
                    path: rel_path.clone(),
                    cid: None,
                    has_thumbnail: false,
                });
                continue;
            }

            // Check if already indexed with thumbnail
            if let Ok(conn) = pool.get() {
                let existing: Option<(String, bool)> = conn
                    .query_row(
                        "SELECT ci.cid, (ct.cid IS NOT NULL)
                         FROM content_index ci
                         LEFT JOIN content_thumbnails ct ON ci.cid = ct.cid
                         WHERE ci.dir = ?1 AND ci.path = ?2",
                        rusqlite::params![dir, rel_path],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();

                if let Some((cid, has_thumb)) = existing {
                    if has_thumb {
                        results.push(IndexResult {
                            path: rel_path.clone(),
                            cid: Some(cid),
                            has_thumbnail: true,
                        });
                        continue;
                    }
                }
            }

            // Index this file now
            match crate::indexer::index_file(&pool, &dir, &base, &file_path) {
                Ok(Some(entry)) => {
                    results.push(IndexResult {
                        path: rel_path.clone(),
                        has_thumbnail: entry.thumbnail_bytes.is_some(),
                        cid: Some(entry.cid),
                    });
                }
                Ok(None) => {
                    // Was already indexed (race with background indexer)
                    if let Ok(conn) = pool.get() {
                        let cid: Option<String> = conn
                            .query_row(
                                "SELECT cid FROM content_index WHERE dir = ?1 AND path = ?2",
                                rusqlite::params![dir, rel_path],
                                |row| row.get(0),
                            )
                            .ok();
                        let has_thumb = cid.as_ref().map_or(false, |c| {
                            conn.query_row(
                                "SELECT COUNT(*) > 0 FROM content_thumbnails WHERE cid = ?1",
                                rusqlite::params![c],
                                |row| row.get::<_, bool>(0),
                            )
                            .unwrap_or(false)
                        });
                        results.push(IndexResult {
                            path: rel_path.clone(),
                            cid,
                            has_thumbnail: has_thumb,
                        });
                    }
                }
                Err(_) => {
                    results.push(IndexResult {
                        path: rel_path.clone(),
                        cid: None,
                        has_thumbnail: false,
                    });
                }
            }
        }

        results
    })
    .await
    .map_err(|e| AppError::Internal(format!("Index task error: {e}")))?;

    Ok(Json(results))
}
