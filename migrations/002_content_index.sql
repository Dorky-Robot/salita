CREATE TABLE content_index (
    cid         TEXT PRIMARY KEY,
    dir         TEXT NOT NULL,
    path        TEXT NOT NULL,
    filename    TEXT NOT NULL,
    size        INTEGER NOT NULL,
    mime        TEXT,
    file_type   TEXT NOT NULL DEFAULT 'other',
    modified    TEXT,
    indexed_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX idx_content_index_dir_path ON content_index(dir, path);
CREATE INDEX idx_content_index_file_type ON content_index(file_type);
CREATE INDEX idx_content_index_modified ON content_index(modified);

CREATE TABLE content_thumbnails (
    cid         TEXT PRIMARY KEY REFERENCES content_index(cid) ON DELETE CASCADE,
    thumbnail   BLOB NOT NULL,
    width       INTEGER NOT NULL,
    height      INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
