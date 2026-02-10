# Device Identity & Merging Strategy

## The Problem: Multiple Identities for Same Device

### Scenario 1: Same Device, Different Access Methods
```
Felix's Phone:
  Day 1: Pairs via LAN (192.168.1.100) → node_id: abc123, auth: PIN
  Day 2: Pairs via ngrok (felix.ngrok.io) → node_id: xyz789, auth: Passkey

Result: Same physical device appears as TWO devices! ❌
```

### Scenario 2: Device Group Membership
```
Question: How do we know these devices belong to Felix?
  • Felix's Laptop (paired via LAN with PIN)
  • Felix's Phone (paired via passkey)
  • Felix's Tablet (paired via PIN)

Need: Explicit user ownership before pairing ✅
```

## Solution: User Login + Device Fingerprinting

### Core Principle

**Every pairing must be authenticated by a logged-in user.**

```
Old flow:
Device → Pair → Get token → No user context ❌

New flow:
User logs in → Device pairs → Linked to user → Device group ✅
```

## Architecture: User Session Required for Pairing

### 1. Initial Setup Flow (First User + First Device)

**Scenario:** Brand new Salita instance, no users exist

```
1. Navigate to Salita (IP or domain)
   ↓
2. "Welcome to Salita" screen
   ↓
3. Create account:
     [Display Name: Felix________]
     [Username: felix___________]
   ↓
4. Choose auth method:
     Domain access → Register passkey
     IP access → Set password (or skip)
   ↓
5. User created + Auto-registers this device as "Felix's [Device Name]"
   ↓
6. Logged in to dashboard
```

**Result:**
- User: Felix (id: felix-uuid)
- Device: Felix's Laptop (node_id: laptop-uuid, owned_by: felix-uuid)
- Session: Logged in as Felix

### 2. Adding Device to Existing User

**Scenario:** Felix wants to add his phone to his device group

#### Option A: User Already Logged In (on another device)

```
On Laptop (logged in as Felix):
  1. Dashboard → [Add Device]
  2. Generate invitation:
       POST /auth/devices/invite
       Headers: Authorization: Bearer <felix-session>
       → { invite_code: "abc123", qr_url }

On Phone:
  3. Scan QR → https://salita.local/pair?invite=abc123
  4. Pairing page shows:
       "Join Felix's Salita"
       "This will add your device to Felix's group"
  5. Choose method (PIN or Passkey)
  6. Complete pairing
  7. Device auto-linked to Felix's group
```

#### Option B: User Not Logged In

```
On Phone:
  1. Navigate to Salita
  2. "Login or Create Account" screen
  3. Login as Felix:
       - Passkey auth (if domain)
       - Username + password (if IP)
  4. After login → Dashboard → [Add This Device]
  5. Device auto-registers to Felix's group
```

### 3. Device Fingerprinting

**Goal:** Detect if the same physical device connects via different methods

#### Browser Fingerprint

```javascript
// Generate stable device fingerprint
function generateDeviceFingerprint() {
    const components = [
        navigator.userAgent,
        navigator.platform,
        navigator.language,
        screen.width + 'x' + screen.height,
        new Date().getTimezoneOffset(),
        navigator.hardwareConcurrency || 'unknown',
        // NOT using IP (changes on network switch)
    ];

    // Hash to create stable ID
    const fingerprint = await sha256(components.join('|'));

    // Store in localStorage (persists across sessions)
    const existingId = localStorage.getItem('salita_device_id');
    const deviceId = existingId || fingerprint;
    localStorage.setItem('salita_device_id', deviceId);

    return deviceId;
}
```

#### Pairing with Fingerprint

```javascript
// When pairing, include device fingerprint
const deviceFingerprint = await generateDeviceFingerprint();

await fetch('/auth/pair/verify', {
    method: 'POST',
    body: JSON.stringify({
        token: pairingToken,
        pin: enteredPin,
        device_name: "Felix's Phone",
        device_fingerprint: deviceFingerprint,  // NEW
        user_id: currentUserId
    })
});
```

#### Server-Side Duplicate Detection

```rust
pub async fn register_device(
    conn: &Connection,
    user_id: &str,
    node_id: &str,
    device_fingerprint: &str,
    device_name: &str,
) -> Result<RegisterResult> {
    // Check if this fingerprint already exists for this user
    let existing_device: Option<String> = conn.query_row(
        "SELECT node_id FROM device_fingerprints
         WHERE user_id = ?1 AND fingerprint = ?2",
        params![user_id, device_fingerprint],
        |row| row.get(0)
    ).ok();

    if let Some(existing_node_id) = existing_device {
        // Same device reconnecting with different auth method
        return merge_device_auth_methods(
            conn,
            &existing_node_id,
            node_id,
            device_fingerprint
        );
    }

    // New device - create normally
    create_new_device(conn, user_id, node_id, device_fingerprint, device_name)
}
```

## Database Schema

```sql
-- migrations/010_device_identity_merging.sql

-- Store device fingerprints
CREATE TABLE device_fingerprints (
    fingerprint TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    first_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (fingerprint, user_id)
);

CREATE INDEX idx_device_fingerprints_node ON device_fingerprints(node_id);

-- Track multiple auth methods per device
CREATE TABLE device_auth_credentials (
    id TEXT PRIMARY KEY,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    auth_type TEXT NOT NULL CHECK(auth_type IN ('pin', 'passkey', 'password')),

    -- For passkey
    passkey_credential_id TEXT REFERENCES passkey_credentials(id),

    -- For PIN-based
    peer_token TEXT,

    -- For password
    password_hash TEXT,

    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_device_auth_node ON device_auth_credentials(node_id);
CREATE INDEX idx_device_auth_type ON device_auth_credentials(auth_type);

-- User login methods
CREATE TABLE user_login_methods (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    login_type TEXT NOT NULL CHECK(login_type IN ('passkey', 'password')),

    -- For passkey
    passkey_credential_id TEXT REFERENCES passkey_credentials(id),

    -- For password
    password_hash TEXT,

    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);

CREATE INDEX idx_user_login_methods_user ON user_login_methods(user_id);
CREATE INDEX idx_user_login_methods_type ON user_login_methods(login_type);
```

## Device Merging Logic

### When to Merge

```rust
pub enum MergeDecision {
    CreateNew,           // Different fingerprint, create new device
    UpdateExisting,      // Same fingerprint, update existing device
    MergeAuthMethods,    // Same fingerprint, add new auth method
}

pub fn should_merge_device(
    conn: &Connection,
    user_id: &str,
    device_fingerprint: &str,
    proposed_node_id: &str,
) -> Result<MergeDecision> {
    // Check for existing device with this fingerprint
    let existing = conn.query_row(
        "SELECT node_id FROM device_fingerprints
         WHERE user_id = ?1 AND fingerprint = ?2",
        params![user_id, device_fingerprint],
        |row| row.get::<_, String>(0)
    );

    match existing {
        Ok(existing_node_id) if existing_node_id == proposed_node_id => {
            // Same node_id, update last_seen
            Ok(MergeDecision::UpdateExisting)
        }
        Ok(existing_node_id) => {
            // Different node_id, same fingerprint → MERGE
            Ok(MergeDecision::MergeAuthMethods)
        }
        Err(_) => {
            // No existing device with this fingerprint
            Ok(MergeDecision::CreateNew)
        }
    }
}
```

### Merge Process

```rust
pub async fn merge_device_auth_methods(
    conn: &Connection,
    primary_node_id: &str,    // Keep this one
    secondary_node_id: &str,  // Merge into primary
    fingerprint: &str,
) -> Result<()> {
    // Start transaction
    conn.execute("BEGIN IMMEDIATE", [])?;

    // 1. Copy auth credentials from secondary to primary
    conn.execute(
        "INSERT INTO device_auth_credentials (id, node_id, auth_type, passkey_credential_id, peer_token)
         SELECT id, ?1, auth_type, passkey_credential_id, peer_token
         FROM device_auth_credentials
         WHERE node_id = ?2",
        params![primary_node_id, secondary_node_id]
    )?;

    // 2. Update tokens to point to primary node
    conn.execute(
        "UPDATE issued_tokens SET issued_to_node_id = ?1 WHERE issued_to_node_id = ?2",
        params![primary_node_id, secondary_node_id]
    )?;

    conn.execute(
        "UPDATE peer_tokens SET peer_node_id = ?1 WHERE peer_node_id = ?2",
        params![primary_node_id, secondary_node_id]
    )?;

    // 3. Delete secondary node (cascades to auth_credentials)
    conn.execute(
        "DELETE FROM mesh_nodes WHERE id = ?1",
        params![secondary_node_id]
    )?;

    // 4. Update fingerprint to point to primary
    conn.execute(
        "UPDATE device_fingerprints SET node_id = ?1, last_seen_at = datetime('now')
         WHERE fingerprint = ?2",
        params![primary_node_id, fingerprint]
    )?;

    // Commit
    conn.execute("COMMIT", [])?;

    Ok(())
}
```

## User Login Flows

### Login via Passkey (Domain Access)

```rust
// POST /auth/login/passkey/challenge
pub async fn passkey_login_challenge(
    State(state): State<AppState>,
) -> AppResult<Response> {
    let webauthn = &state.webauthn;

    // Get all registered passkeys
    let passkeys = get_all_passkeys(&state.db)?;

    let (challenge, auth_state) = webauthn.start_passkey_authentication(&passkeys)?;

    // Store challenge in session
    state.ceremonies.lock().await
        .insert_authentication(challenge.clone(), auth_state);

    Ok(Json(challenge).into_response())
}

// POST /auth/login/passkey/verify
pub async fn passkey_login_verify(
    State(state): State<AppState>,
    Json(auth): Json<PublicKeyCredential>,
) -> AppResult<Response> {
    let webauthn = &state.webauthn;

    // Get challenge from session
    let auth_state = state.ceremonies.lock().await
        .take_authentication(&auth.id)
        .ok_or(AppError::BadRequest("Invalid challenge"))?;

    // Verify assertion
    let auth_result = webauthn.finish_passkey_authentication(&auth, &auth_state)?;

    // Get user_id from passkey credential
    let user_id = get_user_by_passkey_credential(&state.db, &auth_result.cred_id())?;

    // Create session
    let session_token = create_user_session(&state.db, &user_id)?;

    Ok(Json(LoginResponse {
        session_token,
        user_id,
    }).into_response())
}
```

### Login via Password (IP Access)

```rust
// POST /auth/login/password
pub async fn password_login(
    State(state): State<AppState>,
    Json(req): Json<PasswordLoginRequest>,
) -> AppResult<Response> {
    let conn = state.db.get()?;

    // Look up user by username
    let (user_id, password_hash): (String, String) = conn.query_row(
        "SELECT u.id, ulm.password_hash
         FROM users u
         JOIN user_login_methods ulm ON u.id = ulm.user_id
         WHERE u.username = ?1 AND ulm.login_type = 'password'",
        params![req.username],
        |row| Ok((row.get(0)?, row.get(1)?))
    ).map_err(|_| AppError::Unauthorized("Invalid credentials"))?;

    // Verify password
    if !bcrypt::verify(&req.password, &password_hash)? {
        return Err(AppError::Unauthorized("Invalid credentials"));
    }

    // Create session
    let session_token = create_user_session(&conn, &user_id)?;

    Ok(Json(LoginResponse {
        session_token,
        user_id,
    }).into_response())
}
```

## Complete Pairing Flow with User Context

### Invitation-Based Pairing (User Logged In)

```
Desktop (Felix logged in):
  1. POST /auth/devices/invite
     Headers: Authorization: Bearer <felix-session>
     → { invite_code: "abc123", expires_at, qr_url }

Phone:
  2. Scan QR → /pair?invite=abc123
  3. GET /auth/invitations/abc123
     → { user: { username: "felix", display_name: "Felix" } }
  4. UI shows: "Join Felix's Salita"
  5. Choose pairing method (PIN or Passkey)

  If PIN:
    6. POST /auth/pair/connect { invite_code, device_ip }
       → { pin }
    7. POST /auth/pair/verify { invite_code, pin, device_fingerprint, device_name }
       → { session_token, node_id }

  If Passkey:
    6. GET /auth/passkey/pair/challenge?invite=abc123
       → { challenge, user }
    7. Create/use passkey
    8. POST /auth/passkey/pair/verify { invite_code, assertion, device_fingerprint, device_name }
       → { session_token, node_id }

  9. Device registered to Felix's group
  10. Logged in as Felix on phone
```

### Direct Pairing (User Logs In First)

```
Phone:
  1. Navigate to Salita
  2. Login screen (passkey or password)
  3. Login as Felix
  4. Dashboard → [Add This Device]
  5. POST /auth/devices/register
     Headers: Authorization: Bearer <felix-session>
     Body: { device_name: "Felix's Phone", device_fingerprint }
     → { node_id, success }
  6. Device auto-added to Felix's group
```

## Device Group Queries

```rust
// Get all devices for current user
pub async fn list_my_devices(
    State(state): State<AppState>,
    Extension(user): Extension<User>,  // From auth middleware
) -> AppResult<Response> {
    let conn = state.db.get()?;

    let devices = conn.prepare(
        "SELECT mn.id, mn.name, mn.hostname, mn.port, mn.status, mn.visibility,
                df.fingerprint, mn.last_seen
         FROM mesh_nodes mn
         LEFT JOIN device_fingerprints df ON mn.id = df.node_id
         WHERE mn.owned_by = ?1
         ORDER BY mn.name"
    )?
    .query_map(params![user.id], |row| {
        Ok(DeviceInfo {
            node_id: row.get(0)?,
            name: row.get(1)?,
            hostname: row.get(2)?,
            port: row.get(3)?,
            status: row.get(4)?,
            visibility: row.get(5)?,
            fingerprint: row.get(6)?,
            last_seen: row.get(7)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(devices).into_response())
}

// Get auth methods for a device
pub async fn get_device_auth_methods(
    conn: &Connection,
    node_id: &str,
) -> Result<Vec<AuthMethod>> {
    conn.prepare(
        "SELECT auth_type, created_at, last_used_at
         FROM device_auth_credentials
         WHERE node_id = ?1
         ORDER BY created_at"
    )?
    .query_map(params![node_id], |row| {
        Ok(AuthMethod {
            auth_type: row.get(0)?,
            created_at: row.get(1)?,
            last_used_at: row.get(2)?,
        })
    })?
    .collect()
}
```

## UI Updates

### Dashboard: Device List with Auth Methods

```html
<div class="device-list">
  <h2>My Devices</h2>

  <div class="device-card">
    <h3>Felix's Phone</h3>
    <p>Status: <span class="status-online">Online</span></p>
    <p>Location: 192.168.1.100</p>

    <div class="auth-methods">
      <span class="badge badge-pin">PIN</span>
      <span class="badge badge-passkey">Passkey</span>
    </div>

    <button>Manage</button>
  </div>

  <div class="device-card">
    <h3>Felix's Laptop</h3>
    <p>Status: <span class="status-online">Online</span></p>
    <p>Location: This device</p>

    <div class="auth-methods">
      <span class="badge badge-passkey">Passkey</span>
    </div>

    <button>Manage</button>
  </div>

  <button class="add-device">+ Add Device</button>
</div>
```

### Device Management Modal

```html
<div class="device-modal">
  <h2>Felix's Phone</h2>

  <section>
    <h3>Authentication Methods</h3>
    <ul>
      <li>
        🔐 Passkey (added Feb 9, 2026)
        <button>Remove</button>
      </li>
      <li>
        📱 PIN-based token (added Feb 8, 2026)
        <button>Remove</button>
      </li>
    </ul>
    <button>+ Add Authentication Method</button>
  </section>

  <section>
    <h3>Settings</h3>
    <label>
      Device Name:
      <input type="text" value="Felix's Phone">
    </label>
    <label>
      Visibility:
      <select>
        <option value="private">Private (only me)</option>
        <option value="shared">Shared (visible to all)</option>
      </select>
    </label>
  </section>

  <button class="danger">Remove Device</button>
</div>
```

## Migration Strategy

```rust
// Migrate existing anonymous devices to default user
pub async fn migrate_to_user_ownership(conn: &Connection) -> Result<()> {
    // 1. Create "Owner" user if not exists
    let owner_id = ensure_default_user(conn)?;

    // 2. Assign all orphaned devices to Owner
    conn.execute(
        "UPDATE mesh_nodes
         SET owned_by = ?1
         WHERE owned_by IS NULL",
        params![owner_id]
    )?;

    // 3. Create default password login for Owner (IP access)
    let default_password_hash = bcrypt::hash("admin", bcrypt::DEFAULT_COST)?;
    conn.execute(
        "INSERT OR IGNORE INTO user_login_methods (id, user_id, login_type, password_hash)
         VALUES (?1, ?2, 'password', ?3)",
        params![uuid::Uuid::now_v7().to_string(), owner_id, default_password_hash]
    )?;

    Ok(())
}
```

## Summary

**Device Group Membership:**
- ✅ User login required before pairing
- ✅ Invitation codes link device to user
- ✅ All devices explicitly owned by a user

**Device Merging:**
- ✅ Browser fingerprinting detects same device
- ✅ Automatic merge when fingerprint matches
- ✅ Multiple auth methods per device supported
- ✅ Transparent to user

**Authentication:**
- ✅ Passkey login (domain access)
- ✅ Password login (IP access)
- ✅ Session-based after login
- ✅ All pairing tied to authenticated user

**User Experience:**
- ✅ Clear device groups in dashboard
- ✅ Auth method badges on devices
- ✅ Seamless merging (no duplicates)
- ✅ Flexible authentication options
