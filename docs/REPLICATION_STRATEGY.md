# Replication Strategy & Storage Management

> How chunks are distributed across mesh nodes and how storage quotas are managed

## Core Principles

1. **User-configurable quotas** - Each device declares how much storage it's willing to contribute
2. **Weighted replication** - Distribute chunks based on available capacity
3. **Redundancy targets** - Ensure N copies of each chunk exist in the mesh
4. **Smart placement** - Prefer diverse nodes (different networks, power sources, etc.)
5. **Fair distribution** - Balance load across nodes
6. **Priority system** - Critical files get more copies, personal files guaranteed on owner's device

## Storage Quota Management

### Per-Node Configuration

Each node declares its storage limits in config:

```toml
[storage]
enabled = true
max_storage_gb = 100          # Maximum storage this node will dedicate
reserved_gb = 10              # Reserved for local files (always available)
share_excess = true           # Share unused space with mesh
min_free_space_gb = 5         # Stop accepting chunks when this low

# Quota management
auto_evict = true             # Automatically remove chunks when full
eviction_strategy = "lru"     # lru | priority | random
```

### Storage States

Each node tracks its storage:

```rust
pub struct NodeStorageQuota {
    pub node_id: String,
    pub max_bytes: u64,           // User-configured max
    pub used_bytes: u64,          // Currently used
    pub reserved_bytes: u64,      // Reserved for owner's files
    pub available_bytes: u64,     // max - used
    pub share_excess: bool,       // Willing to store others' files?
    pub last_updated: DateTime,
}

impl NodeStorageQuota {
    /// Can this node accept a new chunk?
    pub fn can_accept(&self, chunk_size: u64) -> bool {
        let available = self.max_bytes.saturating_sub(self.used_bytes);
        available >= chunk_size + self.min_free_space()
    }

    /// Priority space: reserved for owner's files
    pub fn has_priority_space(&self, chunk_size: u64) -> bool {
        let used_reserved = self.reserved_bytes.saturating_sub(self.used_bytes);
        used_reserved >= chunk_size
    }
}
```

### Database Schema

```sql
-- Track each node's storage quota and usage
CREATE TABLE node_storage_quotas (
    node_id TEXT PRIMARY KEY,
    max_bytes INTEGER NOT NULL,
    used_bytes INTEGER NOT NULL DEFAULT 0,
    reserved_bytes INTEGER NOT NULL,
    share_excess BOOLEAN DEFAULT 1,
    last_updated TEXT NOT NULL,
    FOREIGN KEY (node_id) REFERENCES mesh_nodes(id)
);

-- Track actual blob storage per node
CREATE TABLE node_blob_storage (
    node_id TEXT NOT NULL,
    blob_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    stored_at TEXT NOT NULL,
    priority INTEGER DEFAULT 5,        -- 0-10, higher = more important
    last_accessed TEXT,
    pin_count INTEGER DEFAULT 0,       -- User pins (prevent eviction)
    PRIMARY KEY (node_id, blob_hash),
    FOREIGN KEY (node_id) REFERENCES mesh_nodes(id)
);

-- Index for finding eviction candidates
CREATE INDEX idx_eviction_candidates
ON node_blob_storage(node_id, priority ASC, last_accessed ASC, pin_count);
```

## Replication Strategy

### 1. Placement Decision Algorithm

When a file is uploaded, decide which nodes should store it:

```rust
pub struct PlacementStrategy {
    replication_factor: usize,    // Target number of copies
    diversity_weight: f32,        // Prefer nodes on different networks
    capacity_weight: f32,         // Prefer nodes with more free space
    reliability_weight: f32,      // Prefer nodes with high uptime
}

impl PlacementStrategy {
    /// Select N nodes to store this blob
    pub async fn select_nodes(
        &self,
        blob_hash: &Hash,
        blob_size: u64,
        owner_node_id: &str,
        available_nodes: &[NodeStorageQuota],
    ) -> Result<Vec<String>> {
        let mut candidates = available_nodes
            .iter()
            .filter(|n| n.can_accept(blob_size))
            .collect::<Vec<_>>();

        // 1. ALWAYS include owner's node (if space available)
        let mut selected = Vec::new();
        if let Some(owner) = candidates.iter().find(|n| n.node_id == owner_node_id) {
            if owner.has_priority_space(blob_size) {
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
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        // 4. Select top N-1 nodes (already have owner)
        let needed = self.replication_factor.saturating_sub(selected.len());
        for (_, node) in scored.into_iter().take(needed) {
            selected.push(node.node_id.clone());
        }

        // 5. Ensure minimum replication
        if selected.len() < 2 {
            tracing::warn!(
                "Only {} replicas for blob {}, target is {}",
                selected.len(),
                blob_hash,
                self.replication_factor
            );
        }

        Ok(selected)
    }

    /// Score a node for suitability (higher = better)
    fn score_node(&self, node: &NodeStorageQuota, owner_id: &str) -> f32 {
        let mut score = 0.0;

        // 1. Capacity score: favor nodes with more free space
        let capacity_ratio = node.available_bytes as f32 / node.max_bytes as f32;
        score += capacity_ratio * self.capacity_weight;

        // 2. Diversity score: different network segment than owner
        // (would query network topology here)
        let is_different_network = node.node_id != owner_id; // Simplified
        if is_different_network {
            score += self.diversity_weight;
        }

        // 3. Reliability score: uptime percentage
        // (would query from mesh_nodes.last_seen, uptime_percentage)
        let reliability = 0.95; // Placeholder
        score += reliability * self.reliability_weight;

        score
    }
}
```

### 2. Replication Coordinator

Manages the actual replication process:

```rust
pub struct ReplicationCoordinator {
    strategy: PlacementStrategy,
    db: DbPool,
}

impl ReplicationCoordinator {
    /// Replicate a blob to target nodes
    pub async fn replicate(
        &self,
        blob_hash: &Hash,
        blob_size: u64,
        owner_node_id: &str,
        priority: u8,
    ) -> Result<ReplicationReport> {
        // 1. Get available nodes and their quotas
        let nodes = self.get_node_quotas().await?;

        // 2. Select target nodes
        let targets = self.strategy.select_nodes(
            blob_hash,
            blob_size,
            owner_node_id,
            &nodes,
        ).await?;

        // 3. Send replication requests to each target
        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for target_id in targets {
            match self.request_replication(&target_id, blob_hash, priority).await {
                Ok(_) => {
                    successes.push(target_id.clone());
                    self.update_quota_usage(&target_id, blob_size, 1).await?;
                }
                Err(e) => {
                    failures.push((target_id, e.to_string()));
                }
            }
        }

        Ok(ReplicationReport {
            blob_hash: blob_hash.to_string(),
            target_replicas: self.strategy.replication_factor,
            successful_replicas: successes.len(),
            failed_replicas: failures.len(),
            nodes: successes,
        })
    }

    /// Request a node to store a blob
    async fn request_replication(
        &self,
        target_node_id: &str,
        blob_hash: &Hash,
        priority: u8,
    ) -> Result<()> {
        // Send mesh message: "please fetch and store blob {hash}"
        // Target node will:
        // 1. Check if it has quota
        // 2. Fetch blob from any node that has it (gossip to find sources)
        // 3. Verify hash
        // 4. Store locally
        // 5. Ack success

        tracing::info!(
            "Requesting {} to store blob {}",
            target_node_id,
            blob_hash
        );

        // TODO: Actual mesh RPC call
        Ok(())
    }

    /// Update node's used quota after successful replication
    async fn update_quota_usage(
        &self,
        node_id: &str,
        blob_size: u64,
        delta: i64, // +1 for add, -1 for remove
    ) -> Result<()> {
        let conn = self.db.get()?;
        conn.execute(
            "UPDATE node_storage_quotas
             SET used_bytes = used_bytes + (?1 * ?2),
                 last_updated = datetime('now')
             WHERE node_id = ?3",
            params![delta, blob_size as i64, node_id],
        )?;
        Ok(())
    }
}
```

## Eviction Strategy

When a node runs out of space, it needs to evict chunks:

### Eviction Candidates

```rust
pub enum EvictionStrategy {
    LRU,           // Least Recently Used
    Priority,      // Lowest priority first
    Redundant,     // Only evict if other copies exist
    Hybrid,        // Combination of above
}

pub struct EvictionManager {
    db: DbPool,
    strategy: EvictionStrategy,
}

impl EvictionManager {
    /// Find chunks to evict to free up space
    pub async fn find_eviction_candidates(
        &self,
        node_id: &str,
        bytes_needed: u64,
    ) -> Result<Vec<EvictionCandidate>> {
        let conn = self.db.get()?;

        let query = match self.strategy {
            EvictionStrategy::LRU => {
                // Evict least recently accessed, never pinned
                "SELECT blob_hash, size_bytes, priority, last_accessed
                 FROM node_blob_storage
                 WHERE node_id = ?1 AND pin_count = 0
                 ORDER BY last_accessed ASC, priority ASC"
            }
            EvictionStrategy::Priority => {
                // Evict lowest priority first
                "SELECT blob_hash, size_bytes, priority, last_accessed
                 FROM node_blob_storage
                 WHERE node_id = ?1 AND pin_count = 0
                 ORDER BY priority ASC, last_accessed ASC"
            }
            EvictionStrategy::Redundant => {
                // Only evict if redundant copies exist elsewhere
                "SELECT nbs.blob_hash, nbs.size_bytes, nbs.priority, nbs.last_accessed
                 FROM node_blob_storage nbs
                 WHERE nbs.node_id = ?1 AND nbs.pin_count = 0
                   AND (SELECT COUNT(*) FROM blob_locations bl
                        WHERE bl.blob_hash = nbs.blob_hash) > 2
                 ORDER BY nbs.priority ASC, nbs.last_accessed ASC"
            }
            EvictionStrategy::Hybrid => {
                // Weighted score: priority + redundancy + age
                "SELECT blob_hash, size_bytes, priority, last_accessed,
                        (priority * 0.5 +
                         (SELECT COUNT(*) FROM blob_locations bl
                          WHERE bl.blob_hash = node_blob_storage.blob_hash) * 0.3 +
                         (julianday('now') - julianday(last_accessed)) * 0.2) as score
                 FROM node_blob_storage
                 WHERE node_id = ?1 AND pin_count = 0
                 ORDER BY score ASC"
            }
        };

        let mut stmt = conn.prepare(query)?;
        let mut candidates = Vec::new();
        let mut freed = 0u64;

        let rows = stmt.query_map(params![node_id], |row| {
            Ok(EvictionCandidate {
                blob_hash: row.get(0)?,
                size_bytes: row.get::<_, i64>(1)? as u64,
                priority: row.get(2)?,
                last_accessed: row.get(3)?,
            })
        })?;

        // Collect until we have enough space
        for candidate in rows {
            let candidate = candidate?;
            freed += candidate.size_bytes;
            candidates.push(candidate);

            if freed >= bytes_needed {
                break;
            }
        }

        Ok(candidates)
    }

    /// Evict chunks from this node
    pub async fn evict(
        &self,
        node_id: &str,
        candidates: Vec<EvictionCandidate>,
    ) -> Result<u64> {
        let mut freed = 0u64;

        for candidate in candidates {
            // 1. Delete blob from Iroh storage
            // iroh.delete_blob(&candidate.blob_hash).await?;

            // 2. Remove from database
            let conn = self.db.get()?;
            conn.execute(
                "DELETE FROM node_blob_storage
                 WHERE node_id = ?1 AND blob_hash = ?2",
                params![node_id, &candidate.blob_hash],
            )?;

            // 3. Update quota
            conn.execute(
                "UPDATE node_storage_quotas
                 SET used_bytes = used_bytes - ?1
                 WHERE node_id = ?2",
                params![candidate.size_bytes as i64, node_id],
            )?;

            freed += candidate.size_bytes;

            tracing::info!(
                "Evicted blob {} from {} (freed {} bytes)",
                candidate.blob_hash,
                node_id,
                candidate.size_bytes
            );
        }

        Ok(freed)
    }
}

pub struct EvictionCandidate {
    pub blob_hash: String,
    pub size_bytes: u64,
    pub priority: u8,
    pub last_accessed: String,
}
```

## Priority System

Different files get different treatment:

```rust
#[derive(Debug, Clone, Copy)]
pub enum FilePriority {
    Critical = 10,      // System files, always keep 5+ copies
    High = 8,           // User's personal files, 3+ copies
    Normal = 5,         // Shared files, 2+ copies
    Low = 3,            // Cached content, 1+ copy ok
    Expendable = 1,     // Can delete anytime
}

impl FilePriority {
    pub fn min_replicas(&self) -> usize {
        match self {
            FilePriority::Critical => 5,
            FilePriority::High => 3,
            FilePriority::Normal => 2,
            FilePriority::Low => 1,
            FilePriority::Expendable => 1,
        }
    }

    pub fn owner_must_have(&self) -> bool {
        match self {
            FilePriority::Critical | FilePriority::High => true,
            _ => false,
        }
    }
}
```

## Rebalancing

Periodically rebalance storage across mesh:

```rust
pub struct RebalancingTask {
    coordinator: ReplicationCoordinator,
    interval: Duration,
}

impl RebalancingTask {
    /// Run periodic rebalancing
    pub async fn run(&self) {
        loop {
            tokio::time::sleep(self.interval).await;

            if let Err(e) = self.rebalance().await {
                tracing::error!("Rebalancing failed: {}", e);
            }
        }
    }

    async fn rebalance(&self) -> Result<()> {
        // 1. Find under-replicated blobs
        let under_replicated = self.find_under_replicated().await?;

        for (blob_hash, current_count, target_count) in under_replicated {
            tracing::info!(
                "Blob {} has {} copies, need {}",
                blob_hash,
                current_count,
                target_count
            );

            // Trigger replication to reach target
            // (similar to initial replication)
        }

        // 2. Find over-utilized nodes
        let overloaded = self.find_overloaded_nodes().await?;

        for node_id in overloaded {
            tracing::info!("Node {} is overloaded, triggering eviction", node_id);
            // Evict some chunks
        }

        // 3. Find imbalanced distribution
        // Migrate chunks from full nodes to empty nodes

        Ok(())
    }
}
```

## User Controls

Users can influence replication:

```rust
// Pin a file to always keep on this device
pub async fn pin_file(file_id: &str, node_id: &str) -> Result<()> {
    // Increment pin_count, prevents eviction
}

// Set file priority
pub async fn set_priority(file_id: &str, priority: FilePriority) -> Result<()> {
    // Update priority in node_blob_storage
}

// Request more replicas
pub async fn increase_replicas(file_id: &str, count: usize) -> Result<()> {
    // Trigger additional replication
}
```

## Example Scenarios

### Scenario 1: Upload a 500MB video

1. User uploads on **Node A** (owner)
2. **PlacementStrategy** selects nodes:
   - Node A (owner, priority space) ✓
   - Node B (most free space, different network) ✓
   - Node C (high uptime, diverse location) ✓
3. Iroh stores blob on Node A
4. Replication requests sent to B and C
5. B and C fetch from A in parallel (QUIC)
6. All nodes update `node_blob_storage` tracking
7. User can access from any device

### Scenario 2: Node C runs out of space

1. Node C receives new blob request
2. Checks quota: 98GB used / 100GB max
3. Needs 5GB → eviction required
4. **EvictionManager** finds candidates:
   - Old cached thumbnails (priority=1, last accessed 30d ago)
   - Shared music files (priority=3, redundant on 4 other nodes)
5. Evicts 6GB of low-priority content
6. Accepts new blob
7. Updates quota tracking

### Scenario 3: Node B goes offline

1. **RebalancingTask** detects Node B offline > 24h
2. Checks blob replication counts
3. Finds 15 blobs now under-replicated (had 3 copies, now 2)
4. Selects new target nodes (Node D, Node E)
5. Triggers re-replication from remaining copies
6. Mesh self-heals

---

## Configuration Example

```toml
[storage]
enabled = true
max_storage_gb = 500
reserved_gb = 50
share_excess = true
min_free_space_gb = 10

[storage.replication]
default_factor = 3              # Most files get 3 copies
high_priority_factor = 5        # Important files get 5 copies
min_factor = 2                  # Never go below 2 copies

[storage.placement]
strategy = "hybrid"
capacity_weight = 0.4           # 40% weight to free space
diversity_weight = 0.3          # 30% weight to network diversity
reliability_weight = 0.3        # 30% weight to uptime

[storage.eviction]
strategy = "hybrid"             # lru | priority | redundant | hybrid
auto_evict = true
rebalance_interval_hours = 24
```

## Summary

**Quota management**: Each node declares limits, tracks usage, enforces caps
**Placement strategy**: Weighted scoring (capacity + diversity + reliability)
**Replication**: Automatic, configurable redundancy targets
**Eviction**: Smart cleanup when space runs out
**Priority system**: Critical files get preferential treatment
**Rebalancing**: Periodic health checks and redistribution
**User control**: Pin files, adjust priorities, view replica counts

This ensures fair, resilient, and efficient storage distribution across your mesh!
