# Salita Design Principles

Adapted from [Levee's design principles](../../nerdnest/levee/docs/DESIGN_PRINCIPLES.md) for a personal home server context.

---

## Core Philosophy

### Name Things What They Are

A post is a `post`, not a "content item with type=post".
A social account is a `social_account`, not a "connection with provider field".
A service is a `service`, not a "link with health_url".

Each concept keeps its natural language. If you'd explain it to someone using a specific word, that's the table name.

### No Generic Buckets

Don't collapse distinct concepts into one table with a `type` column.

**Anti-pattern:**
```sql
items (id, user_id, type, body, metadata_json, ...)
-- "type" = 'post' | 'social_post' | 'comment' | 'service'
-- Everything loses meaning, queries become WHERE type = '...' soup
```

**Pattern:**
```sql
posts (id, user_id, body, ...)
social_posts (id, account_id, body, platform_post_id, ...)
comments (id, post_id, user_id, body, ...)
services (id, name, url, health_status, ...)
```

Each table has columns that make sense for its concept. A social post has `platform_post_id` and `author_avatar_url`. A local post doesn't. They're different things.

---

## Data vs Context

Separate **facts about an entity** from the **role it plays**.

**Key question:** "Is this a fact about the entity, or a role it plays here?"

### Current Schema Assessment

**Users — mostly clean, one concern:**
```sql
users (id, username, display_name, bio, avatar_path, is_admin, ...)
```

`username`, `display_name`, `bio`, `avatar_path` are facts about the person.
`is_admin` is a role — it describes what the person can do in this instance, not who they are.

For a single-user home server this is fine. If multi-user grows, `is_admin` should move to a role context (e.g., `user_roles` or similar). No need to act on this now.

**Everything else is clean:**
- `posts`, `comments`, `reactions` — pure data about user actions
- `social_accounts` — data about a connection (platform, handle, tokens)
- `services` — data about an external service (name, url, health)

---

## Trait Views for Polymorphic Access

When different types need to appear in the same list (a timeline, a feed, a search result), use **views** to project them into a shared shape — don't flatten them into one table.

### The Streamable Trait

The unified feed (Phase 4) mixes local posts with social posts. Both are "things in the stream" but they're structurally different.

```sql
CREATE VIEW streamable AS
  SELECT
    p.id,
    u.username AS author_name,
    u.display_name AS author_display_name,
    NULL AS author_avatar_url,
    p.body,
    p.created_at,
    'local' AS source,
    NULL AS source_url
  FROM posts p
  JOIN users u ON u.id = p.user_id

  UNION ALL

  SELECT
    sp.id,
    sp.author_handle AS author_name,
    sp.author_display_name,
    sp.author_avatar_url,
    sp.body,
    sp.posted_at AS created_at,
    sa.platform AS source,
    sp.post_url AS source_url
  FROM social_posts sp
  JOIN social_accounts sa ON sa.id = sp.account_id;
```

This gives:
- One query for the unified feed: `SELECT * FROM streamable ORDER BY created_at DESC`
- Local posts and social posts keep their own tables with their own columns
- The view defines the "contract" — what it means to be streamable

### Future Traits

As the app grows, other trait views may emerge:

| Trait | Purpose | Implements |
|-------|---------|------------|
| `streamable` | Unified timeline feed | `posts`, `social_posts` |
| `searchable` | Full-text search results | `posts`, `comments`, `services` |
| `exportable` | Backup/export items | `posts`, `comments`, `social_accounts` |

Don't create these until needed. The pattern is here when you need it.

---

## Policies

For a personal home server, policies are simpler than a multi-tenant app, but the principle still applies: **keep rules out of data models**.

### Current Policies

| Policy | Where | Rule |
|--------|-------|------|
| Post ownership | `stream.rs` handler | Only the author or admin can delete a post |
| Auth required | `CurrentUser` extractor | Rejects unauthenticated requests with 401 |
| Post length | `stream.rs` handler | Body <= 2000 chars, comment <= 500 chars |
| Reaction kinds | `stream.rs` handler | Only "like" and "heart" are valid |

These live in handlers/extractors (context), not in the data models. Good.

### As Complexity Grows

If cross-posting adds rules like "only cross-post to accounts you own" or services adds "only admin can add services", keep those in the handler/context layer, not baked into the DB schema or data structs.

---

## Schema Decision Tree

When adding a new concept:

1. **What would you call it in conversation?** → That's the table name
2. **Does it have unique columns that don't make sense on other types?** → It's its own table
3. **Does it need to appear alongside other types in a list?** → Add a trait view
4. **Is it a fact or a role?** → Facts go on the entity table, roles go in context
5. **Is it truly the same thing as an existing concept?** → Then extend that table (rare)

---

## Salita's Domain Map

```
DATA (facts about entities)
├── users              — people who use this instance
├── posts              — things a user wrote
├── comments           — responses to posts
├── reactions          — emoji reactions on posts
├── social_accounts    — connected external platforms
├── social_posts       — cached posts from external platforms
├── services           — external services to monitor
└── passkey_credentials — WebAuthn credentials

CONTEXT (roles and relationships)
├── sessions           — a user's active login
├── invite_tokens      — a user's ability to invite others
├── cross_posts        — a post's publication to an external platform
└── is_admin (on users) — admin role (migrate to user_roles if multi-user grows)

TRAITS (polymorphic views)
├── streamable         — unified feed (posts + social_posts)
└── (future: searchable, exportable)
```
