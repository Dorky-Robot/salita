use anyhow::Result;
use iroh::blobs::Hash;
use rusqlite::params;

use crate::state::DbPool;

/// Metadata for a file being uploaded
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub name: String,
    pub mime_type: Option<String>,
    pub owner_id: String,
    pub size: u64,
}

/// A file record from the database
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub id: String,
    pub hash: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: u64,
    pub owner_id: String,
    pub created_at: String,
}

/// Store file metadata in SQLite
pub fn store_file_metadata(db: &DbPool, hash: &Hash, metadata: FileMetadata) -> Result<String> {
    let conn = db.get()?;
    let file_id = uuid::Uuid::now_v7().to_string();

    conn.execute(
        "INSERT INTO files_dss (id, hash, name, mime_type, size, owner_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        params![
            &file_id,
            &hash.to_string(),
            &metadata.name,
            &metadata.mime_type,
            metadata.size as i64,
            &metadata.owner_id,
        ],
    )?;

    Ok(file_id)
}

/// Get file metadata by ID
pub fn get_file_metadata(db: &DbPool, file_id: &str) -> Result<FileRecord> {
    let conn = db.get()?;

    let record = conn.query_row(
        "SELECT id, hash, name, mime_type, size, owner_id, created_at
         FROM files_dss
         WHERE id = ?1",
        params![file_id],
        |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                hash: row.get(1)?,
                name: row.get(2)?,
                mime_type: row.get(3)?,
                size: row.get::<_, i64>(4)? as u64,
                owner_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )?;

    Ok(record)
}

/// List all files for a user
pub fn list_user_files(db: &DbPool, user_id: &str) -> Result<Vec<FileRecord>> {
    let conn = db.get()?;

    let mut stmt = conn.prepare(
        "SELECT id, hash, name, mime_type, size, owner_id, created_at
         FROM files_dss
         WHERE owner_id = ?1
         ORDER BY created_at DESC",
    )?;

    let records = stmt
        .query_map(params![user_id], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                hash: row.get(1)?,
                name: row.get(2)?,
                mime_type: row.get(3)?,
                size: row.get::<_, i64>(4)? as u64,
                owner_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(records)
}

/// Delete file metadata
pub fn delete_file_metadata(db: &DbPool, file_id: &str) -> Result<()> {
    let conn = db.get()?;
    conn.execute("DELETE FROM files_dss WHERE id = ?1", params![file_id])?;
    Ok(())
}

/// Count total files
pub fn count_files(db: &DbPool) -> Result<u64> {
    let conn = db.get()?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM files_dss", [], |row| row.get(0))?;
    Ok(count as u64)
}

/// Get total size of all files
pub fn total_size(db: &DbPool) -> Result<u64> {
    let conn = db.get()?;
    let size: i64 = conn.query_row("SELECT COALESCE(SUM(size), 0) FROM files_dss", [], |row| {
        row.get(0)
    })?;
    Ok(size as u64)
}
