# Salita Kanban

See [Design Principles](docs/DESIGN_PRINCIPLES.md) for schema and architecture guidance.

## Done

- [x] Project scaffolding (cargo, config, build.rs, Tailwind)
- [x] SQLite with r2d2 pool + migration runner
- [x] Static asset serving (rust-embed)
- [x] Welcome / setup page
- [x] Passkey registration + login (WebAuthn)
- [x] Session management (cookie-based)
- [x] CurrentUser / MaybeUser extractors
- [x] Stream page (posts list, HTMX)
- [x] Create / delete posts
- [x] Reactions (like/heart toggle)
- [x] Comments (create, list per post)
- [x] Relative timestamps
- [x] E2E tests (welcome + stream flows)
- [x] Design principles doc (DCI, data vs context, trait views)

## In Progress

- [ ] Commit Phase 3 (stream) working tree changes

## To Do — Phase 4: Social Media

DB tables already exist (`social_accounts`, `social_posts`, `cross_posts`).

**Schema / Design:**
- [ ] Create `streamable` trait view (unified feed across posts + social_posts)
- [ ] Social post card component (author avatar, handle, link to original)

**Bluesky:**
- [ ] Bluesky integration (OAuth / app password, fetch timeline)
- [ ] Bluesky cross-posting

**Mastodon:**
- [ ] Mastodon integration (OAuth, instance URL support)
- [ ] Mastodon cross-posting

**Shared:**
- [ ] Unified feed view — query `streamable` view, render mixed timeline
- [ ] Token refresh / re-auth flow
- [ ] Account management UI (connect, disconnect, status)

## To Do — Phase 5: Services Dashboard

DB table already exists (`services`).

- [ ] Service CRUD (add, edit, remove, reorder)
- [ ] Health check poller (background task, configurable interval)
- [ ] Dashboard page — grid of service tiles with status indicators
- [ ] Service detail / link-out

## To Do — Phase 6: Polish

- [ ] Image uploads for posts (store in `~/.salita/uploads/`)
- [ ] Dark mode / theme toggle
- [ ] Mobile-responsive layout pass
- [ ] Error pages (404, 500)
- [ ] Rate limiting
- [ ] HTTPS / TLS support (or reverse proxy docs)
- [ ] Backup / export (SQLite dump + uploads tar)

## Backlog / Ideas

**Content:**
- [ ] RSS feed for local stream
- [ ] Markdown support in posts
- [ ] Post editing
- [ ] Media gallery page
- [ ] Search (full-text across posts + comments) — candidate for `searchable` trait view

**Social:**
- [ ] ActivityPub federation
- [ ] Notifications (new comments on your posts)

**Infrastructure:**
- [ ] Multi-user support (invite links) — if so, extract `is_admin` to `user_roles`
- [ ] API tokens for external tools
- [ ] Webhook support (post events -> external URL)
