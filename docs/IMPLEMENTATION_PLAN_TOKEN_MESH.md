# Implementation Plan: Token-Based Mesh

**Design Document**: [TOKEN_BASED_MESH.md](./TOKEN_BASED_MESH.md)

---

## Overview

Implement token-based peer authentication for the Salita mesh, replacing manual node registration with automatic token exchange during pairing.

**Core changes:**
1. ✅ Devices exchange tokens during pairing (completely invisible to user)
2. ✅ Tokens auto-expire (30 days) and auto-renew on use (silent)
3. ✅ On-demand connections only (no persistent mesh)
4. ✅ Capability-based discovery with reachability checks (automatic)

---

## UX Philosophy: It Just Works™

### What Users Experience

✅ **Pairing**: Scan QR → Enter PIN → Done
✅ **Using**: "Send to Laptop" → It happens
✅ **Managing**: See device list, tap "Remove" if needed

### What's Invisible

❌ Users NEVER see tokens
❌ Users NEVER manage permissions
❌ Users NEVER see technical errors
❌ Users NEVER configure anything

### Implementation Principle

**Every feature must answer**: "Would an Apple user understand this?"

- ✅ "My Devices" - Yes
- ✅ "Available" / "Offline" - Yes
- ✅ "Needs re-pairing" - Yes
- ❌ "Token expired" - No
- ❌ "Bearer authentication failed" - No
- ❌ "Manage API keys" - No

---

## Phase 1: Database Schema & Migrations

### Tasks

1. **Create migration file**: `migrations/006_token_based_mesh.sql`
   - Add new tables: `peer_tokens`, `issued_tokens`, `discovery_cache`, `current_node`
   - Alter `mesh_nodes`: Add `metadata` column
   - Drop old table: `node_connections` (no longer needed)
   - Add indexes for performance

2. **Update `db.rs`**: Run new migration on startup

3. **Test migration**: Verify schema changes apply cleanly

**Files to modify:**
- `src/db.rs` - Database initialization
- `migrations/` - New migration file

---

## Phase 2: Token Generation & Storage

### Tasks

1. **Create `src/mesh/tokens.rs`** module:
   ```rust
   pub fn generate_secure_token() -> String
   pub fn hash_token(token: &str) -> String  // For storage
   ```

2. **Create token models in `src/mesh/models.rs`**:
   ```rust
   pub struct PeerToken { ... }
   pub struct IssuedToken { ... }
   impl IssuedToken {
       pub fn is_expired(&self) -> bool
       pub fn needs_renewal(&self) -> bool
       pub fn renew(&mut self) -> Result<()>
   }
   ```

3. **Add token CRUD operations in `src/mesh/store.rs`**:
   ```rust
   pub fn store_peer_token(conn: &Connection, peer_id: &str, token: &str, ...) -> Result<()>
   pub fn get_peer_token(conn: &Connection, peer_id: &str) -> Result<String>
   pub fn verify_issued_token(conn: &Connection, token: &str) -> Result<IssuedToken>
   pub fn revoke_token(conn: &Connection, peer_id: &str) -> Result<()>
   ```

**Files to create/modify:**
- `src/mesh/tokens.rs` - New module
- `src/mesh/models.rs` - Token structs
- `src/mesh/store.rs` - Token storage

**Dependencies:**
- `ring` crate for secure random (already used for WebAuthn)

---

## Phase 3: Node Identity Persistence

### Tasks

1. **Create `src/mesh/node_identity.rs`**:
   ```rust
   pub struct NodeIdentity {
       pub id: String,
       pub name: String,
       pub created_at: DateTime<Utc>,
   }

   impl NodeIdentity {
       pub fn load_or_create(data_dir: &Path) -> Result<Self>
       pub fn save(&self, data_dir: &Path) -> Result<()>
   }

   fn default_node_name() -> String  // Use hostname or "Salita Node"
   ```

2. **Update `src/main.rs`**: Load node identity on startup
   ```rust
   let node_identity = NodeIdentity::load_or_create(&config.data_dir)?;
   ```

3. **Initialize current node in DB**:
   ```rust
   // Insert into mesh_nodes + current_node
   ```

**Files to create/modify:**
- `src/mesh/node_identity.rs` - New module
- `src/main.rs` - Initialize on startup
- `~/.salita/node_identity.json` - Created on first run

---

## Phase 4: Token Verification Middleware

### Tasks

1. **Create `src/auth/peer_auth.rs`**:
   ```rust
   pub struct PeerNode {
       pub node_id: String,
       pub permissions: Vec<String>,
   }

   pub async fn verify_peer_token(
       State(state): State<AppState>,
       req: Request<Body>,
       next: Next,
   ) -> Result<Response, StatusCode>
   ```
   - Extract bearer token from `Authorization` header
   - Look up in `issued_tokens`
   - Check expiration, revocation
   - Auto-renew if < 7 days left
   - Insert `PeerNode` into request extensions

2. **Create permission checker**:
   ```rust
   pub fn require_permission(permission: &str) -> impl Fn(Extension<PeerNode>) -> Result<(), StatusCode>
   ```

**Files to create:**
- `src/auth/peer_auth.rs` - New module
- Update `src/auth/mod.rs` - Export new types

---

## Phase 5: Update Pairing Flow (Token Exchange)

### Tasks

1. **Update `src/auth/handlers.rs::pair_verify()`**:
   - Accept `node_identity` in request body
   - Store peer node in `mesh_nodes` (upsert by ID)
   - Generate token for peer: `generate_secure_token()`
   - Store in `issued_tokens`
   - Return desktop's identity + token in response

2. **Update request/response types**:
   ```rust
   #[derive(Deserialize)]
   pub struct PairVerifyRequest {
       pub code: String,
       pub pin: String,
       pub linking_code: String,
       pub node_identity: NodeIdentityInfo,  // NEW
   }

   #[derive(Serialize)]
   pub struct PairVerifyResponse {
       pub ok: bool,
       pub session_token: String,
       pub peer_node: PeerNodeInfo,  // NEW
   }

   pub struct NodeIdentityInfo {
       pub node_id: String,
       pub name: String,
       pub hostname: String,
       pub port: u16,
       pub capabilities: Vec<String>,
   }

   pub struct PeerNodeInfo {
       pub node_id: String,
       pub name: String,
       pub hostname: String,
       pub port: u16,
       pub capabilities: Vec<String>,
       pub access_token: String,  // Token to call this node
       pub permissions: Vec<String>,
       pub expires_at: String,
   }
   ```

3. **Get current node identity**:
   ```rust
   fn get_current_node(db: &DbPool) -> Result<NodeIdentityInfo>
   ```

**Files to modify:**
- `src/auth/handlers.rs` - Update pair_verify
- `src/auth/types.rs` - New request/response types

---

## Phase 6: GraphQL API Extensions

### Tasks

1. **Update `src/graphql/types.rs`**:
   - Add `IssuedToken` type
   - Add `PeerToken` type
   - Add `DiscoveryResult` type

2. **Update `src/graphql/query.rs`**:
   ```rust
   async fn capable_nodes(&self, ctx: &Context<'_>, capability: String) -> Result<Vec<MeshNode>>
   async fn reachable_nodes(&self, ctx: &Context<'_>) -> Result<Vec<MeshNode>>
   async fn issued_tokens(&self, ctx: &Context<'_>) -> Result<Vec<IssuedToken>>
   async fn peer_tokens(&self, ctx: &Context<'_>) -> Result<Vec<PeerToken>>
   ```

3. **Update `src/graphql/mutation.rs`**:
   ```rust
   async fn revoke_token(&self, ctx: &Context<'_>, node_id: String) -> Result<bool>
   async fn remove_node(&self, ctx: &Context<'_>, id: String) -> Result<bool>
   async fn discover_nodes(&self, ctx: &Context<'_>) -> Result<DiscoveryResult>
   ```

**Files to modify:**
- `src/graphql/types.rs` - New types
- `src/graphql/query.rs` - New queries
- `src/graphql/mutation.rs` - New mutations

---

## Phase 7: REST Endpoints for Device Operations

**IMPORTANT**: Use "device" language, not "node"

### Tasks

1. **Create `src/routes/node.rs`**:
   ```rust
   GET  /api/node/identity          - Get current node
   POST /api/node/identity          - Update current node
   GET  /health                     - Health check (requires peer token)
   POST /api/discover               - Ping all peers, update reachability
   ```

2. **Create `src/routes/tokens.rs`**:
   ```rust
   GET  /api/tokens                 - List issued tokens
   POST /api/tokens/revoke/:node_id - Revoke token
   ```

3. **Register routes in `src/main.rs`**:
   ```rust
   .route("/api/node/identity", get(node::get_identity).post(node::update_identity))
   .route("/health", get(node::health_check).layer(middleware::from_fn_with_state(state.clone(), verify_peer_token)))
   .route("/api/tokens", get(tokens::list_issued))
   .route("/api/tokens/revoke/:node_id", post(tokens::revoke))
   ```

**Files to create/modify:**
- `src/routes/node.rs` - New module
- `src/routes/tokens.rs` - New module
- `src/main.rs` - Register routes
- `src/routes/mod.rs` - Export new modules

---

## Phase 8: Error Message Translation Layer

### Tasks

1. **Create `src/errors/user_friendly.rs`**:
   ```rust
   pub trait UserFriendlyError {
       fn to_user_message(&self) -> String;
       fn to_log_message(&self) -> String;  // Technical details for logs
   }

   impl UserFriendlyError for TokenError {
       fn to_user_message(&self) -> String {
           match self {
               TokenError::Expired => "Device needs re-pairing".to_string(),
               TokenError::Invalid => "Couldn't connect to device".to_string(),
               TokenError::Revoked => "Device access removed".to_string(),
               _ => "Something went wrong".to_string(),
           }
       }
   }
   ```

2. **Add friendly error responses**:
   ```rust
   // In API handlers
   .map_err(|e| {
       error!("Technical error: {:?}", e);  // Log technical details
       (
           StatusCode::BAD_REQUEST,
           Json(json!({ "error": e.to_user_message() }))  // Show friendly message
       )
   })
   ```

3. **Create error message dictionary**:
   ```rust
   pub const ERROR_MESSAGES: &[(&str, &str)] = &[
       // (Technical, User-Friendly)
       ("Token expired", "Device needs re-pairing"),
       ("Connection refused", "Device is offline"),
       ("Permission denied", "Device can't do that"),
       ("Token not found", "Device not recognized"),
       ("Invalid bearer token", "Couldn't connect to device"),
   ];
   ```

4. **Update frontend to show friendly errors**:
   ```javascript
   catch (error) {
       // Show user-friendly message
       toast.error(error.message);  // "Device needs re-pairing"

       // Log technical details to console (for debugging)
       console.error('Technical error:', error);
   }
   ```

**Files to create/modify:**
- `src/errors/user_friendly.rs` - New module
- All API handlers - Use friendly errors
- Frontend JS - Show friendly messages

**Principle**: User sees "Device needs re-pairing", logs show "401 Unauthorized: Token expired at 2026-02-08T10:00:00Z"

---

## Phase 9: Frontend Pairing Updates

**Focus**: Make pairing feel magical

### Tasks

1. **Update pairing JavaScript** (in templates or static files):
   - Load current node identity from local storage or generate
   - Include `node_identity` in `/auth/pair/verify` request
   - Store returned `peer_node` info in local database/storage
   - Store peer token in `peer_tokens` table

2. **Create client-side token storage** (if web app):
   - Use IndexedDB or localStorage
   - Store: `{ peer_node_id → token }`

**Files to modify:**
- `templates/pairing.html` (or wherever pairing UI lives)
- Client-side JS for pairing flow

---

## Phase 10: Discovery & Reachability

### Tasks

1. **Create `src/mesh/discovery.rs`**:
   ```rust
   pub async fn ping_node(hostname: &str, port: u16, token: &str) -> Result<PingResult>
   pub async fn discover_all(db: &DbPool) -> Result<DiscoveryResult>
   ```

2. **Implement health endpoint handler**:
   ```rust
   async fn health_check(
       Extension(peer): Extension<PeerNode>,
       State(state): State<AppState>,
   ) -> Json<HealthResponse> {
       Json(HealthResponse {
           ok: true,
           node_id: get_current_node_id(&state.db),
           capabilities: get_current_capabilities(&state.db),
           metadata: get_current_metadata(),
       })
   }
   ```

3. **Update discovery cache on ping**:
   ```rust
   INSERT OR REPLACE INTO discovery_cache (node_id, reachable, last_ping_at, response_time_ms)
   VALUES (?, ?, datetime('now'), ?)
   ```

**Files to create/modify:**
- `src/mesh/discovery.rs` - New module
- `src/routes/node.rs` - Health endpoint

---

## Phase 11: Dashboard UI Updates (User-Friendly)

### Tasks

1. **Update dashboard template** (`templates/dashboard.html`):
   - Show "My Devices" section
   - Device cards with friendly status:
     - ● Available (green dot)
     - ○ Offline (gray dot)
     - ⚠️ Needs re-pairing (yellow warning)
   - Show friendly capability descriptions:
     - "Can store media" not "media.storage"
     - "Can process video" not "media.transcode"
     - "Always on" not "always_on"
   - Add "Remove Device" button (not "Revoke Token")
   - Add "Re-pair" button for devices that need it
   - Hide all technical details (IPs, tokens, permissions)

2. **Add GraphQL queries to frontend**:
   ```graphql
   query {
     devices {
       id
       name
       status          # AVAILABLE, OFFLINE, NEEDS_REPAIR
       capabilities
       lastSeen
       needsRepair
       canStoreMedia
       canProcessVideo
       isAlwaysOn
     }
   }
   ```

3. **Add device actions**:
   ```graphql
   mutation {
     removeDevice(id: "phone-abc123")  # Not "revokeToken"
   }

   mutation {
     repairDevice(id: "phone-abc123") {
       qrCodeUrl
       pin
     }
   }
   ```

4. **Add friendly empty states**:
   - No devices: "Pair your first device to get started"
   - All offline: "No devices available right now"

**Files to modify:**
- `templates/dashboard.html` - Device list UI
- Frontend JS - GraphQL queries/mutations
- CSS - Status indicators (dots, colors)

**Design principle**: Could your grandma use this?

---

## Phase 12: Background Magic (Auto-Maintenance)

### Tasks

1. **Create `src/mesh/background.rs`**:
   ```rust
   // Health check loop (every 60s)
   pub async fn health_check_loop(db: DbPool)

   // Stale device cleanup (daily)
   pub async fn cleanup_loop(db: DbPool)

   // Token renewal (on every request via middleware)
   pub fn auto_renew_token(token: &IssuedToken) -> Result<()>
   ```

2. **Spawn background tasks in `src/main.rs`**:
   ```rust
   // After server starts
   tokio::spawn(mesh::background::health_check_loop(db.clone()));
   tokio::spawn(mesh::background::cleanup_loop(db.clone()));
   ```

3. **Implement health check**:
   - Ping all devices every 60 seconds
   - Update `discovery_cache` table
   - Update device status: AVAILABLE/OFFLINE/NEEDS_REPAIR
   - No user notification (just updates status)

4. **Implement stale cleanup**:
   - Find devices offline > 90 days
   - Optional: Prompt user to remove them
   - Or auto-archive (mark as "archived" not deleted)

5. **Add silent token renewal in middleware** (already in Phase 4):
   - Check if token expires in < 7 days
   - If yes: extend by 30 days
   - If no: just update last_used_at
   - Never tell user this happened

**Files to create/modify:**
- `src/mesh/background.rs` - New module
- `src/main.rs` - Spawn background tasks

**Result**: Devices stay healthy without user intervention

---

## Phase 13: Testing

### Tasks

1. **Unit tests**:
   - Token generation (uniqueness, length)
   - Token expiration logic
   - Token renewal logic
   - Permission verification

2. **Integration tests**:
   - Pairing flow end-to-end
   - Token exchange
   - Peer authentication
   - Token revocation

3. **E2E tests** (Playwright):
   - Pair two devices
   - Verify both see each other
   - Upload media from phone to desktop (using token)
   - Revoke token, verify upload fails

**Files to create/modify:**
- `src/mesh/tokens.rs` - Add `#[cfg(test)]` module
- `tests/` - Integration tests
- Playwright tests

---

## Phase 14: Documentation & Cleanup

### Tasks

1. **Update README.md**:
   - Explain token-based pairing
   - Document token lifecycle
   - Add examples

2. **Update API docs** (if any)

3. **Mark old designs as superseded**:
   - Update `MESH_CONCEPT.md`: "See TOKEN_BASED_MESH.md"
   - Update `CAPABILITY_MESH_DESIGN.md`: "Superseded by TOKEN_BASED_MESH.md"
   - Update `PAIRING_HANDSHAKE.md`: "Superseded by TOKEN_BASED_MESH.md"

4. **Clean up unused code**:
   - Remove `node_connections` table references
   - Remove old registration flow (if completely replaced)

**Files to modify:**
- `README.md`
- `docs/MESH_CONCEPT.md`
- `docs/CAPABILITY_MESH_DESIGN.md`
- `docs/PAIRING_HANDSHAKE.md`

---

## Rollout Plan

### Step 1: Backend Infrastructure (Phases 1-7)
- Database migrations
- Token generation & storage
- Node identity
- Token verification
- Pairing flow updates
- GraphQL API
- REST endpoints (device-centric, not token-centric)

**Checkpoint**: Backend API complete, testable via curl/GraphQL playground

### Step 2: User Experience (Phases 8-11)
- Error message translation (technical → friendly)
- Pairing UI (magical experience)
- Discovery & reachability
- Dashboard (My Devices, not "Node Management")

**Checkpoint**: Full pairing flow works between two devices, feels like Apple product

### Step 3: Auto-Maintenance (Phase 12)
- Background health checks
- Auto token renewal
- Stale device cleanup

**Checkpoint**: Everything works automatically, no user intervention needed

### Step 4: Quality (Phases 13-14)
- Tests (unit, integration, E2E)
- Documentation
- Cleanup

**Checkpoint**: Ready for production use

---

## Risk Assessment

### High Risk
- **Token security**: Must use cryptographically secure random
  - Mitigation: Use `ring::rand::SecureRandom`

- **Token leakage**: Tokens in logs, URLs, etc.
  - Mitigation: Never log tokens, only use in headers

### Medium Risk
- **Migration failures**: Existing data could be lost
  - Mitigation: Test migration on copy of production DB first

- **Auto-renewal edge cases**: Clock skew, race conditions
  - Mitigation: Use conservative thresholds (7 days)

### Low Risk
- **Performance**: Token verification on every request
  - Mitigation: Use DB index on token column, consider in-memory cache later

---

## Success Criteria

✅ **Functional**:
1. Can pair two devices via QR code + PIN
2. Both devices automatically know about each other after pairing
3. Can call peer endpoints using tokens
4. Tokens auto-renew on use
5. Expired tokens are rejected
6. Can revoke tokens from dashboard

✅ **Non-functional**:
1. Token generation is cryptographically secure
2. Pairing completes in < 5 seconds
3. Token verification adds < 10ms per request
4. Migration runs without data loss

---

## Timeline Estimate

- Phases 1-2: **2-3 hours** (Schema + tokens)
- Phases 3-4: **1-2 hours** (Node identity + middleware)
- Phase 5: **2-3 hours** (Pairing updates)
- Phases 6-7: **2-3 hours** (GraphQL + REST APIs)
- Phase 8: **1-2 hours** (Error message translation)
- Phases 9-11: **4-5 hours** (Frontend: pairing, discovery, dashboard)
- Phase 12: **2-3 hours** (Background tasks)
- Phases 13-14: **3-4 hours** (Testing + docs)

**Total: ~18-25 hours** for complete implementation (including "it just works" polish)

---

## Open Questions

1. **Token rotation**: Should we force rotation periodically? (Lean towards no for YAGNI)
2. **Permission granularity**: Start with coarse permissions or fine-grained? (Coarse: `media:*`, `posts:*`)
3. **mDNS discovery**: Implement now or later? (Later - manual pairing first)
4. **Token storage format**: Plain text or hashed? (Plain text for peer_tokens since we need to send them; could hash issued_tokens)

---

## Next Step

**Ready to implement?**

Start with Phase 1 (database migration) and work sequentially through phases.
