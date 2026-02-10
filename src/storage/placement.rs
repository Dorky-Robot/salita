use anyhow::Result;
use iroh::blobs::Hash;
use rusqlite::params;

use crate::state::DbPool;

/// Strategy for selecting which nodes should store a blob
#[derive(Debug, Clone)]
pub struct PlacementStrategy {
    pub replication_factor: usize,
    pub capacity_weight: f32,
    pub diversity_weight: f32,
    pub reliability_weight: f32,
}

impl Default for PlacementStrategy {
    fn default() -> Self {
        Self {
            replication_factor: 3,
            capacity_weight: 0.4,
            diversity_weight: 0.3,
            reliability_weight: 0.3,
        }
    }
}

/// Node storage quota and usage information
#[derive(Debug, Clone)]
pub struct NodeStorageQuota {
    pub node_id: String,
    pub max_bytes: u64,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub share_excess: bool,
}

impl NodeStorageQuota {
    /// Available storage space
    pub fn available_bytes(&self) -> u64 {
        self.max_bytes.saturating_sub(self.used_bytes)
    }

    /// Can this node accept a chunk of this size?
    pub fn can_accept(&self, chunk_size: u64, min_free: u64) -> bool {
        let available = self.available_bytes();
        available >= chunk_size + min_free
    }

    /// Has priority space (reserved for owner's files)?
    pub fn has_priority_space(&self, chunk_size: u64) -> bool {
        let priority_available = self.reserved_bytes.saturating_sub(self.used_bytes);
        priority_available >= chunk_size
    }

    /// Capacity ratio (0.0 = full, 1.0 = empty)
    pub fn capacity_ratio(&self) -> f32 {
        if self.max_bytes == 0 {
            return 0.0;
        }
        self.available_bytes() as f32 / self.max_bytes as f32
    }
}

impl PlacementStrategy {
    /// Select N nodes to store this blob
    pub fn select_nodes(
        &self,
        blob_hash: &Hash,
        blob_size: u64,
        owner_node_id: &str,
        available_nodes: &[NodeStorageQuota],
        min_free_space: u64,
    ) -> Result<Vec<String>> {
        let mut candidates: Vec<_> = available_nodes
            .iter()
            .filter(|n| n.can_accept(blob_size, min_free_space))
            .collect();

        let mut selected = Vec::new();

        // 1. ALWAYS include owner's node if possible (priority)
        if let Some(owner) = candidates.iter().find(|n| n.node_id == owner_node_id) {
            if owner.has_priority_space(blob_size) || owner.can_accept(blob_size, min_free_space) {
                selected.push(owner_node_id.to_string());
                candidates.retain(|n| n.node_id != owner_node_id);
            }
        }

        // 2. Score remaining nodes
        let mut scored: Vec<(f32, &NodeStorageQuota)> = candidates
            .iter()
            .map(|node| {
                let score = self.score_node(node, owner_node_id);
                (score, *node)
            })
            .collect();

        // 3. Sort by score (highest first)
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // 4. Select top N-1 nodes (already have owner)
        let needed = self.replication_factor.saturating_sub(selected.len());
        for (_, node) in scored.into_iter().take(needed) {
            selected.push(node.node_id.clone());
        }

        tracing::info!(
            "Selected {} nodes for blob {} (target: {})",
            selected.len(),
            blob_hash,
            self.replication_factor
        );

        Ok(selected)
    }

    /// Score a node for suitability (higher = better)
    fn score_node(&self, node: &NodeStorageQuota, owner_id: &str) -> f32 {
        let mut score = 0.0;

        // 1. Capacity score: favor nodes with more free space
        score += node.capacity_ratio() * self.capacity_weight;

        // 2. Diversity score: different node than owner
        if node.node_id != owner_id {
            score += self.diversity_weight;
        }

        // 3. Sharing score: willing to share storage
        if node.share_excess {
            score += 0.1;
        }

        // 4. Reliability placeholder (would query uptime from mesh_nodes)
        // For now, assume all nodes equally reliable
        score += 0.5 * self.reliability_weight;

        score
    }
}

/// Report on replication status
#[derive(Debug)]
pub struct ReplicationReport {
    pub blob_hash: String,
    pub target_replicas: usize,
    pub successful_replicas: usize,
    pub failed_replicas: usize,
    pub nodes: Vec<String>,
}

/// Get node storage quotas from database
pub fn get_node_quotas(db: &DbPool) -> Result<Vec<NodeStorageQuota>> {
    let conn = db.get()?;

    let mut stmt = conn.prepare(
        "SELECT node_id, max_bytes, used_bytes, reserved_bytes, share_excess
         FROM node_storage_quotas
         WHERE max_bytes > 0",
    )?;

    let quotas = stmt
        .query_map([], |row| {
            Ok(NodeStorageQuota {
                node_id: row.get(0)?,
                max_bytes: row.get::<_, i64>(1)? as u64,
                used_bytes: row.get::<_, i64>(2)? as u64,
                reserved_bytes: row.get::<_, i64>(3)? as u64,
                share_excess: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(quotas)
}

/// Initialize or update a node's storage quota
pub fn set_node_quota(
    db: &DbPool,
    node_id: &str,
    max_bytes: u64,
    reserved_bytes: u64,
    share_excess: bool,
) -> Result<()> {
    let conn = db.get()?;

    conn.execute(
        "INSERT INTO node_storage_quotas (node_id, max_bytes, used_bytes, reserved_bytes, share_excess, last_updated)
         VALUES (?1, ?2, 0, ?3, ?4, datetime('now'))
         ON CONFLICT(node_id) DO UPDATE SET
             max_bytes = excluded.max_bytes,
             reserved_bytes = excluded.reserved_bytes,
             share_excess = excluded.share_excess,
             last_updated = datetime('now')",
        params![
            node_id,
            max_bytes as i64,
            reserved_bytes as i64,
            share_excess
        ],
    )?;

    Ok(())
}

/// Update node's used storage after adding/removing a blob
pub fn update_storage_usage(db: &DbPool, node_id: &str, delta_bytes: i64) -> Result<()> {
    let conn = db.get()?;

    conn.execute(
        "UPDATE node_storage_quotas
         SET used_bytes = MAX(0, used_bytes + ?1),
             last_updated = datetime('now')
         WHERE node_id = ?2",
        params![delta_bytes, node_id],
    )?;

    Ok(())
}

/// Track that a blob is stored on a node
pub fn record_blob_storage(
    db: &DbPool,
    node_id: &str,
    blob_hash: &str,
    size_bytes: u64,
    priority: u8,
) -> Result<()> {
    let conn = db.get()?;

    conn.execute(
        "INSERT INTO node_blob_storage (node_id, blob_hash, size_bytes, stored_at, priority, last_accessed)
         VALUES (?1, ?2, ?3, datetime('now'), ?4, datetime('now'))
         ON CONFLICT(node_id, blob_hash) DO UPDATE SET
             last_accessed = datetime('now')",
        params![node_id, blob_hash, size_bytes as i64, priority],
    )?;

    // Also update blob_locations for tracking
    conn.execute(
        "INSERT INTO blob_locations (blob_hash, node_id, added_at, last_verified)
         VALUES (?1, ?2, datetime('now'), datetime('now'))
         ON CONFLICT(blob_hash, node_id) DO UPDATE SET
             last_verified = datetime('now')",
        params![blob_hash, node_id],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_quota_can_accept() {
        let quota = NodeStorageQuota {
            node_id: "test".to_string(),
            max_bytes: 1000,
            used_bytes: 500,
            reserved_bytes: 200,
            share_excess: true,
        };

        // Has 500 bytes free, needs 100 + 50 min_free
        assert!(quota.can_accept(100, 50));

        // Needs 600 bytes but only 500 free
        assert!(!quota.can_accept(500, 100));
    }

    #[test]
    fn test_placement_prefers_owner() {
        let strategy = PlacementStrategy::default();

        let nodes = vec![
            NodeStorageQuota {
                node_id: "owner".to_string(),
                max_bytes: 1000,
                used_bytes: 100,
                reserved_bytes: 200,
                share_excess: true,
            },
            NodeStorageQuota {
                node_id: "other1".to_string(),
                max_bytes: 10000,
                used_bytes: 100,
                reserved_bytes: 0,
                share_excess: true,
            },
            NodeStorageQuota {
                node_id: "other2".to_string(),
                max_bytes: 10000,
                used_bytes: 100,
                reserved_bytes: 0,
                share_excess: true,
            },
        ];

        let hash = Hash::from_bytes([0u8; 32]);
        let selected = strategy
            .select_nodes(&hash, 50, "owner", &nodes, 10)
            .unwrap();

        // Owner should always be first
        assert_eq!(selected[0], "owner");
    }

    #[test]
    fn test_capacity_ratio() {
        let quota = NodeStorageQuota {
            node_id: "test".to_string(),
            max_bytes: 1000,
            used_bytes: 700,
            reserved_bytes: 0,
            share_excess: true,
        };

        assert_eq!(quota.capacity_ratio(), 0.3);
    }
}
