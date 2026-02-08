-- Token-Based Mesh Migration
-- Adds peer-to-peer authentication tokens and node identity tracking

-- Tokens for calling other devices (we store their tokens)
CREATE TABLE peer_tokens (
    peer_node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    permissions TEXT NOT NULL,  -- JSON array
    issued_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    last_used_at TEXT,
    PRIMARY KEY (peer_node_id)
);

-- Tokens issued to other devices to call us
CREATE TABLE issued_tokens (
    token TEXT PRIMARY KEY,
    issued_to_node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    permissions TEXT NOT NULL,
    issued_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    last_used_at TEXT,
    revoked_at TEXT
);

-- Device reachability cache
CREATE TABLE discovery_cache (
    node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    reachable BOOLEAN NOT NULL,
    last_ping_at TEXT NOT NULL,
    response_time_ms INTEGER,
    health_metadata TEXT
);

-- Formally mark current node (replaces is_current flag)
CREATE TABLE current_node (
    node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id)
);

-- Add metadata column to mesh_nodes if it doesn't exist
-- Note: This will be handled specially in Rust code since SQLite
-- doesn't support IF NOT EXISTS for ALTER TABLE ADD COLUMN

-- Drop old connections table (no longer needed)
DROP TABLE IF EXISTS node_connections;

-- Indexes for performance
CREATE INDEX idx_peer_tokens_expires ON peer_tokens(expires_at);
CREATE INDEX idx_issued_tokens_node ON issued_tokens(issued_to_node_id);
CREATE INDEX idx_issued_tokens_expires ON issued_tokens(expires_at);
CREATE INDEX idx_issued_tokens_revoked ON issued_tokens(revoked_at) WHERE revoked_at IS NULL;
