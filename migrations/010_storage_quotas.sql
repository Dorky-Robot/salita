-- Storage quota and eviction management

-- Track each node's storage quota and usage
CREATE TABLE IF NOT EXISTS node_storage_quotas (
    node_id TEXT PRIMARY KEY,
    max_bytes INTEGER NOT NULL,           -- Maximum storage this node will dedicate
    used_bytes INTEGER NOT NULL DEFAULT 0,-- Currently used storage
    reserved_bytes INTEGER NOT NULL,      -- Reserved space for owner's files
    share_excess BOOLEAN DEFAULT 1,       -- Willing to store other nodes' files
    last_updated TEXT NOT NULL,
    FOREIGN KEY (node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE
);

-- Track which blobs are stored on which nodes (with eviction metadata)
CREATE TABLE IF NOT EXISTS node_blob_storage (
    node_id TEXT NOT NULL,
    blob_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    stored_at TEXT NOT NULL,              -- When this blob was stored
    priority INTEGER DEFAULT 5,           -- 0-10, higher = more important
    last_accessed TEXT,                   -- Last time this blob was accessed
    pin_count INTEGER DEFAULT 0,          -- Number of pins (prevent eviction)
    PRIMARY KEY (node_id, blob_hash),
    FOREIGN KEY (node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE
);

-- Index for finding eviction candidates
CREATE INDEX IF NOT EXISTS idx_eviction_lru
ON node_blob_storage(node_id, last_accessed ASC, priority ASC)
WHERE pin_count = 0;

-- Index for finding redundant blobs
CREATE INDEX IF NOT EXISTS idx_blob_redundancy
ON blob_locations(blob_hash);

-- Index for priority-based eviction
CREATE INDEX IF NOT EXISTS idx_eviction_priority
ON node_blob_storage(node_id, priority ASC, last_accessed ASC)
WHERE pin_count = 0;
