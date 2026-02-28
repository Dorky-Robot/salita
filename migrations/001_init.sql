-- Salita v2 schema — simplified for MCP mesh

-- Every device in the mesh (including self)
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    endpoint TEXT,
    port INTEGER NOT NULL DEFAULT 6969,
    is_self INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'offline',
    last_seen TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Which device is "self" on this machine
CREATE TABLE IF NOT EXISTS current_node (
    node_id TEXT PRIMARY KEY
);
