CREATE TABLE content_previews (
    cid         TEXT PRIMARY KEY REFERENCES content_index(cid) ON DELETE CASCADE,
    preview     BLOB NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    format      TEXT NOT NULL DEFAULT 'image/jpeg',
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
