# Node Identity & Discovery

**Problem:** Nodes need stable identity even when their IP addresses change.

---

## Current State

**What we have:**
```sql
mesh_nodes (
  id TEXT PRIMARY KEY,        -- UUID, assigned by OTHER node during registration
  hostname TEXT NOT NULL,     -- IP or hostname (changes!)
  ...
)
```

**The issue:**
1. Phone pairs with Desktop at IP `192.168.1.50`
2. Desktop creates entry: `{ id: "abc-123", hostname: "192.168.1.50" }`
3. Phone disconnects, gets new IP `192.168.1.75`
4. Phone tries to reconnect...
   - Desktop doesn't recognize it
   - Creates NEW entry: `{ id: "xyz-789", hostname: "192.168.1.75" }`
   - Now there are 2 phone entries, one is stale

---

## Solution Options

### Option 1: Self-Declared Identity (Simplest)

**Idea:** Nodes know their own ID and declare it on connection.

**How it works:**
```
First time:
  Phone: "Hi, I'm node-abc-123 at 192.168.1.50"
  Desktop: "Nice to meet you!" (creates entry)

IP changes:
  Phone: "Hi, I'm node-abc-123 at 192.168.1.75"
  Desktop: "Oh, you moved! Updating hostname..." (updates existing entry)
```

**Implementation:**
```sql
-- Nodes generate their own ID on first boot
-- Store in ~/.salita/node_identity.json
{
  "node_id": "550e8400-e29b-41d4-a716-446655440000",
  "created_at": "2026-02-08T10:30:00Z"
}
```

```rust
// On pairing/join
POST /mesh/announce
{
  "node_id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "Felix's iPhone",
  "hostname": "192.168.1.75",
  "port": 6969,
  "capabilities": ["posting", "media.capture"]
}

// Handler does:
INSERT INTO mesh_nodes (...) VALUES (...)
  ON CONFLICT(id) DO UPDATE SET
    hostname = excluded.hostname,
    port = excluded.port,
    last_seen = datetime('now'),
    status = 'online'
```

**Pros:**
- ✅ Simple - nodes just remember their own ID
- ✅ No extra infrastructure needed
- ✅ Works with current architecture

**Cons:**
- ⚠️ ID stored in file - could be lost if data dir cleared
- ⚠️ No cryptographic proof (nodes could claim fake IDs)
- ⚠️ Requires changing registration flow

---

### Option 2: mDNS/Bonjour Discovery

**Idea:** Nodes advertise themselves on the local network.

**How it works:**
```
Phone starts Salita:
  → Broadcasts: "salita-node-abc-123._salita._tcp.local"
  → Includes: hostname, port, capabilities

Desktop listens:
  → Discovers: "salita-node-abc-123" at 192.168.1.75:6969
  → Updates mesh_nodes table automatically
```

**Example (using mdns crate):**
```rust
use mdns_sd::{ServiceDaemon, ServiceInfo};

// Phone advertises
let service = ServiceInfo::new(
    "_salita._tcp.local.",
    "node-abc-123",  // Instance name (our node ID)
    "192.168.1.75",
    6969,
    &[
        ("id", "550e8400-e29b-41d4-a716-446655440000"),
        ("name", "Felix's iPhone"),
        ("caps", "posting,media.capture")
    ]
)?;
daemon.register(service)?;

// Desktop listens
for event in daemon.browse("_salita._tcp.local.")? {
    match event {
        ServiceEvent::ServiceResolved(info) => {
            let node_id = info.get_property_val_str("id").unwrap();
            let hostname = info.get_hostname();
            // Update mesh_nodes...
        }
    }
}
```

**Pros:**
- ✅ Automatic discovery - no manual IP entry needed
- ✅ Real-time updates when IPs change
- ✅ Works across LAN without central server
- ✅ Standard protocol (Bonjour/Avahi compatible)

**Cons:**
- ⚠️ LAN-only (doesn't work across the internet)
- ⚠️ Requires mDNS daemon running
- ⚠️ Some networks block multicast (corporate WiFi, etc.)

---

### Option 3: Cryptographic Identity (Most Secure)

**Idea:** Nodes have public/private keypairs. Public key = identity.

**How it works:**
```
First boot:
  Node generates keypair (Ed25519)
  Public key becomes node ID: "ed25519:AbCd1234..."
  Private key stored securely

On connection:
  Node signs a challenge with private key
  Other node verifies with public key
  Proves identity cryptographically
```

**Schema:**
```sql
mesh_nodes (
  id TEXT PRIMARY KEY,              -- "ed25519:AbCd1234..." (public key)
  public_key_bytes BLOB NOT NULL,   -- Raw key for verification
  ...
)

-- Local storage
node_identity (
  id TEXT PRIMARY KEY,
  public_key BLOB NOT NULL,
  private_key BLOB NOT NULL  -- Encrypted at rest
)
```

**Authentication flow:**
```rust
// Phone connects to Desktop
Phone: "Hi, I'm ed25519:AbCd1234..."
Desktop: "Prove it! Sign this: [random challenge]"
Phone: [signs challenge with private key]
Desktop: [verifies signature with public key]
Desktop: "Welcome back! Updating your hostname..."
```

**Pros:**
- ✅ Cryptographically unforgeable identity
- ✅ No central authority needed
- ✅ Can't impersonate other nodes
- ✅ Foundation for encrypted mesh communication

**Cons:**
- ⚠️ More complex to implement
- ⚠️ Key management (backup, recovery, rotation)
- ⚠️ Overkill if mesh is just your own devices

---

### Option 4: Hybrid (Recommended)

**Combine Option 1 + Option 2:**

**Self-declared ID + mDNS for discovery**

```rust
// Node storage: ~/.salita/node_identity.json
{
  "node_id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "Felix's iPhone",
  "created_at": "2026-02-08T10:30:00Z"
}

// On startup:
1. Load node_id from disk (or generate if first boot)
2. Advertise via mDNS: "salita-{node_id}._salita._tcp.local"
3. Listen for other Salita nodes on LAN

// On discovery:
1. Other node found via mDNS
2. HTTP ping to verify it's alive
3. Upsert into mesh_nodes (by node_id, update hostname)
```

**Pros:**
- ✅ Simple persistent identity (Option 1)
- ✅ Automatic discovery (Option 2)
- ✅ Works great for home network
- ✅ Falls back to manual entry if mDNS unavailable

**Cons:**
- ⚠️ Still LAN-only for discovery
- ⚠️ For internet access, need additional mechanism

---

## Recommendation: Start with Option 1, Add Option 2

### Phase 1: Self-Declared Identity ✨

**Changes needed:**

1. **Node identity file:**
```rust
// src/node_identity.rs
pub struct NodeIdentity {
    pub id: String,
    pub created_at: DateTime<Utc>,
}

impl NodeIdentity {
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("node_identity.json");
        if path.exists() {
            // Load existing
            let json = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            // Generate new
            let identity = NodeIdentity {
                id: uuid::Uuid::now_v7().to_string(),
                created_at: Utc::now(),
            };
            fs::write(path, serde_json::to_string_pretty(&identity)?)?;
            Ok(identity)
        }
    }
}
```

2. **Initialize current node with stable ID:**
```sql
-- migrations/005_mesh_network.sql
-- Change from:
INSERT OR IGNORE INTO mesh_nodes (id, name, ...)
VALUES (hex(randomblob(16)), ...)

-- To:
-- Populated by application code with loaded node_identity.id
```

3. **Change registration to upsert:**
```rust
// GraphQL mutation
async fn announce_node(
    &self,
    ctx: &Context<'_>,
    input: AnnounceNodeInput,  // Now includes node_id!
) -> Result<NodeOperationResult> {
    // Use INSERT ... ON CONFLICT DO UPDATE
    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at)
         VALUES (?1, ?2, ?3, ?4, 'online', ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
           name = excluded.name,
           hostname = excluded.hostname,
           port = excluded.port,
           status = 'online',
           capabilities = excluded.capabilities,
           last_seen = excluded.last_seen",
        params![
            input.node_id,  // Now provided by the node itself!
            input.name,
            input.hostname,
            input.port,
            capabilities_json,
            now,
            now,
        ],
    )?;
}
```

4. **Update pairing flow:**
```
Current:
  Phone → Desktop: "Add me! (provides IP)"
  Desktop: Creates new node entry

New:
  Phone → Desktop: "I'm node-abc-123 at this IP"
  Desktop: Upserts node entry (creates or updates)
```

---

### Phase 2: mDNS Discovery (Later)

Add automatic LAN discovery:
- Nodes advertise via mDNS on startup
- Background task listens for other nodes
- Auto-updates mesh_nodes when discovered
- Makes manual IP entry optional

**Crate:** `mdns-sd` (pure Rust, works on all platforms)

---

## Open Questions

1. **What if node_identity.json is lost?**
   - Node gets new ID → appears as new device
   - Old entry becomes "stale/offline"
   - Manual cleanup needed
   - Alternative: Backup identity in QR code during first pairing?

2. **How to handle manual IP changes?**
   - User moves phone to new network
   - Option A: Re-scan QR code (announces with same ID, updates IP)
   - Option B: Manual "Update IP" in UI
   - Option C: mDNS auto-discovers new IP

3. **Cross-internet mesh?**
   - mDNS doesn't work across internet
   - Need rendezvous server or manual public URL entry
   - Or use Tailscale/Wireguard for virtual LAN

4. **Security between nodes?**
   - If using self-declared IDs, anyone can claim any ID
   - For home network, probably fine (trust your LAN)
   - For internet, need crypto (Option 3)

---

## Next Steps

1. **Implement node_identity.rs** - Load/generate stable ID
2. **Update current node init** - Use loaded ID, not random
3. **Change registerNode → announceNode** - Takes node_id as input
4. **Update pairing flow** - Phone sends its ID
5. **Add heartbeat** - Nodes ping mesh to keep IP updated

Sound good? Want to start with the node identity module?
