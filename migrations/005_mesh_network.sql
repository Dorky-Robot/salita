-- Mesh Network Tables
-- Stores information about nodes in the personal mesh and their connections

-- Table: mesh_nodes
-- Stores information about each node in the mesh
CREATE TABLE IF NOT EXISTS mesh_nodes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    hostname TEXT NOT NULL,
    port INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'offline' CHECK(status IN ('online', 'offline', 'degraded')),
    capabilities TEXT NOT NULL DEFAULT '[]', -- JSON array of capability strings
    last_seen TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    metadata TEXT, -- Optional JSON metadata
    is_current INTEGER NOT NULL DEFAULT 0 CHECK(is_current IN (0, 1)) -- 1 if this is the current node
);

-- Index for quick lookups
CREATE INDEX IF NOT EXISTS idx_mesh_nodes_status ON mesh_nodes(status);
CREATE INDEX IF NOT EXISTS idx_mesh_nodes_last_seen ON mesh_nodes(last_seen);
CREATE INDEX IF NOT EXISTS idx_mesh_nodes_is_current ON mesh_nodes(is_current);

-- Table: node_connections
-- Stores connections between nodes in the mesh
CREATE TABLE IF NOT EXISTS node_connections (
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    connection_type TEXT NOT NULL DEFAULT 'unknown' CHECK(connection_type IN ('webrtc', 'http', 'unknown')),
    status TEXT NOT NULL DEFAULT 'disconnected' CHECK(status IN ('active', 'idle', 'disconnected')),
    last_ping TEXT, -- Last successful ping timestamp
    latency_ms INTEGER, -- Round-trip time in milliseconds
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (from_node_id, to_node_id),
    FOREIGN KEY (from_node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (to_node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE
);

-- Indexes for connection queries
CREATE INDEX IF NOT EXISTS idx_node_connections_from ON node_connections(from_node_id);
CREATE INDEX IF NOT EXISTS idx_node_connections_to ON node_connections(to_node_id);
CREATE INDEX IF NOT EXISTS idx_node_connections_status ON node_connections(status);

-- Initialize current node
-- This represents the Salita instance running this database
INSERT OR IGNORE INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, is_current)
VALUES (
    hex(randomblob(16)), -- Generate a random UUID-like ID
    'Salita Node',
    'localhost',
    6969,
    'online',
    '[]',
    datetime('now'),
    1
);
