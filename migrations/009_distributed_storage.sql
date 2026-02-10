-- Distributed Storage System (Iroh-based)

-- Files stored in distributed storage
CREATE TABLE IF NOT EXISTS files_dss (
    id TEXT PRIMARY KEY,              -- UUIDv7
    hash TEXT NOT NULL,                -- Iroh blob hash (BLAKE3)
    name TEXT NOT NULL,                -- Original filename
    mime_type TEXT,                    -- MIME type
    size INTEGER NOT NULL,             -- File size in bytes
    owner_id TEXT NOT NULL,            -- User who uploaded
    created_at TEXT NOT NULL,          -- Upload timestamp
    metadata TEXT,                     -- JSON: tags, description, etc.
    FOREIGN KEY (owner_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Index for fast user file lookups
CREATE INDEX IF NOT EXISTS idx_files_dss_owner ON files_dss(owner_id, created_at DESC);

-- Index for hash lookups (deduplication)
CREATE INDEX IF NOT EXISTS idx_files_dss_hash ON files_dss(hash);

-- Track which mesh nodes have which blobs (for replication management)
CREATE TABLE IF NOT EXISTS blob_locations (
    blob_hash TEXT NOT NULL,           -- Iroh blob hash
    node_id TEXT NOT NULL,             -- Mesh node ID
    added_at TEXT NOT NULL,            -- When this node received the blob
    last_verified TEXT NOT NULL,       -- Last verification timestamp
    PRIMARY KEY (blob_hash, node_id),
    FOREIGN KEY (node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE
);

-- Shared files (permissions and sharing links)
CREATE TABLE IF NOT EXISTS file_shares (
    id TEXT PRIMARY KEY,               -- UUIDv7
    file_id TEXT NOT NULL,             -- File being shared
    created_by TEXT NOT NULL,          -- User who created share
    share_type TEXT NOT NULL,          -- 'user', 'link', 'public'
    target_user_id TEXT,               -- For user-specific shares
    token TEXT,                        -- For link-based shares
    expires_at TEXT,                   -- Optional expiration
    created_at TEXT NOT NULL,
    FOREIGN KEY (file_id) REFERENCES files_dss(id) ON DELETE CASCADE,
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (target_user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- Index for finding shares by file
CREATE INDEX IF NOT EXISTS idx_file_shares_file ON file_shares(file_id);

-- Index for finding shares by token (link-based access)
CREATE INDEX IF NOT EXISTS idx_file_shares_token ON file_shares(token) WHERE token IS NOT NULL;
