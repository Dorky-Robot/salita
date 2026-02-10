# Storage Quota Management - Practical Examples

## Overview

This document shows real-world scenarios of how storage quotas, chunk placement, and eviction work in practice.

## Example Mesh Setup

```
Mesh with 4 devices:

┌─────────────────────────────────────────────────────┐
│ Desktop (Node A) - Owner's main computer            │
│ - 500 GB max storage                                │
│ - 50 GB reserved for owner's files                  │
│ - Share excess: YES                                 │
│ - Currently used: 200 GB                            │
│ - Available: 300 GB                                 │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Laptop (Node B) - Owner's portable device           │
│ - 100 GB max storage                                │
│ - 20 GB reserved                                    │
│ - Share excess: YES                                 │
│ - Currently used: 60 GB                             │
│ - Available: 40 GB                                  │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ NAS (Node C) - Dedicated storage server             │
│ - 2 TB max storage                                  │
│ - 100 GB reserved                                   │
│ - Share excess: YES                                 │
│ - Currently used: 500 GB                            │
│ - Available: 1500 GB                                │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Raspberry Pi (Node D) - Low-power always-on device  │
│ - 32 GB max storage                                 │
│ - 5 GB reserved                                     │
│ - Share excess: YES                                 │
│ - Currently used: 28 GB                             │
│ - Available: 4 GB                                   │
└─────────────────────────────────────────────────────┘
```

## Scenario 1: Uploading a 10 GB Video

**User uploads vacation.mp4 from Desktop (Node A)**

### Step 1: Placement Decision

```rust
// Configuration: replication_factor = 3, min_free_space = 5GB

PlacementStrategy::select_nodes(
    blob_hash: "abc123...",
    blob_size: 10 GB,
    owner_node_id: "Node A",
    available_nodes: [A, B, C, D],
    min_free_space: 5 GB
)
```

**Node Evaluation:**

| Node | Available | Can Fit 10GB + 5GB? | Score Calculation | Selected? |
|------|-----------|-------------------|-------------------|-----------|
| A (Desktop) | 300 GB | ✅ YES | Owner priority | ✅ **1st** |
| B (Laptop) | 40 GB | ✅ YES | `0.4 * (40/100) + 0.3 * 1.0 + 0.3 * 0.95 = 0.61` | ✅ **3rd** |
| C (NAS) | 1500 GB | ✅ YES | `0.4 * (1500/2000) + 0.3 * 1.0 + 0.3 * 0.95 = 0.89` | ✅ **2nd** |
| D (Pi) | 4 GB | ❌ NO | Not enough space | ❌ |

**Result:** Blob stored on **A, C, B** (3 copies)

### Step 2: Storage Updates

```sql
-- Update quotas
UPDATE node_storage_quotas
SET used_bytes = used_bytes + 10737418240  -- 10 GB
WHERE node_id IN ('A', 'B', 'C');

-- Track blob storage
INSERT INTO node_blob_storage (node_id, blob_hash, size_bytes, priority, ...)
VALUES
  ('A', 'abc123...', 10737418240, 8, ...),  -- High priority (owner)
  ('B', 'abc123...', 10737418240, 5, ...),  -- Normal priority
  ('C', 'abc123...', 10737418240, 5, ...);  -- Normal priority

-- Track locations
INSERT INTO blob_locations (blob_hash, node_id, ...)
VALUES
  ('abc123...', 'A', ...),
  ('abc123...', 'B', ...),
  ('abc123...', 'C', ...);
```

**New State:**

- Node A: 200 GB → 210 GB used (still 290 GB available)
- Node B: 60 GB → 70 GB used (still 30 GB available)
- Node C: 500 GB → 510 GB used (still 1490 GB available)
- Node D: Unchanged

## Scenario 2: Laptop Runs Out of Space

**User tries to upload a 50 GB file from Laptop (Node B)**

### Step 1: Check Quota

```rust
Node B status:
- Available: 30 GB
- Needed: 50 GB + 5 GB min_free = 55 GB
- Shortfall: 25 GB

Eviction required!
```

### Step 2: Find Eviction Candidates

Using **Hybrid** strategy:

```sql
SELECT blob_hash, size_bytes, priority, last_accessed, pin_count
FROM node_blob_storage
WHERE node_id = 'B'
  AND pin_count = 0  -- Not pinned
ORDER BY
  (priority * 2 +
   (SELECT COUNT(*) FROM blob_locations WHERE blob_hash = node_blob_storage.blob_hash) * -5 +
   (julianday('now') - julianday(last_accessed)) * -0.1
  ) ASC
```

**Candidates Found:**

| Blob | Size | Priority | Redundant? | Age (days) | Evict? |
|------|------|----------|------------|------------|--------|
| old_photos.zip | 15 GB | 3 | Yes (4 copies) | 60 | ✅ |
| cached_videos.mp4 | 8 GB | 2 | Yes (3 copies) | 45 | ✅ |
| documents.tar | 5 GB | 5 | Yes (3 copies) | 10 | ✅ |

**Total to evict:** 28 GB (enough to free 25 GB needed)

### Step 3: Perform Eviction

```rust
EvictionManager::evict(node_id: "B", candidates: [
    old_photos.zip,
    cached_videos.mp4,
    documents.tar
])
```

**Result:**
- Node B: 70 GB → 42 GB used
- Available: 30 GB → 58 GB (can now fit 50 GB file!)
- Evicted blobs still exist on other nodes (A, C, D)

### Step 4: Upload Proceeds

New 50 GB file replicates to:
- Node B (owner): Priority storage
- Node C (NAS): Most available space
- Node A (Desktop): Good reliability

## Scenario 3: Raspberry Pi Storage Management

**Pi (Node D) has only 4 GB free, replication coordinator tries to store a 3 GB chunk**

### Option A: Auto-Evict Enabled

```toml
[storage.eviction]
auto_evict = true
strategy = "redundant"  # Only evict if copies exist elsewhere
```

**Process:**

1. Check quota: 4 GB available, need 3 GB + 5 GB min_free = ❌ Not enough
2. Find redundant blobs:

```sql
SELECT * FROM node_blob_storage
WHERE node_id = 'D'
  AND pin_count = 0
  AND (SELECT COUNT(*) FROM blob_locations WHERE blob_hash = node_blob_storage.blob_hash) >= 2
```

3. Evict **6 GB of low-priority, redundant content**
4. Accept 3 GB chunk

### Option B: Auto-Evict Disabled

```toml
[storage.eviction]
auto_evict = false
```

**Result:** Node D **rejects** the chunk, replication coordinator selects different node.

## Scenario 4: Priority Files

**User marks critical work files as high priority**

```rust
// Set priority to 10 (critical)
update_blob_priority(blob_hash: "work_docs.zip", priority: 10);

// Pin to prevent eviction
pin_blob(node_id: "A", blob_hash: "work_docs.zip");
```

**Effect:**

| Priority Level | Behavior |
|----------------|----------|
| 10 (Critical) | - Never evicted if pinned<br>- Replicated to 5+ nodes<br>- Owner device guaranteed copy |
| 8 (High) | - Owner device guaranteed copy<br>- 3+ replicas |
| 5 (Normal) | - 2+ replicas<br>- Evictable if space needed |
| 3 (Low) | - 1-2 replicas<br>- First to evict |
| 1 (Expendable) | - 1 replica<br>- Aggressive eviction |

## Scenario 5: Node Goes Offline

**Desktop (Node A) goes offline for maintenance**

### Rebalancing Task Detects Issue

```rust
// Run every 24 hours
RebalancingTask::run() {
    // Find blobs that are under-replicated
    let under_replicated = find_blobs_below_target_replicas();

    // Example: vacation.mp4 had 3 copies (A, B, C), now only 2 (B, C)
    // Target: 3 copies
    // Need: 1 more copy
}
```

**Action:**

1. Select new node for replication: **Node D** (Pi)
2. Check if Pi has space: 4 GB available, blob is 10 GB → ❌ No
3. Try next candidate: **No other nodes available**
4. **Decision:** Wait for Node A to return (acceptable risk with 2 copies)

**If critical file:**
- Would evict low-priority content from Node D
- Or alert user: "Critical file under-replicated"

## Scenario 6: Adding a New Device

**User adds new NAS (Node E) with 4 TB storage**

### Initialization

```rust
set_node_quota(
    node_id: "E",
    max_bytes: 4 * 1024 * 1024 * 1024 * 1024,  // 4 TB
    reserved_bytes: 100 * 1024 * 1024 * 1024,  // 100 GB
    share_excess: true
);
```

### Rebalancing Kicks In

```rust
// Distribute load to new node
RebalancingTask::migrate_to_underutilized_nodes() {
    // Node E: 0% full
    // Node C: 25% full
    // Node A: 42% full
    // Node B: 70% full

    // Move some blobs from B → E to balance load
}
```

**Result:** Mesh automatically spreads files to new capacity!

## Configuration Reference

```toml
[storage]
enabled = true
max_storage_gb = 500
reserved_gb = 50
share_excess = true
min_free_space_gb = 5

[storage.replication]
default_factor = 3
high_priority_factor = 5
min_factor = 2

[storage.placement]
strategy = "hybrid"
capacity_weight = 0.4      # Prefer nodes with free space
diversity_weight = 0.3     # Prefer diverse nodes
reliability_weight = 0.3   # Prefer reliable nodes

[storage.eviction]
strategy = "hybrid"        # lru | priority | redundant | hybrid
auto_evict = true
min_free_space_gb = 5

[storage.rebalancing]
enabled = true
interval_hours = 24
target_utilization = 0.7   # Rebalance if node > 70% full
```

## Summary

**Quotas** → Each device declares limits
**Placement** → Smart selection based on capacity, diversity, reliability
**Eviction** → Automatic cleanup when full (respects pins, priorities, redundancy)
**Rebalancing** → Self-healing when nodes join/leave
**Priorities** → Important files get preferential treatment

Your mesh becomes a self-managing, fault-tolerant distributed storage system!
