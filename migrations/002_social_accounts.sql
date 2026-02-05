-- Social media accounts
CREATE TABLE IF NOT EXISTS social_accounts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform TEXT NOT NULL,  -- 'bluesky', 'mastodon', 'twitter', 'instagram'
    handle TEXT NOT NULL,
    access_token_enc BLOB,   -- AES-256-GCM encrypted
    refresh_token_enc BLOB,  -- AES-256-GCM encrypted
    token_expires_at TEXT,
    instance_url TEXT,        -- For Mastodon
    extra_json TEXT,          -- Platform-specific data
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(user_id, platform, handle)
);

-- Cached social posts
CREATE TABLE IF NOT EXISTS social_posts (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES social_accounts(id) ON DELETE CASCADE,
    platform_post_id TEXT NOT NULL,
    author_handle TEXT NOT NULL,
    author_display_name TEXT,
    author_avatar_url TEXT,
    body TEXT,
    media_urls TEXT,  -- JSON array
    post_url TEXT,
    posted_at TEXT NOT NULL,
    fetched_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(account_id, platform_post_id)
);

CREATE INDEX IF NOT EXISTS idx_social_posts_posted ON social_posts(posted_at DESC);
CREATE INDEX IF NOT EXISTS idx_social_posts_account ON social_posts(account_id);

-- Cross-posts (local post -> social platform)
CREATE TABLE IF NOT EXISTS cross_posts (
    id TEXT PRIMARY KEY,
    post_id TEXT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES social_accounts(id) ON DELETE CASCADE,
    platform_post_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',  -- 'pending', 'posted', 'failed'
    error_message TEXT,
    posted_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
