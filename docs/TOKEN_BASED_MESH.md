# Token-Based Mesh: Peer Services with API Keys

**Status:** Active Design (Supersedes CAPABILITY_MESH_DESIGN.md and PAIRING_HANDSHAKE.md)

---

## Core Principles

1. **All nodes are equal peers** - No anchors, no hierarchy, just devices with different capabilities
2. **Token-based authentication** - Nodes exchange tokens (like API keys) during pairing **[INVISIBLE TO USER]**
3. **On-demand connections** - Connect when needed, not persistent mesh connections
4. **Capability-driven discovery** - "Who can do X and is reachable right now?"
5. **It just works™** - "Send photo to laptop" → it happens. No token management, no configuration.

---

## User Experience Philosophy

### What Users See

```
Pairing:
  1. Desktop: "Connect New Device" → Shows QR code
  2. Phone: Scan QR → Enter PIN (123456)
  3. ✓ Done!

Using:
  1. Take photo on phone
  2. Tap "Send to My Laptop"
  3. ✓ Photo appears on laptop

Managing:
  1. Settings → My Devices
  2. See: "iPhone (Available)", "Old Laptop (Offline)"
  3. Tap "Remove" to unpair
```

**Users NEVER see:**
- Tokens
- Permissions
- Expiration dates
- "Bearer authorization"
- Error messages about tokens

### What Users NEVER Do

❌ Copy/paste tokens
❌ Manage API keys
❌ Set permissions
❌ Renew anything
❌ Configure discovery

### Behind the Scenes (Completely Invisible)

The token infrastructure handles everything automatically:
- Token exchange during pairing ✓
- Auto-renewal on use ✓
- Capability matching ✓
- Reachability checks ✓
- Graceful degradation ✓

---

## Mental Model

### Every device is a service

```
Desktop = Service {
  capabilities: ["media.storage", "media.transcode", "compute.background"]
  access_token: "desktop_abc123"  // Others use this to call me
}

Phone = Service {
  capabilities: ["posting", "media.capture"]
  access_token: "phone_xyz789"  // Others use this to call me
}
```

### Pairing = Token Exchange

```
When Phone pairs with Desktop:
  1. Phone → Desktop: "Here's my identity + capabilities"
  2. Desktop → Phone: "Here's my token to call me: desktop_abc123"
  3. Phone stores: { peer: Desktop, token: desktop_abc123 }
  4. Desktop stores: { peer: Phone } (no token needed, Phone didn't offer services)
```

### Usage = On-Demand Service Call

```
Phone wants to upload photo:
  1. Query local DB: "Who has 'media.storage' capability?"
     → Desktop, Old Laptop

  2. Ping both: "Are you alive?"
     → Desktop: ✓ (200ms)
     → Laptop: ✗ (timeout)

  3. Call Desktop:
     POST https://desktop.local/media/upload
     Authorization: Bearer desktop_abc123

  4. Done. Close connection.
```

---

## Data Model

### 1. Mesh Nodes (All Devices)

```sql
CREATE TABLE mesh_nodes (
  id TEXT PRIMARY KEY,              -- UUID
  name TEXT NOT NULL,               -- "Felix's iPhone", "Old Laptop"

  -- Discovery
  hostname TEXT,                    -- IP or hostname (can change)
  port INTEGER DEFAULT 6969,

  -- Capabilities
  capabilities TEXT NOT NULL DEFAULT '[]',  -- JSON array

  -- Connection
  status TEXT DEFAULT 'unknown',    -- 'online', 'offline', 'unknown'
  last_seen TEXT,

  -- Metadata
  metadata TEXT,                    -- JSON: storage_free, battery, etc.
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Mark which node is "this device"
CREATE TABLE current_node (
  node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id)
);

CREATE INDEX idx_mesh_nodes_capabilities ON mesh_nodes(capabilities);
CREATE INDEX idx_mesh_nodes_status ON mesh_nodes(status);
```

### 2. Peer Tokens (How I Call Others)

```sql
CREATE TABLE peer_tokens (
  peer_node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
  token TEXT NOT NULL UNIQUE,       -- Bearer token to authenticate with peer
  permissions TEXT NOT NULL,        -- JSON array: ["media:upload", "posts:create"]

  -- Token lifecycle
  issued_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT NOT NULL,         -- Auto-renew on use
  last_used_at TEXT,                -- Track activity for auto-renewal

  PRIMARY KEY (peer_node_id)
);

CREATE INDEX idx_peer_tokens_expires ON peer_tokens(expires_at);
```

### 3. Issued Tokens (Who Can Call Me)

```sql
CREATE TABLE issued_tokens (
  token TEXT PRIMARY KEY,           -- The actual bearer token
  issued_to_node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
  permissions TEXT NOT NULL,        -- JSON array

  -- Token lifecycle
  issued_at TEXT NOT NULL DEFAULT (datetime('now')),
  expires_at TEXT NOT NULL,
  last_used_at TEXT,
  revoked_at TEXT,                  -- NULL = active, set = revoked

  CHECK (revoked_at IS NULL OR revoked_at >= issued_at)
);

CREATE INDEX idx_issued_tokens_node ON issued_tokens(issued_to_node_id);
CREATE INDEX idx_issued_tokens_expires ON issued_tokens(expires_at);
CREATE INDEX idx_issued_tokens_revoked ON issued_tokens(revoked_at) WHERE revoked_at IS NULL;
```

### 4. Discovery Cache (Reachability Status)

```sql
CREATE TABLE discovery_cache (
  node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id) ON DELETE CASCADE,
  reachable BOOLEAN NOT NULL,
  last_ping_at TEXT NOT NULL,
  response_time_ms INTEGER,
  health_metadata TEXT,             -- JSON from /health endpoint

  CHECK (response_time_ms IS NULL OR response_time_ms >= 0)
);
```

---

## Token Lifecycle

### Auto-Expiring + Auto-Renewing

**Default TTL:** 30 days

**Auto-renewal logic:**
```
When token is used:
  1. Check: Is it expired?
     → Yes: Reject with 401 Unauthorized
     → No: Continue

  2. Check: Is it close to expiring? (< 7 days left)
     → Yes: Extend expires_at by 30 days from now
     → No: Just update last_used_at

  3. Process request
```

**Example:**
```
Day 0:  Token issued, expires_at = Day 30
Day 15: Token used → last_used_at updated, expires_at unchanged
Day 25: Token used → expires_at extended to Day 55
Day 40: Token used → expires_at extended to Day 70
...
(Token stays alive as long as it's used at least once per 30 days)

Day 100: Token not used for 30 days → expires → next use fails with 401
```

### Manual Revocation

```sql
-- Revoke a token (can't be un-revoked)
UPDATE issued_tokens
SET revoked_at = datetime('now')
WHERE issued_to_node_id = ?;

-- Token verification checks revoked_at IS NULL
```

---

## Pairing Flow (Token Exchange)

### Step 1: Desktop generates pairing code

```rust
POST /auth/pair/start
→ {
  "code": "550e8400-...",
  "pin": "123456",
  "expires_at": "2026-02-08T11:00:00Z",
  "qr_url": "salita://pair?code=550e8400&url=https://desktop.local:6969"
}
```

### Step 2: Phone scans QR + sends identity

```http
POST /auth/pair/verify
Content-Type: application/json

{
  "code": "550e8400-...",
  "pin": "123456",

  "node_identity": {
    "node_id": "phone-abc123",
    "name": "Felix's iPhone",
    "hostname": "192.168.1.75",
    "port": 6969,
    "capabilities": ["posting", "media.capture"]
  }
}
```

### Step 3: Desktop verifies PIN + creates token

```rust
// 1. Verify PIN
let linking = get_linking_by_code_and_pin(&code, &pin)?;

// 2. Store Phone as peer
db.execute(
    "INSERT INTO mesh_nodes (id, name, hostname, port, capabilities, status, last_seen)
     VALUES (?1, ?2, ?3, ?4, ?5, 'online', datetime('now'))
     ON CONFLICT(id) DO UPDATE SET
       name = excluded.name,
       hostname = excluded.hostname,
       port = excluded.port,
       capabilities = excluded.capabilities,
       status = 'online',
       last_seen = datetime('now')",
    params![
        req.node_identity.node_id,
        req.node_identity.name,
        req.node_identity.hostname,
        req.node_identity.port,
        serde_json::to_string(&req.node_identity.capabilities)?
    ]
)?;

// 3. Generate token for Phone to call Desktop
let desktop_token = generate_secure_token(); // 32-byte random
let expires_at = Utc::now() + Duration::days(30);

db.execute(
    "INSERT INTO issued_tokens (token, issued_to_node_id, permissions, expires_at)
     VALUES (?1, ?2, ?3, ?4)",
    params![
        desktop_token,
        req.node_identity.node_id,
        serde_json::to_string(&["media:upload", "media:read", "posts:create"])?,
        expires_at.to_rfc3339()
    ]
)?;

// 4. Get Desktop's own identity
let desktop_node = get_current_node(&db)?;

// 5. Return Desktop's identity + token
Ok(Json(PairVerifyResponse {
    ok: true,
    session_token: create_user_session(...)?, // For web UI
    peer_node: PeerNodeInfo {
        node_id: desktop_node.id,
        name: desktop_node.name,
        hostname: desktop_node.hostname,
        port: desktop_node.port,
        capabilities: desktop_node.capabilities,
        access_token: desktop_token,
        permissions: vec!["media:upload", "media:read", "posts:create"],
        expires_at: expires_at.to_rfc3339(),
    }
}))
```

### Step 4: Phone stores Desktop's token

```javascript
const response = await fetch('/auth/pair/verify', { ... });
const result = await response.json();

if (result.ok) {
  // Store Desktop as peer
  await db.execute(
    `INSERT INTO mesh_nodes (id, name, hostname, port, capabilities, status, last_seen)
     VALUES (?, ?, ?, ?, ?, 'online', datetime('now'))
     ON CONFLICT(id) DO UPDATE SET
       name = excluded.name,
       hostname = excluded.hostname,
       status = 'online',
       last_seen = datetime('now')`,
    [
      result.peer_node.node_id,
      result.peer_node.name,
      result.peer_node.hostname,
      result.peer_node.port,
      JSON.stringify(result.peer_node.capabilities)
    ]
  );

  // Store token to call Desktop
  await db.execute(
    `INSERT INTO peer_tokens (peer_node_id, token, permissions, expires_at)
     VALUES (?, ?, ?, ?)
     ON CONFLICT(peer_node_id) DO UPDATE SET
       token = excluded.token,
       expires_at = excluded.expires_at`,
    [
      result.peer_node.node_id,
      result.peer_node.access_token,
      JSON.stringify(result.peer_node.permissions),
      result.peer_node.expires_at
    ]
  );

  // ✓ Both devices now know about each other!
}
```

---

## Error Handling: User-Friendly Degradation

### When Things Go Wrong (Invisible Recovery)

**Scenario: Token expired (not renewed for 30+ days)**

❌ Bad UX:
```
Error: Bearer token expired. Please re-authenticate.
```

✅ Good UX:
```
Device Status:
  📱 iPhone - Needs re-pairing

[Tap to re-pair]
```

User taps → Shows QR code → Scan → Done. New token issued invisibly.

---

**Scenario: Device offline**

❌ Bad UX:
```
Connection refused: 192.168.1.100:6969
```

✅ Good UX:
```
🖥️ Laptop is offline

Try:
  • Make sure it's turned on
  • Check it's on the same network

[Try Again]
```

---

**Scenario: Permission denied (shouldn't happen in normal use)**

❌ Bad UX:
```
403 Forbidden: Token lacks "media:upload" permission
```

✅ Good UX:
```
Couldn't upload to Laptop

[Try another device] [Report Problem]
```

System logs the real error for debugging, user sees friendly message.

---

## Using the Mesh: Upload Photo Example

### 1. Query capable nodes

```graphql
query {
  capableNodes(capability: "media.storage") {
    id
    name
    hostname
    port
    lastSeen
  }
}
```

Returns:
```json
[
  { "id": "desktop-xyz", "name": "Desktop", "hostname": "192.168.1.100", "port": 6969 },
  { "id": "laptop-abc", "name": "Old Laptop", "hostname": "192.168.1.120", "port": 6969 }
]
```

### 2. Check reachability

```javascript
async function findReachableNode(nodes) {
  const results = await Promise.allSettled(
    nodes.map(async node => {
      const token = await getTokenForPeer(node.id);
      const start = Date.now();

      const response = await fetch(`https://${node.hostname}:${node.port}/health`, {
        method: 'GET',
        headers: { 'Authorization': `Bearer ${token}` },
        signal: AbortSignal.timeout(2000)
      });

      const elapsed = Date.now() - start;
      return { node, online: response.ok, latency: elapsed };
    })
  );

  const online = results
    .filter(r => r.status === 'fulfilled' && r.value.online)
    .map(r => r.value)
    .sort((a, b) => a.latency - b.latency);

  return online[0]?.node; // Return fastest
}
```

### 3. Upload to selected node

```javascript
const target = await findReachableNode(capableNodes);
if (!target) {
  throw new Error("No storage nodes available");
}

const token = await getTokenForPeer(target.id);
const formData = new FormData();
formData.append('file', photoBlob);

const response = await fetch(`https://${target.hostname}:${target.port}/media/upload`, {
  method: 'POST',
  headers: { 'Authorization': `Bearer ${token}` },
  body: formData
});

const result = await response.json();
// { url: "https://desktop.local/media/photo123.jpg" }
```

---

## Token Verification (Server-Side)

### Middleware to verify bearer tokens

```rust
pub async fn verify_peer_token(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract bearer token
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up token
    let (node_id, permissions, expires_at, revoked_at) = conn
        .query_row(
            "SELECT issued_to_node_id, permissions, expires_at, revoked_at
             FROM issued_tokens
             WHERE token = ?",
            [token],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Check revoked
    if revoked_at.is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check expired
    let expires_at = DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = Utc::now();

    if expires_at < now {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Auto-renew if close to expiring (< 7 days)
    if expires_at < now + Duration::days(7) {
        let new_expires = now + Duration::days(30);
        conn.execute(
            "UPDATE issued_tokens
             SET expires_at = ?, last_used_at = datetime('now')
             WHERE token = ?",
            params![new_expires.to_rfc3339(), token],
        )
        .ok(); // Don't fail request if renewal fails
    } else {
        // Just update last_used_at
        conn.execute(
            "UPDATE issued_tokens SET last_used_at = datetime('now') WHERE token = ?",
            [token],
        )
        .ok();
    }

    // Store peer identity in request extensions
    req.extensions_mut().insert(PeerNode {
        node_id,
        permissions: serde_json::from_str(&permissions).unwrap_or_default(),
    });

    Ok(next.run(req).await)
}

// Use in routes
async fn handle_media_upload(
    Extension(peer): Extension<PeerNode>,
    // ... other params
) -> Result<Response> {
    // Check permission
    if !peer.permissions.contains(&"media:upload".to_string()) {
        return Err(StatusCode::FORBIDDEN);
    }

    // Process upload...
}
```

---

## API Endpoints

### Device Management (User-Facing)

```
GET  /api/devices                - List all paired devices
  → [
      { "id": "...", "name": "iPhone", "status": "available", "lastSeen": "..." },
      { "id": "...", "name": "Laptop", "status": "offline", "lastSeen": "..." }
    ]

GET  /api/devices/:id            - Get device details
  → { "id": "...", "name": "iPhone", "capabilities": [...], "status": "..." }

DELETE /api/devices/:id          - Remove device (revokes tokens invisibly)
  → { "ok": true }

POST /api/devices/:id/repair     - Re-pair device (issues new token)
  → { "qr_code": "...", "pin": "123456" }

POST /api/devices/discover       - Check which devices are reachable right now
  → { "available": [...], "offline": [...] }
```

### Current Device (User-Facing)

```
GET  /api/device/identity        - Get this device's info
  → { "id": "...", "name": "Felix's Desktop", "capabilities": [...] }

POST /api/device/identity        - Update this device's name
  → { "ok": true }
```

### Internal Endpoints (Not User-Facing)

```
GET  /health                     - Health check for peer devices
  Authorization: Bearer <peer_token>
  → { "ok": true, "node_id": "...", "capabilities": [...], "metadata": {...} }

POST /media/upload               - Upload media from peer
  Authorization: Bearer <peer_token>
  Content-Type: multipart/form-data
  → { "url": "https://...", "thumbnail_url": "..." }
```

**Note**: Tokens are never exposed in user-facing APIs. They're only used in peer-to-peer calls (Authorization headers).

---

## GraphQL Extensions (User-Facing)

```graphql
type Query {
  # Current device
  currentDevice: Device!

  # All paired devices
  devices: [Device!]!
  device(id: String!): Device

  # Smart queries (internally check capabilities + reachability)
  availableDevices: [Device!]!
  devicesForTask(task: TaskType!): [Device!]!
}

type Mutation {
  # Pairing
  startPairing: PairingCode!

  # Device management
  updateDeviceName(id: String!, name: String!): Device!
  removeDevice(id: String!): Boolean!
  repairDevice(id: String!): PairingCode!

  # Discovery (automatic in background, but user can trigger)
  checkDevices: DiscoveryResult!
}

# User-friendly device representation
type Device {
  id: String!
  name: String!
  status: DeviceStatus!
  capabilities: [String!]!
  lastSeen: DateTime
  needsRepair: Boolean!     # Token expired → needs re-pairing

  # Friendly capability descriptions
  canStoreMedia: Boolean!
  canProcessVideo: Boolean!
  isAlwaysOn: Boolean!
}

enum DeviceStatus {
  AVAILABLE    # Online and ready
  OFFLINE      # Can't reach right now
  NEEDS_REPAIR # Token expired, needs re-pairing
}

enum TaskType {
  STORE_MEDIA
  PROCESS_VIDEO
  STORE_LARGE_FILE
  RUN_BACKGROUND_JOB
}

type PairingCode {
  code: String!
  pin: String!
  qrCodeUrl: String!
  expiresAt: DateTime!
}

type DiscoveryResult {
  available: [Device!]!
  offline: [Device!]!
  needsRepair: [Device!]!
}
```

### Internal Types (Not Exposed to GraphQL)

Tokens, permissions, and low-level details are handled internally:
- `peer_tokens` table
- `issued_tokens` table
- Token verification middleware
- Auto-renewal logic

Users interact with "Devices" not "Tokens".

---

## Security Considerations

### Token Generation

- Use `ring::rand::SecureRandom` for cryptographically secure tokens
- 32 bytes (256 bits) minimum
- Base64url encoded for URL safety
- Example: `"dGVzdF90b2tlbl8xMjM0NTY3ODkwYWJjZGVmZ2hpamtsbW5vcA=="`

### HTTPS Only

- All peer-to-peer calls MUST use HTTPS
- Self-signed certs OK for LAN (trust on first use)
- Public nodes should use proper certs (Let's Encrypt)

### Permission Scoping

- Default permissions: `["posts:read"]` (read-only)
- Media upload: `["media:upload", "media:read"]`
- Full access: `["*"]` (use sparingly)

### Rate Limiting

- Per-token rate limits (optional, YAGNI for now)
- Track last_used_at to detect abuse

---

## Background Magic: Keep It Working Automatically

### Automatic Background Tasks

**1. Health Check Loop (every 60 seconds)**
```rust
loop {
    // Ping all devices
    for device in get_all_devices() {
        let reachable = ping_device(&device).await;
        update_status(&device.id, reachable);
    }
    sleep(60).await;
}
```
- Updates device status: Available/Offline/Needs Repair
- Completely silent - user never sees this
- Only shows result: green dot (available) vs gray dot (offline)

**2. Token Renewal (on every request)**
```rust
// When device A calls device B:
if token.expires_in() < 7.days() {
    token.renew_silently();  // Extends by 30 days
}
```
- Happens automatically during normal use
- User never knows tokens exist
- Only fails if device unused for 30+ days → "Needs repair"

**3. Stale Device Cleanup (daily)**
```rust
// Remove devices offline for 90+ days
for device in get_devices_offline_for(90.days()) {
    if user_confirms("Remove {device.name}? It hasn't been seen in 3 months") {
        remove_device(&device);
    }
}
```
- Keeps device list clean
- Optional user confirmation
- Prevents clutter from old devices

**4. Automatic mDNS Discovery (optional, later)**
```rust
// Listen for Salita devices on LAN
mdns_listener.on_discovered(|service| {
    let device_id = service.get_property("node_id");
    if known_device(device_id) && ip_changed(device_id) {
        update_device_hostname(device_id, service.hostname);
        // User sees: "Laptop" status changes from Offline → Available
    }
});
```
- Automatically finds devices when they come online
- Updates IPs when devices move networks
- User just sees device appear as "Available"

---

## User-Facing Status Indicators

### Device List UI

```
My Devices:

📱 iPhone
   ● Available • Just now
   Can: Capture photos, Create posts

🖥️ Desktop
   ● Available • 2 minutes ago
   Can: Store media, Process video, Always on

💻 Old Laptop
   ○ Offline • Last seen 3 hours ago
   Can: Store media

🍓 Raspberry Pi
   ⚠️ Needs re-pairing • Last seen 45 days ago
   Can: Store media, Always on

   [Re-pair Now]
```

**Status meanings:**
- `● Available` = Reachable right now (green)
- `○ Offline` = Not reachable (gray)
- `⚠️ Needs re-pairing` = Token expired, scan QR again (yellow)

**User never sees:**
- "Token expired"
- "Bearer authentication failed"
- "Permission denied"
- IP addresses (unless advanced mode)

---

## Future Enhancements (Not Now)

- **mDNS discovery**: Auto-discover nodes on LAN
- **Token rotation**: Proactive rotation (not just expiration)
- **Webhook notifications**: Node comes online → notify peers
- **SLA tracking**: Measure actual uptime/reliability
- **Permission requests**: Node asks for permission upgrade
- **Multi-user tokens**: Token for specific user, not just node

---

## Migration from Current Schema

### Existing tables to update:

```sql
-- mesh_nodes: Already good, minor tweaks
-- Add: metadata column
ALTER TABLE mesh_nodes ADD COLUMN metadata TEXT;

-- Drop: connections table (we don't track persistent connections)
DROP TABLE IF EXISTS node_connections;
```

### New tables to create:

```sql
-- current_node, peer_tokens, issued_tokens, discovery_cache (see above)
```

---

## Next Steps

1. **Database migration**: Add new tables, update mesh_nodes
2. **Token generation module**: Secure random tokens
3. **Update pairing flow**: Exchange tokens, not just metadata
4. **Token verification middleware**: Verify + auto-renew
5. **GraphQL mutations**: Node/token management
6. **Update dashboard UI**: Show peers + token status
7. **Test pairing flow**: End-to-end with two devices

---

## Summary

**What changed from previous design:**
- ❌ No "anchor" vs "roaming" hierarchy
- ❌ No persistent mesh connections
- ❌ No SLA tracking (YAGNI)
- ✅ Token-based peer authentication
- ✅ Auto-expiring + auto-renewing tokens
- ✅ On-demand connections only
- ✅ Simple capability query + reachability check

**Core user flow:**
1. Pair devices → tokens exchanged automatically
2. Use device → "Send photo to laptop" → finds laptop, uploads with token
3. Manage devices → See list, revoke access if needed

**Developer mental model:**
- Devices = microservices with API keys
- Pairing = API key issuance
- Usage = authenticated REST calls
