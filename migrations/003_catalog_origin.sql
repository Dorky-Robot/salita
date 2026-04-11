ALTER TABLE content_index ADD COLUMN origin_node TEXT;
ALTER TABLE content_index ADD COLUMN origin_iroh_node TEXT;
ALTER TABLE content_index ADD COLUMN is_local INTEGER NOT NULL DEFAULT 1;
CREATE INDEX idx_content_index_origin ON content_index(origin_node);
