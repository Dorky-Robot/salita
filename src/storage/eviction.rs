use anyhow::Result;
use rusqlite::params;

use crate::state::DbPool;

/// Strategy for evicting blobs when storage is full
#[derive(Debug, Clone, Copy)]
pub enum EvictionStrategy {
    /// Least Recently Used - evict oldest accessed first
    LRU,
    /// Priority-based - evict lowest priority first
    Priority,
    /// Redundant-only - only evict if copies exist elsewhere
    Redundant,
    /// Hybrid - combination of priority, redundancy, and age
    Hybrid,
}

/// A blob that's a candidate for eviction
#[derive(Debug)]
pub struct EvictionCandidate {
    pub blob_hash: String,
    pub size_bytes: u64,
    pub priority: u8,
    pub last_accessed: String,
    pub pin_count: i32,
}

pub struct EvictionManager {
    db: DbPool,
    strategy: EvictionStrategy,
}

impl EvictionManager {
    pub fn new(db: DbPool, strategy: EvictionStrategy) -> Self {
        Self { db, strategy }
    }

    /// Find blobs to evict to free up the requested space
    pub fn find_candidates(
        &self,
        node_id: &str,
        bytes_needed: u64,
    ) -> Result<Vec<EvictionCandidate>> {
        let conn = self.db.get()?;

        let query = self.get_query();

        let mut stmt = conn.prepare(query)?;
        let mut candidates = Vec::new();
        let mut freed = 0u64;

        let rows = stmt.query_map(params![node_id], |row| {
            Ok(EvictionCandidate {
                blob_hash: row.get(0)?,
                size_bytes: row.get::<_, i64>(1)? as u64,
                priority: row.get(2)?,
                last_accessed: row.get(3)?,
                pin_count: row.get(4)?,
            })
        })?;

        for candidate in rows {
            let candidate = candidate?;

            // Never evict pinned blobs
            if candidate.pin_count > 0 {
                continue;
            }

            freed += candidate.size_bytes;
            candidates.push(candidate);

            if freed >= bytes_needed {
                break;
            }
        }

        tracing::info!(
            "Found {} eviction candidates for {} (need {} bytes, can free {})",
            candidates.len(),
            node_id,
            bytes_needed,
            freed
        );

        Ok(candidates)
    }

    /// Get SQL query based on eviction strategy
    fn get_query(&self) -> &str {
        match self.strategy {
            EvictionStrategy::LRU => {
                "SELECT blob_hash, size_bytes, priority, last_accessed, pin_count
                 FROM node_blob_storage
                 WHERE node_id = ?1
                 ORDER BY last_accessed ASC, priority ASC"
            }
            EvictionStrategy::Priority => {
                "SELECT blob_hash, size_bytes, priority, last_accessed, pin_count
                 FROM node_blob_storage
                 WHERE node_id = ?1
                 ORDER BY priority ASC, last_accessed ASC"
            }
            EvictionStrategy::Redundant => {
                "SELECT nbs.blob_hash, nbs.size_bytes, nbs.priority, nbs.last_accessed, nbs.pin_count
                 FROM node_blob_storage nbs
                 WHERE nbs.node_id = ?1
                   AND (SELECT COUNT(*) FROM blob_locations bl
                        WHERE bl.blob_hash = nbs.blob_hash) >= 2
                 ORDER BY nbs.priority ASC, nbs.last_accessed ASC"
            }
            EvictionStrategy::Hybrid => {
                // Score based on priority (lower better), age (older better), redundancy (more copies better)
                "SELECT blob_hash, size_bytes, priority, last_accessed, pin_count
                 FROM node_blob_storage
                 WHERE node_id = ?1
                 ORDER BY
                   (priority * 2 +
                    (CASE WHEN (SELECT COUNT(*) FROM blob_locations bl WHERE bl.blob_hash = blob_hash) >= 3
                          THEN -5 ELSE 0 END) +
                    (julianday('now') - julianday(last_accessed)) * -0.1
                   ) ASC"
            }
        }
    }

    /// Evict the specified blobs from storage
    pub fn evict(&self, node_id: &str, candidates: Vec<EvictionCandidate>) -> Result<u64> {
        let conn = self.db.get()?;
        let mut freed = 0u64;

        for candidate in candidates {
            // Remove from node_blob_storage
            conn.execute(
                "DELETE FROM node_blob_storage
                 WHERE node_id = ?1 AND blob_hash = ?2",
                params![node_id, &candidate.blob_hash],
            )?;

            // Remove from blob_locations
            conn.execute(
                "DELETE FROM blob_locations
                 WHERE node_id = ?1 AND blob_hash = ?2",
                params![node_id, &candidate.blob_hash],
            )?;

            // Update quota
            conn.execute(
                "UPDATE node_storage_quotas
                 SET used_bytes = MAX(0, used_bytes - ?1),
                     last_updated = datetime('now')
                 WHERE node_id = ?2",
                params![candidate.size_bytes as i64, node_id],
            )?;

            freed += candidate.size_bytes;

            tracing::info!(
                "Evicted blob {} from {} (freed {} bytes, priority={})",
                candidate.blob_hash,
                node_id,
                candidate.size_bytes,
                candidate.priority
            );
        }

        Ok(freed)
    }

    /// Check if eviction is needed and perform it
    pub fn evict_if_needed(
        &self,
        node_id: &str,
        incoming_size: u64,
        min_free_space: u64,
    ) -> Result<bool> {
        let conn = self.db.get()?;

        // Get current quota status
        let (max_bytes, used_bytes): (i64, i64) = conn.query_row(
            "SELECT max_bytes, used_bytes FROM node_storage_quotas WHERE node_id = ?1",
            params![node_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        let available = (max_bytes as u64).saturating_sub(used_bytes as u64);
        let required = incoming_size + min_free_space;

        if available >= required {
            // No eviction needed
            return Ok(false);
        }

        // Need to free up space
        let bytes_needed = required - available;

        tracing::info!(
            "Node {} needs to free {} bytes (available: {}, required: {})",
            node_id,
            bytes_needed,
            available,
            required
        );

        let candidates = self.find_candidates(node_id, bytes_needed)?;

        if candidates.is_empty() {
            anyhow::bail!("Cannot evict enough space - no candidates available");
        }

        let freed = self.evict(node_id, candidates)?;

        if freed < bytes_needed {
            tracing::warn!("Only freed {} bytes, needed {}", freed, bytes_needed);
        }

        Ok(true)
    }
}

/// Pin/unpin blobs to prevent eviction
pub fn pin_blob(db: &DbPool, node_id: &str, blob_hash: &str) -> Result<()> {
    let conn = db.get()?;

    conn.execute(
        "UPDATE node_blob_storage
         SET pin_count = pin_count + 1
         WHERE node_id = ?1 AND blob_hash = ?2",
        params![node_id, blob_hash],
    )?;

    Ok(())
}

pub fn unpin_blob(db: &DbPool, node_id: &str, blob_hash: &str) -> Result<()> {
    let conn = db.get()?;

    conn.execute(
        "UPDATE node_blob_storage
         SET pin_count = MAX(0, pin_count - 1)
         WHERE node_id = ?1 AND blob_hash = ?2",
        params![node_id, blob_hash],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

    #[test]
    #[ignore] // TODO: Enable after migration 010 is merged
    fn test_eviction_respects_pins() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = db::create_pool(&db_path).unwrap();
        db::run_migrations(&pool).unwrap();

        // Insert test data
        let conn = pool.get().unwrap();
        conn.execute(
            "INSERT INTO node_storage_quotas (node_id, max_bytes, used_bytes, reserved_bytes, share_excess, last_updated)
             VALUES ('test-node', 1000, 900, 100, 1, datetime('now'))",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO node_blob_storage (node_id, blob_hash, size_bytes, stored_at, priority, pin_count)
             VALUES ('test-node', 'pinned-blob', 100, datetime('now'), 1, 1),
                    ('test-node', 'unpinned-blob', 100, datetime('now'), 1, 0)",
            [],
        ).unwrap();

        let manager = EvictionManager::new(pool, EvictionStrategy::LRU);
        let candidates = manager.find_candidates("test-node", 50).unwrap();

        // Should only find unpinned blob
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].blob_hash, "unpinned-blob");
    }
}
