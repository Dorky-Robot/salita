# Pairing Handshake: Identity Exchange

**Key Insight:** When nodes successfully pair, they're establishing trust. This is the perfect moment to exchange identity metadata.

---

## Current Pairing Flow (Incomplete)

```
1. Desktop generates: code + PIN + linking_code
2. Phone scans QR, enters PIN
3. Phone → Desktop: POST /auth/pair/verify { code, pin, linking_code }
4. Desktop validates, creates session
5. Desktop → Phone: session cookie

MISSING: Desktop doesn't know about Phone!
```

**Problem:** Desktop creates session for phone, but doesn't learn anything about it. Phone isn't added to mesh automatically.

**Current workaround:** Separate manual step - "Add Device" with IP entry → calls `registerNode`

---

## Proposed: Bidirectional Handshake

### The Handshake Exchange

```
                DESKTOP                                PHONE
                   |                                      |
    1. Generate    |  /auth/pair/start                    |
       QR + PIN    |  → { code, pin, url }                |
                   |                                      |
                   |          [Phone scans QR]            |
                   |                                      |
                   |  /auth/pair/verify                   |  2. Send identity
                   |  ← { code, pin,                      |     + verify PIN
                   |      node_id, name,                  |
                   |      capabilities }                  |
                   |                                      |
    3. Verify PIN  |                                      |
       + Store     |  → { ok: true,                       |  4. Receive desktop
       phone info  |      desktop_node: {                 |     identity info
                   |        node_id, name,                |     + session
                   |        hostname, port,               |
                   |        capabilities                  |
                   |      },                              |
                   |      session_token }                 |
                   |                                      |
    5. Both nodes now know about each other!             |
                   |                                      |
```

---

## Implementation

### Step 1: Phone Sends Identity

**Phone → Desktop**
```http
POST /auth/pair/verify
Content-Type: application/json

{
  "code": "550e8400-...",
  "pin": "123456",
  "linking_code": "ABCD-1234",

  // NEW: Phone's identity
  "node_identity": {
    "node_id": "phone-abc-123",
    "name": "Felix's iPhone",
    "capabilities": ["posting", "media.capture", "media.upload"],
    "device_info": {
      "platform": "ios",
      "os_version": "17.2",
      "app_version": "1.0.0"
    }
  }
}
```

### Step 2: Desktop Stores Phone + Responds with Self

**Desktop handler:**
```rust
pub async fn pair_verify(
    State(state): State<AppState>,
    origin: RequestOrigin,
    Json(req): Json<PairVerifyRequest>,
) -> AppResult<Response> {
    // ... existing PIN verification ...

    // NEW: Store phone's identity in mesh
    let conn = state.db.get()?;
    let now = Utc::now().to_rfc3339();
    let caps_json = serde_json::to_string(&req.node_identity.capabilities)?;
    let metadata_json = serde_json::to_string(&req.node_identity.device_info)?;

    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata)
         VALUES (?1, ?2, ?3, ?4, 'online', ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           hostname = excluded.hostname,
           status = 'online',
           capabilities = excluded.capabilities,
           last_seen = excluded.last_seen,
           metadata = excluded.metadata",
        params![
            req.node_identity.node_id,
            req.node_identity.name,
            extract_ip_from_request(&origin)?, // Get phone's IP from connection
            6969, // Default port
            caps_json,
            now.clone(),
            now,
            metadata_json,
        ],
    )?;

    // NEW: Get desktop's own identity to send back
    let desktop_node = conn.query_row(
        "SELECT id, name, hostname, port, capabilities
         FROM mesh_nodes WHERE is_current = 1",
        [],
        |row| {
            Ok(NodeIdentityInfo {
                node_id: row.get(0)?,
                name: row.get(1)?,
                hostname: row.get(2)?,
                port: row.get(3)?,
                capabilities: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
            })
        },
    )?;

    // Create session for phone
    let token = session::create_session(&state.db, &linking.user_id, state.config.auth.session_hours)?;

    // NEW: Return desktop's identity + session
    let body = serde_json::json!({
        "ok": true,
        "session_token": token,
        "desktop_node": desktop_node,  // Phone can now register desktop!
    });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        AppendHeaders([(
            header::SET_COOKIE,
            session_cookie(&token, state.config.auth.session_hours),
        )]),
        serde_json::to_string(&body)?,
    ).into_response())
}
```

### Step 3: Phone Stores Desktop Identity

**Phone side (after pair/verify response):**
```javascript
const response = await fetch('/auth/pair/verify', {
  method: 'POST',
  body: JSON.stringify({
    code: code,
    pin: pin,
    linking_code: linkingCode,
    node_identity: {
      node_id: localStorage.getItem('salita_node_id'),
      name: "Felix's iPhone",
      capabilities: ["posting", "media.capture", "media.upload"],
      device_info: {
        platform: navigator.platform,
        app_version: "1.0.0"
      }
    }
  })
});

const result = await response.json();

if (result.ok) {
  // NEW: Store desktop's identity in our local mesh
  await fetch('/graphql', {
    method: 'POST',
    body: JSON.stringify({
      query: `mutation AnnounceNode($input: AnnounceNodeInput!) {
        announceNode(input: $input) { success }
      }`,
      variables: {
        input: {
          node_id: result.desktop_node.node_id,
          name: result.desktop_node.name,
          hostname: result.desktop_node.hostname,
          port: result.desktop_node.port,
          capabilities: result.desktop_node.capabilities
        }
      }
    })
  });

  // Now both nodes know about each other!
  window.location.href = '/';
}
```

---

## Benefits

✅ **Automatic mutual registration** - Both nodes learn about each other during pairing
✅ **No separate "Add Device" step** - Happens automatically
✅ **Identity persistence** - Node IDs exchanged, not regenerated
✅ **Capability discovery** - Each node knows what the other can do
✅ **IP captured accurately** - Desktop sees phone's real IP from connection
✅ **Symmetric relationship** - Both nodes have equal knowledge

---

## Data Model Updates

### New types for handshake

```rust
#[derive(Deserialize)]
pub struct NodeIdentityInfo {
    pub node_id: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub device_info: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct PairVerifyRequest {
    pub code: String,
    pub pin: String,
    pub linking_code: String,

    // NEW: Include identity
    pub node_identity: NodeIdentityInfo,
}

#[derive(Serialize)]
pub struct PairVerifyResponse {
    pub ok: bool,
    pub session_token: String,

    // NEW: Send back desktop's identity
    pub desktop_node: NodeIdentityInfo,
}
```

### GraphQL changes

```graphql
# Rename registerNode → announceNode (better semantics)
mutation {
  announceNode(input: AnnounceNodeInput!): NodeOperationResult!
}

input AnnounceNodeInput {
  node_id: String!        # Now required (not generated)
  name: String!
  hostname: String!
  port: Int!
  capabilities: [String!]
  metadata: JSON
}
```

---

## What About First Boot?

**Node needs its own identity before it can pair!**

### Solution: Generate on startup

```rust
// src/node_identity.rs
pub struct NodeIdentity {
    pub id: String,
    pub name: String, // Can be changed later in settings
    pub created_at: DateTime<Utc>,
}

impl NodeIdentity {
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("node_identity.json");
        if path.exists() {
            let json = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            // Generate new identity
            let identity = Self {
                id: format!("node-{}", uuid::Uuid::now_v7()),
                name: default_node_name(),
                created_at: Utc::now(),
            };
            fs::write(path, serde_json::to_string_pretty(&identity)?)?;
            Ok(identity)
        }
    }
}

fn default_node_name() -> String {
    // Try to get hostname
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Salita Node".to_string())
}
```

**On Salita startup:**
```rust
// src/main.rs
#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;

    // Load or create node identity
    let node_identity = NodeIdentity::load_or_create(&config.data_dir)?;

    // Initialize current node in database
    let conn = db_pool.get()?;
    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, is_current)
         VALUES (?1, ?2, ?3, ?4, 'online', '[]', datetime('now'), datetime('now'), 1)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           status = 'online',
           last_seen = datetime('now')",
        params![
            node_identity.id,
            node_identity.name,
            "localhost",
            config.server.port,
        ],
    )?;

    // ... continue startup
}
```

---

## Updated Pairing Flow

### From User's Perspective

**Before (Manual):**
1. Desktop: Click "Connect Device" → see QR
2. Phone: Scan QR, enter PIN
3. Phone: Logged in! ✓
4. Desktop: Click "Add Device", manually enter phone's IP 🤦
5. Desktop: Phone appears in mesh

**After (Automatic):**
1. Desktop: Click "Connect Device" → see QR
2. Phone: Scan QR, enter PIN
3. ✨ **Both devices instantly appear in each other's mesh** ✨
4. Done!

---

## Implementation Phases

### Phase 1: Node Identity Module
- [ ] Create `src/node_identity.rs`
- [ ] Load/generate identity on startup
- [ ] Initialize current node with persistent ID
- [ ] Add `/api/node/identity` endpoint to expose current node's ID

### Phase 2: Handshake Exchange
- [ ] Update `PairVerifyRequest` to include `node_identity`
- [ ] Update `pair_verify` handler to store phone's info
- [ ] Update response to include desktop's identity
- [ ] Phone stores desktop identity after pairing

### Phase 3: Rename & Clean Up
- [ ] Rename `registerNode` → `announceNode`
- [ ] Add upsert logic (ON CONFLICT DO UPDATE)
- [ ] Update UI to show "paired devices"
- [ ] Remove manual "Add Device" flow (or make it fallback)

---

## Open Questions

1. **What if phone's IP is behind NAT?**
   - Desktop sees public IP, not LAN IP
   - Solution: Phone includes its own LAN IP in handshake
   - Or: Use mDNS to discover actual LAN address

2. **What if pairing happens over internet (ngrok)?**
   - Desktop sees ngrok IP, not useful
   - Need explicit hostname exchange
   - Or: Mark as "remote" node, requires public URL

3. **How to update node info later?**
   - Add `/api/node/heartbeat` that includes updated info
   - Nodes periodically announce themselves
   - Keeps mesh fresh even after IP changes

4. **Security: Can phone lie about capabilities?**
   - Yes, but it's your own device
   - If concerned, add capability verification
   - Desktop tests: "You claim media.transcode? Prove it!"

---

## This Solves Multiple Problems!

1. ✅ **Stable identity** - Node IDs exchanged during trust establishment
2. ✅ **Automatic discovery** - No manual "Add Device" needed
3. ✅ **Capability awareness** - Nodes know what others can do immediately
4. ✅ **Symmetric relationship** - Both nodes register each other
5. ✅ **Works today** - No mDNS needed (though we can add it later)

**This is elegant!** The pairing ceremony *is* the mesh join ceremony.

Want to implement this? We could start with Phase 1 (node identity module).
