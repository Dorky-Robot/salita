-- Recreate passkey_credentials with JSON storage for webauthn-rs Passkey type.
-- Safe to drop since no production data exists yet.
DROP TABLE IF EXISTS passkey_credentials;

CREATE TABLE passkey_credentials (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    passkey_json TEXT NOT NULL,
    name TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_passkey_credentials_user ON passkey_credentials(user_id);
