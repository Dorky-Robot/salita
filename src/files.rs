use crate::config::Config;
use crate::error::{AppError, AppResult};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, Clone)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
    pub mime_type: Option<String>,
}

/// Resolve a directory label to its filesystem path, validating it exists
pub fn resolve_dir(config: &Config, label: &str) -> AppResult<PathBuf> {
    config
        .resolve_directory(label)
        .ok_or_else(|| AppError::NotFound)
}

/// Resolve a relative path within a directory, rejecting traversal attacks
pub fn resolve_path(base_dir: &PathBuf, rel_path: &str) -> AppResult<PathBuf> {
    let cleaned = rel_path.trim_start_matches('/');
    if cleaned.contains("..") {
        return Err(AppError::BadRequest("Path traversal not allowed".into()));
    }

    let full_path = base_dir.join(cleaned);

    let canonical_base = base_dir.canonicalize().unwrap_or_else(|_| base_dir.clone());
    let canonical_full = full_path
        .canonicalize()
        .unwrap_or_else(|_| full_path.clone());

    if !canonical_full.starts_with(&canonical_base) {
        return Err(AppError::BadRequest("Path outside directory".into()));
    }

    Ok(full_path)
}

/// List files in a directory
pub fn list_files(config: &Config, label: &str, rel_path: &str) -> AppResult<Vec<FileEntry>> {
    let base = resolve_dir(config, label)?;
    let dir_path = if rel_path.is_empty() || rel_path == "." {
        base.clone()
    } else {
        resolve_path(&base, rel_path)?
    };

    if !dir_path.is_dir() {
        return Err(AppError::BadRequest("Not a directory".into()));
    }

    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(&dir_path)
        .map_err(|e| AppError::Internal(format!("Failed to read directory: {}", e)))?;

    for entry in read_dir {
        let entry = entry.map_err(|e| AppError::Internal(format!("Failed to read entry: {}", e)))?;
        let metadata = entry
            .metadata()
            .map_err(|e| AppError::Internal(format!("Failed to read metadata: {}", e)))?;

        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let entry_rel_path = if rel_path.is_empty() || rel_path == "." {
            name.clone()
        } else {
            format!("{}/{}", rel_path.trim_end_matches('/'), name)
        };

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                    })
            });

        entries.push(FileEntry {
            name,
            path: entry_rel_path,
            is_dir: metadata.is_dir(),
            size: if metadata.is_file() { metadata.len() } else { 0 },
            modified,
        });
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(entries)
}

/// Search for files matching a glob pattern
pub fn search_files(
    config: &Config,
    pattern: &str,
    label: Option<&str>,
) -> AppResult<Vec<FileEntry>> {
    let dirs_to_search: Vec<(String, PathBuf)> = if let Some(label) = label {
        let base = resolve_dir(config, label)?;
        vec![(label.to_string(), base)]
    } else {
        config
            .directories
            .iter()
            .filter_map(|d| {
                config
                    .resolve_directory(&d.label)
                    .map(|p| (d.label.clone(), p))
            })
            .collect()
    };

    let mut results = Vec::new();

    for (dir_label, base_path) in &dirs_to_search {
        let glob_pattern = format!("{}/{}", base_path.display(), pattern);
        let paths = glob::glob(&glob_pattern)
            .map_err(|e| AppError::BadRequest(format!("Invalid glob pattern: {}", e)))?;

        for path_result in paths {
            let path = match path_result {
                Ok(p) => p,
                Err(_) => continue,
            };

            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let rel = path
                .strip_prefix(base_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .ok()
                        .and_then(|d| {
                            chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                                .map(|dt| dt.to_rfc3339())
                        })
                });

            results.push(FileEntry {
                name,
                path: format!("{}:{}", dir_label, rel),
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() { metadata.len() } else { 0 },
                modified,
            });
        }
    }

    Ok(results)
}

/// Read file content (text or base64 for binary)
pub fn read_file(config: &Config, label: &str, rel_path: &str) -> AppResult<String> {
    let base = resolve_dir(config, label)?;
    let file_path = resolve_path(&base, rel_path)?;

    if !file_path.is_file() {
        return Err(AppError::NotFound);
    }

    let metadata = std::fs::metadata(&file_path)
        .map_err(|e| AppError::Internal(format!("Failed to read metadata: {}", e)))?;

    if metadata.len() as usize > config.max_read_bytes {
        return Err(AppError::BadRequest(format!(
            "File too large: {} bytes (max {})",
            metadata.len(),
            config.max_read_bytes
        )));
    }

    let content = std::fs::read_to_string(&file_path).unwrap_or_else(|_| {
        // Binary file — return base64
        use std::io::Read;
        let mut buf = Vec::new();
        if let Ok(mut f) = std::fs::File::open(&file_path) {
            let _ = f.read_to_end(&mut buf);
        }
        format!("[binary file, {} bytes]", buf.len())
    });

    Ok(content)
}

/// Read raw file bytes (for HTTP streaming)
pub fn read_file_bytes(config: &Config, label: &str, rel_path: &str) -> AppResult<Vec<u8>> {
    let base = resolve_dir(config, label)?;
    let file_path = resolve_path(&base, rel_path)?;

    if !file_path.is_file() {
        return Err(AppError::NotFound);
    }

    std::fs::read(&file_path).map_err(|e| AppError::Internal(format!("Failed to read file: {}", e)))
}

/// Get file metadata
pub fn file_info(config: &Config, label: &str, rel_path: &str) -> AppResult<FileInfo> {
    let base = resolve_dir(config, label)?;
    let file_path = resolve_path(&base, rel_path)?;

    if !file_path.exists() {
        return Err(AppError::NotFound);
    }

    let metadata = std::fs::metadata(&file_path)
        .map_err(|e| AppError::Internal(format!("Failed to read metadata: {}", e)))?;

    let name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let mime_type = if metadata.is_file() {
        Some(
            mime_guess::from_path(&file_path)
                .first_or_octet_stream()
                .to_string(),
        )
    } else {
        None
    };

    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.to_rfc3339())
                })
        });

    Ok(FileInfo {
        name,
        path: rel_path.to_string(),
        is_dir: metadata.is_dir(),
        size: if metadata.is_file() { metadata.len() } else { 0 },
        modified,
        mime_type,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DirectoryConfig;

    fn test_config(dir: &std::path::Path) -> Config {
        Config {
            directories: vec![DirectoryConfig {
                label: "test".to_string(),
                path: dir.to_string_lossy().to_string(),
            }],
            ..Config::default()
        }
    }

    #[test]
    fn resolve_path_rejects_traversal() {
        let base = PathBuf::from("/tmp");
        assert!(resolve_path(&base, "../../etc/passwd").is_err());
        assert!(resolve_path(&base, "foo/../../../etc/passwd").is_err());
    }

    #[test]
    fn resolve_path_allows_normal_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("file.txt"), "hello").unwrap();

        let base = tmp.path().to_path_buf();
        let result = resolve_path(&base, "sub/file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn list_files_works() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "world").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();
        std::fs::write(tmp.path().join(".hidden"), "secret").unwrap();

        let config = test_config(tmp.path());
        let entries = list_files(&config, "test", "").unwrap();

        assert_eq!(entries.len(), 3); // a.txt, b.txt, subdir (not .hidden)
        assert!(entries[0].is_dir); // subdir first
    }

    #[test]
    fn read_file_works() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "Hello, Salita!").unwrap();

        let config = test_config(tmp.path());
        let content = read_file(&config, "test", "hello.txt").unwrap();
        assert_eq!(content, "Hello, Salita!");
    }

    #[test]
    fn file_info_works() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("info.txt"), "content").unwrap();

        let config = test_config(tmp.path());
        let info = file_info(&config, "test", "info.txt").unwrap();
        assert_eq!(info.name, "info.txt");
        assert_eq!(info.size, 7);
        assert!(!info.is_dir);
        assert_eq!(info.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn search_files_works() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "world").unwrap();

        let config = test_config(tmp.path());
        let results = search_files(&config, "*.txt", Some("test")).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name.ends_with(".txt"));
    }
}
