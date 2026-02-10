# Dual Pairing Strategy: LAN vs External Access

## The IP Address Problem

**WebAuthn Restriction:** Passkeys require a valid domain as the Relying Party (RP) ID. IP addresses are **not allowed**.

### What This Means

| Access Method | URL Example | Passkeys Work? |
|---------------|-------------|----------------|
| Local IP | `http://192.168.1.100:6969` | ❌ No |
| mDNS/Bonjour | `https://salita.local:6969` | ✅ Yes |
| ngrok | `https://abc123.ngrok.io` | ✅ Yes |
| Public domain | `https://salita.example.com` | ✅ Yes |

## Solution: Dual Pairing Flows

We need **both** authentication methods, chosen based on how the user accesses Salita:

### 1. **LAN Pairing** (IP-based access)
**When:** User navigates via IP address (e.g., `192.168.1.100:6969`)
**Method:** QR code + PIN (existing flow)
**Use case:** Local network devices, no domain needed

### 2. **External Pairing** (domain-based access)
**When:** User navigates via domain (e.g., `salita.local`, ngrok, public domain)
**Method:** Passkey + PRF extension
**Use case:** Remote access, cross-network pairing

## Flow Decision Matrix

```
User accesses Salita
    ↓
Check access URL
    ↓
┌─────────────────────┬──────────────────────┐
│ IP-based?           │ Domain-based?        │
│ (192.168.x.x)       │ (*.local, *.ngrok)   │
│                     │                      │
│ Use LAN Pairing:    │ Use External Pairing:│
│ • QR + PIN          │ • Passkey auth       │
│ • Device tokens     │ • PRF for tokens     │
│ • Local only        │ • Works remotely     │
└─────────────────────┴──────────────────────┘
```

## Architecture: Unified User Model, Dual Auth

### User & Device Groups (Both Flows)

Both pairing methods create the same underlying structure:
- **Users** - Identity (Felix, Alice)
- **Device Groups** - Felix's devices, Alice's devices
- **Ownership** - Private/shared visibility
- **Permissions** - Who can see/manage what

The difference is HOW devices authenticate, not WHO owns them.

### Authentication Methods per Device

```sql
CREATE TABLE device_auth_methods (
    node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    auth_type TEXT NOT NULL,  -- 'pin' or 'passkey'

    -- For PIN-based (LAN)
    session_token TEXT,  -- Short-lived session
    peer_token TEXT,     -- Long-lived API token

    -- For passkey-based (external)
    user_id TEXT REFERENCES users(id),  -- Links to user
    passkey_credential_id TEXT REFERENCES passkey_credentials(id),
    prf_salt BLOB,       -- Salt for PRF derivation

    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

## LAN Pairing Flow (IP-based)

**Current flow, enhanced with user model:**

```
1. Desktop (192.168.1.100): Start pairing
   POST /auth/pair/start
   → Returns: { token, qr_url, expires_at }

2. Phone scans QR → Lands on pairing page
   Shows: "Pair with Felix's Salita" (if Felix logged in)

3. Phone enters PIN
   POST /auth/pair/verify
   {
     token: "abc123",
     pin: "123456",
     device_node_id: "phone-uuid",
     device_name: "Felix's Phone",
     user_id: "felix-uuid"  // NEW: Links to user
   }

4. Server:
   - Verifies PIN
   - Creates device owned by Felix
   - Issues peer tokens
   - Marks auth_type = 'pin'

5. Phone receives:
   {
     session_token: "...",
     peer_token: "...",
     user: { id: "felix-uuid", name: "Felix" }
   }
```

**Key changes:**
- PIN pairing now creates devices owned by current user
- If no user logged in → Create default "Owner" user
- Devices get `auth_type = 'pin'` marker

## External Pairing Flow (domain-based)

**New flow for domain access:**

```
1. Desktop (salita.local): Start pairing
   POST /auth/passkey/pair/start
   Headers: Authorization: Bearer <user-session>

   → Returns: {
       invitation_code: "abc123",
       qr_url: "https://salita.local/pair?invite=abc123",
       expires_at: "..."
     }

2. Phone scans QR → Lands on pairing page
   Shows: "Join Felix's Salita"

3. Phone authenticates with passkey
   GET /auth/passkey/pair/challenge?invite=abc123
   → Returns WebAuthn challenge

   const credential = await navigator.credentials.get({
     publicKey: {
       challenge: challengeFromServer,
       rpId: 'salita.local',
       extensions: {
         prf: {
           eval: { first: saltFromServer }
         }
       }
     }
   });

4. Phone submits assertion
   POST /auth/passkey/pair/verify
   {
     invitation_code: "abc123",
     device_name: "Felix's Phone",
     passkey_assertion: credential,
     prf_output: credential.getClientExtensionResults().prf
   }

5. Server:
   - Verifies passkey assertion
   - Creates device owned by Felix
   - Derives API token from PRF (or generates if unavailable)
   - Marks auth_type = 'passkey'

6. Phone receives:
   {
     session_token: "...",
     api_token: "...",  // Derived from PRF or generated
     user: { id: "felix-uuid", name: "Felix" }
   }
```

**Key features:**
- Passkey authenticates user identity
- PRF derives deterministic API tokens (if supported)
- Fallback to standard token generation if PRF unavailable

## Auto-Detection of Available Methods

### Server-Side Detection

```rust
pub fn get_available_pairing_methods(host: &str) -> Vec<PairingMethod> {
    let mut methods = vec![];

    // PIN pairing always available
    methods.push(PairingMethod::Pin);

    // Passkey only if accessed via domain
    if !is_ip_address(host) {
        methods.push(PairingMethod::Passkey);
    }

    methods
}

fn is_ip_address(host: &str) -> bool {
    // Remove port if present
    let host = host.split(':').next().unwrap_or(host);

    // Check if it's an IPv4 or IPv6 address
    host.parse::<std::net::IpAddr>().is_ok()
}
```

### Client-Side UI

```javascript
// On pairing page
const response = await fetch('/auth/pairing/methods');
const { methods } = await response.json();

if (methods.includes('passkey')) {
    // Show passkey option
    showPasskeyButton();
} else {
    // Show only PIN option
    showPinInput();
    showWarning("Passkeys require domain access (e.g., salita.local)");
}
```

## User Experience

### Scenario 1: Local Network (IP-based)

```
User navigates to: http://192.168.1.100:6969

┌─────────────────────────────────────┐
│ Pair New Device                     │
├─────────────────────────────────────┤
│ Scan this QR code with your device  │
│                                     │
│    ███████████████████              │
│    ██ ▄▄▄▄▄ █▀ █▄ ▄▄▄▄▄ ██          │
│    ██ █   █ █▀▀  █ █   █ ██          │
│    ██ █▄▄▄█ █ ▀█ █ █▄▄▄█ ██          │
│                                     │
│ Then enter the 6-digit PIN shown   │
│                                     │
│ ℹ️ Using PIN pairing (local network)│
└─────────────────────────────────────┘
```

### Scenario 2: Domain Access

```
User navigates to: https://salita.local:6969

┌─────────────────────────────────────┐
│ Pair New Device                     │
├─────────────────────────────────────┤
│ Choose pairing method:              │
│                                     │
│ [🔐 Use Passkey] (Recommended)      │
│   • More secure                     │
│   • Works remotely                  │
│   • No PIN needed                   │
│                                     │
│ [📱 Use PIN] (Local only)           │
│   • Quick for nearby devices        │
│   • Works on any network            │
│                                     │
└─────────────────────────────────────┘
```

### Scenario 3: First-Time Setup

**IP-based first setup:**
```
1. Navigate to http://192.168.1.100:6969
2. "Welcome to Salita" screen
3. Enter your name: [Felix________]
4. Creates user "Felix" via PIN flow
5. This device becomes "Felix's Laptop"
```

**Domain-based first setup:**
```
1. Navigate to https://salita.local:6969
2. "Welcome to Salita" screen
3. Enter your name: [Felix________]
4. [Set up with Passkey] button
5. Passkey registration → Creates user + device
```

## Database Schema Updates

```sql
-- migrations/009_dual_pairing_support.sql

-- Enhanced users table (same for both flows)
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    display_name TEXT NOT NULL,
    created_via TEXT NOT NULL,  -- 'pin' or 'passkey'
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Device authentication tracking
CREATE TABLE device_auth_methods (
    node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    auth_type TEXT NOT NULL CHECK(auth_type IN ('pin', 'passkey')),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- PIN-based auth fields
    session_token TEXT,
    peer_token TEXT,

    -- Passkey-based auth fields
    passkey_credential_id TEXT REFERENCES passkey_credentials(id),
    prf_salt BLOB,

    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_auth_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Track which method was used for each pairing
CREATE TABLE pairing_history (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id),
    pairing_method TEXT NOT NULL,  -- 'pin' or 'passkey'
    access_url TEXT NOT NULL,      -- How they accessed Salita
    paired_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_pairing_history_user ON pairing_history(user_id);
CREATE INDEX idx_pairing_history_method ON pairing_history(pairing_method);
```

## API Endpoints

### Common Endpoints

```rust
// Get available pairing methods for current access
GET /auth/pairing/methods
→ { methods: ["pin", "passkey"] }

// Get current user (from session)
GET /auth/me
→ { id, username, display_name, devices: [...] }
```

### PIN Pairing (Enhanced)

```rust
// Existing endpoints, enhanced with user context
POST /auth/pair/start
POST /auth/pair/connect
POST /auth/pair/verify  // Now accepts user_id
GET /auth/pair/status
```

### Passkey Pairing (New)

```rust
// Create invitation for passkey pairing
POST /auth/passkey/pair/invite
Headers: Authorization: Bearer <user-session>
→ { invitation_code, qr_url, expires_at }

// Get challenge for passkey authentication
GET /auth/passkey/pair/challenge?invite=<code>
→ { challenge, rp, user, extensions }

// Complete pairing with passkey
POST /auth/passkey/pair/verify
{
    invitation_code,
    device_name,
    passkey_assertion,
    prf_output
}
→ { session_token, api_token, user }
```

## Implementation Strategy

### Phase 1: Foundation (User Model)
- ✅ Implement user schema
- ✅ Add user ownership to mesh_nodes
- ✅ Create default user for existing devices
- ✅ Update PIN pairing to link devices to users

### Phase 2: Method Detection
- ✅ Add `/auth/pairing/methods` endpoint
- ✅ Detect IP vs domain access
- ✅ Update UI to show available methods

### Phase 3: Passkey Pairing (Domain-based)
- ✅ Implement passkey invitation flow
- ✅ Add WebAuthn challenge/verify endpoints
- ✅ Integrate PRF extension (opportunistic)
- ✅ Create devices via passkey auth

### Phase 4: Unified Dashboard
- ✅ Show all devices regardless of auth method
- ✅ Indicate auth method per device
- ✅ Support both flows in "Add Device"

### Phase 5: Advanced Features
- ✅ Allow upgrading PIN devices to passkey
- ✅ Support multiple auth methods per device
- ✅ Device-to-device trust chains

## Edge Cases & Considerations

### Mixed Access Patterns

**Problem:** User starts with IP, later gets domain
```
Day 1: Setup via 192.168.1.100 (PIN-based)
Day 2: Access via salita.local (domain available)
```

**Solution:** Allow "upgrading" to passkey auth
```
Settings → My Devices → Felix's Laptop
[🔓 Add Passkey Authentication]
```

### Lost Passkey Authenticator

**Problem:** User loses device with passkey
**Solution:**
1. Keep PIN-based pairing as backup
2. Allow account recovery via existing device
3. Support multiple passkeys per user

### PRF Unavailable

**Problem:** Browser/device doesn't support PRF
**Solution:**
1. Detect PRF support during challenge
2. Fall back to standard token generation
3. Still use passkey for authentication

### IP Changes on LAN

**Problem:** Device IP changes, breaks PIN-based tokens
**Solution:**
1. Tokens are node_id-based, not IP-based
2. IP is just for display, not authentication
3. Works fine as long as mesh is reachable

## Testing Strategy

### Unit Tests
- ✅ IP address detection
- ✅ Method availability logic
- ✅ User creation via both flows
- ✅ Device ownership assignment

### Integration Tests
- ✅ PIN pairing creates user-owned device
- ✅ Passkey pairing creates user-owned device
- ✅ Both flows produce same device structure
- ✅ Cross-method device visibility

### E2E Tests
- ✅ Complete PIN pairing flow
- ✅ Complete passkey pairing flow
- ✅ Mixed devices (some PIN, some passkey)
- ✅ Method detection UI

### Manual Tests
- ✅ Test via IP address (192.168.1.100)
- ✅ Test via mDNS (salita.local)
- ✅ Test via ngrok (abc123.ngrok.io)
- ✅ Test PRF support (Android, macOS)

## Migration from Current State

```rust
// Migrate existing PIN-paired devices
pub async fn migrate_to_user_model(conn: &Connection) -> Result<()> {
    // 1. Create default user
    let default_user = User::create("owner", "Owner", "pin")?;

    // 2. Assign all nodes to default user
    conn.execute(
        "UPDATE mesh_nodes
         SET owned_by = ?1, visibility = 'private'
         WHERE owned_by IS NULL",
        params![default_user.id]
    )?;

    // 3. Mark all as PIN-based auth
    conn.execute(
        "INSERT INTO device_auth_methods (node_id, auth_type, user_id)
         SELECT id, 'pin', ?1 FROM mesh_nodes",
        params![default_user.id]
    )?;

    Ok(())
}
```

## Success Criteria

✅ **Functionality:**
- PIN pairing works on IP-based access
- Passkey pairing works on domain-based access
- Both create user-owned devices
- Both flows integrated into unified dashboard

✅ **User Experience:**
- Auto-detection of available methods
- Clear explanation of which method to use
- Smooth onboarding for both flows
- No confusion about why passkeys don't work on IP

✅ **Security:**
- Users can't see each other's private devices
- Passkey provides stronger auth than PIN
- PRF enables stateless token derivation
- Both methods enforce same permissions

✅ **Compatibility:**
- Works with IP addresses (PIN)
- Works with mDNS .local domains (passkey)
- Works with ngrok/public domains (passkey)
- Graceful degradation when PRF unavailable

## Recommendation

**Default Strategy:**
1. **Local network (IP)**: Use PIN pairing (existing flow + user model)
2. **External access (domain)**: Use passkey pairing (new flow)
3. **Auto-detect** which methods are available
4. **Unified user model** for both flows

This gives us:
- ✅ Best UX for local network (quick PIN pairing)
- ✅ Best security for external (passkey + PRF)
- ✅ Consistent user/device model
- ✅ No breaking changes to existing PIN flow
