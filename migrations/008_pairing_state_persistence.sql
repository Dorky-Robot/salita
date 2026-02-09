-- Add persistent pairing state storage
-- Replaces in-memory join_tokens HashMap with durable state

CREATE TABLE IF NOT EXISTS pairing_states (
    token TEXT PRIMARY KEY,
    state_json TEXT NOT NULL,  -- Serialized PairingState enum
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_pairing_states_updated ON pairing_states(updated_at);
CREATE INDEX IF NOT EXISTS idx_pairing_states_created ON pairing_states(created_at);

-- Link sessions explicitly to devices
-- This provides clear device-session binding
CREATE TABLE IF NOT EXISTS device_sessions (
    session_token TEXT PRIMARY KEY,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_device_sessions_node ON device_sessions(node_id);
CREATE INDEX IF NOT EXISTS idx_device_sessions_expires ON device_sessions(expires_at);

-- Event log for pairing audit trail (optional but useful)
CREATE TABLE IF NOT EXISTS pairing_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    token TEXT NOT NULL,
    event_type TEXT NOT NULL,  -- 'created', 'connected', 'pin_verified', 'registered', 'failed'
    event_data TEXT,  -- JSON with details
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_pairing_events_token ON pairing_events(token);
CREATE INDEX IF NOT EXISTS idx_pairing_events_created ON pairing_events(created_at);
