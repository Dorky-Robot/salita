# User Identity & Device Groups Architecture

## Problem Statement

Salita currently has **nodes** but no concept of **users** or **ownership**. This creates problems:

❌ No way to distinguish "Felix's phone" from "Alice's phone"
❌ Everyone can see and manage all devices
❌ No privacy between different people using the same Salita instance
❌ Can't have "family shared" vs "personal private" devices

## Solution: User-Centric Device Groups

### Core Concepts

1. **User** - A person (Felix, Alice, etc.)
   - Created during first passkey registration
   - Owns multiple devices
   - Has privacy boundaries

2. **Device Group** - All devices belonging to one user
   - Felix's group: Felix's phone, Felix's laptop, Felix's tablet
   - Alice's group: Alice's phone, Alice's laptop
   - Devices in same group can see/manage each other

3. **Device** - A physical node (phone, laptop, etc.)
   - Belongs to exactly one user
   - Has one or more passkeys (for different authenticators)
   - Can have visibility settings (private/shared)

### Ownership Model

```
┌─────────────────────────────────────────────────┐
│ Salita Instance (felix-home.local)              │
│                                                  │
│  ┌──────────────────────┐  ┌─────────────────┐ │
│  │ User: Felix          │  │ User: Alice     │ │
│  │                      │  │                 │ │
│  │  Devices:            │  │  Devices:       │ │
│  │  • Felix's Phone     │  │  • Alice's Phone│ │
│  │  • Felix's Laptop    │  │  • Alice's iPad │ │
│  │  • Felix's Tablet    │  │                 │ │
│  └──────────────────────┘  └─────────────────┘ │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │ Shared Devices (visible to all)         │   │
│  │  • Family iPad (owned by Felix)          │   │
│  │  • Living Room Display (owned by Alice)  │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Passkey Onboarding Flow (Inspired by Katulong)

### First Device Registration (Creates User)

**Scenario:** Felix sets up Salita for the first time on his laptop

```
1. Navigate to https://salita.example.com
   ↓
2. Click "Set up Salita" (no existing users)
   ↓
3. Enter display name: "Felix"
   ↓
4. Register passkey → Creates:
      • User: Felix (id: uuid-123)
      • Device: Felix's Laptop (node_id: uuid-456, owned_by: uuid-123)
      • Passkey credential linked to user
   ↓
5. Logged in to Salita dashboard
```

### Second Device (Same User)

**Scenario:** Felix wants to add his phone to his device group

```
1. On laptop dashboard, click "Add Device"
   ↓
2. Show QR code with invitation link:
   https://salita.example.com/join?invite=abc123&user=uuid-123
   ↓
3. Phone scans QR → Lands on pairing page
   ↓
4. Phone sees: "Join Felix's Salita"
   ↓
5. Click "Continue as Felix" → Register passkey
   ↓
6. Creates:
      • Device: Felix's Phone (node_id: uuid-789, owned_by: uuid-123)
      • Passkey credential linked to Felix's user
   ↓
7. Phone now in Felix's device group
```

### Different User (New Device Group)

**Scenario:** Alice wants her own devices on the same Salita instance

```
1. Navigate to https://salita.example.com
   ↓
2. Click "Join Salita" (existing users exist)
   ↓
3. Options:
      [Join as existing user]  [Create new account]
   ↓
4. Choose "Create new account"
   ↓
5. Enter display name: "Alice"
   ↓
6. Register passkey → Creates:
      • User: Alice (id: uuid-999)
      • Device: Alice's Phone (node_id: uuid-888, owned_by: uuid-999)
      • Passkey credential linked to Alice
   ↓
7. Alice's dashboard (can only see her devices)
```

## Database Schema

### Updated Users Table

```sql
-- migrations/009_user_device_groups.sql

-- Drop existing minimal users table
DROP TABLE IF EXISTS users;

-- Recreate with full user identity support
CREATE TABLE users (
    id TEXT PRIMARY KEY,                    -- UUID v7
    username TEXT UNIQUE NOT NULL,          -- Unique username (e.g., "felix")
    display_name TEXT NOT NULL,             -- Display name (e.g., "Felix")
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Add owner to mesh_nodes
ALTER TABLE mesh_nodes ADD COLUMN owned_by TEXT REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE mesh_nodes ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'; -- 'private' or 'shared'

CREATE INDEX idx_mesh_nodes_owner ON mesh_nodes(owned_by);

-- Link passkey credentials to users (not devices)
ALTER TABLE passkey_credentials DROP COLUMN user_id; -- Old reference
ALTER TABLE passkey_credentials ADD COLUMN user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE;

-- Invitation codes for adding devices
CREATE TABLE device_invitations (
    code TEXT PRIMARY KEY,                  -- Short code for QR/link
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by_node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL,
    used_at TEXT,
    used_by_node_id TEXT REFERENCES mesh_nodes(id)
);

CREATE INDEX idx_device_invitations_user ON device_invitations(user_id);
CREATE INDEX idx_device_invitations_expires ON device_invitations(expires_at);

-- Device naming (user-friendly names)
CREATE TABLE device_names (
    node_id TEXT PRIMARY KEY REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    custom_name TEXT,                       -- User can override (e.g., "Work Laptop")
    auto_name TEXT NOT NULL,                -- Auto-generated (e.g., "MacBook Pro")
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

## User Sessions & Authentication

### Session Model

```sql
-- User sessions (browser sessions, not device sessions)
CREATE TABLE user_sessions (
    session_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    last_used_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_user_sessions_user ON user_sessions(user_id);
CREATE INDEX idx_user_sessions_node ON user_sessions(node_id);
CREATE INDEX idx_user_sessions_expires ON user_sessions(expires_at);
```

### Authentication Flow

1. **Passkey authentication** → Verifies user identity
2. **Creates user session** → Browser cookie/token
3. **Logs which device** → Tracks which node the user is on
4. **Session expiry** → 30 days, sliding window

## Privacy & Permissions

### Visibility Rules

| Scenario | Felix's Laptop | Felix's Phone | Alice's Phone |
|----------|---------------|---------------|---------------|
| Felix logged in on laptop | ✅ Manage | ✅ Manage | ❌ Hidden |
| Alice logged in on phone | ❌ Hidden | ❌ Hidden | ✅ Manage |
| Shared device (Family iPad) | ✅ View | ✅ View | ✅ View |

### Implementation

```rust
// Check if user can see device
pub fn can_view_device(user_id: &str, node_id: &str, conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM mesh_nodes
         WHERE id = ?1
         AND (owned_by = ?2 OR visibility = 'shared')",
        params![node_id, user_id],
        |_| Ok(true)
    ).unwrap_or(false)
}

// Check if user can manage device
pub fn can_manage_device(user_id: &str, node_id: &str, conn: &Connection) -> bool {
    conn.query_row(
        "SELECT 1 FROM mesh_nodes
         WHERE id = ?1 AND owned_by = ?2",
        params![node_id, user_id],
        |_| Ok(true)
    ).unwrap_or(false)
}
```

## API Changes

### New Endpoints

```rust
// Create new user (first-time setup)
POST /auth/users/create
{
    "display_name": "Felix",
    "device_name": "Felix's Laptop"
}
→ { user_id, node_id, session_token }

// Add device to existing user
POST /auth/devices/add
Headers: Authorization: Bearer <session_token>
{
    "invitation_code": "abc123",
    "device_name": "Felix's Phone"
}
→ { node_id, success }

// Generate invitation code
POST /auth/invitations/create
Headers: Authorization: Bearer <session_token>
→ { code, qr_url, expires_at }

// List my devices
GET /auth/devices
Headers: Authorization: Bearer <session_token>
→ [{ node_id, name, visibility, status, last_seen }]

// Update device visibility
PATCH /auth/devices/:node_id
Headers: Authorization: Bearer <session_token>
{
    "visibility": "shared",  // or "private"
    "custom_name": "Work Laptop"
}
→ { success }
```

### Updated GraphQL

```graphql
type User {
    id: ID!
    username: String!
    displayName: String!
    devices: [Device!]!
    createdAt: DateTime!
}

type Device {
    id: ID!
    name: String!
    owner: User!
    visibility: DeviceVisibility!
    status: NodeStatus!
    lastSeen: DateTime!
}

enum DeviceVisibility {
    PRIVATE
    SHARED
}

# Queries
query MyDevices {
    myDevices {
        id
        name
        visibility
        status
    }
}

query SharedDevices {
    sharedDevices {
        id
        name
        owner {
            displayName
        }
    }
}

# Mutations
mutation UpdateDeviceVisibility($nodeId: ID!, $visibility: DeviceVisibility!) {
    updateDeviceVisibility(nodeId: $nodeId, visibility: $visibility) {
        success
        device {
            id
            visibility
        }
    }
}
```

## UI/UX Changes

### Dashboard Views

#### Felix's View (logged in)
```
┌─────────────────────────────────────┐
│ Salita - Felix's Devices            │
├─────────────────────────────────────┤
│ My Devices                          │
│ • Felix's Laptop      [Online]  🔒  │
│ • Felix's Phone       [Online]  🔒  │
│ • Felix's Tablet      [Offline] 🔒  │
│                                     │
│ Shared Devices                      │
│ • Family iPad         [Online]  🌐  │
│   (owned by Felix)                  │
│                                     │
│ [+ Add Device]                      │
└─────────────────────────────────────┘
```

#### Alice's View (logged in)
```
┌─────────────────────────────────────┐
│ Salita - Alice's Devices            │
├─────────────────────────────────────┤
│ My Devices                          │
│ • Alice's Phone       [Online]  🔒  │
│ • Alice's iPad        [Online]  🔒  │
│                                     │
│ Shared Devices                      │
│ • Family iPad         [Online]  🌐  │
│   (owned by Felix)                  │
│                                     │
│ [+ Add Device]                      │
└─────────────────────────────────────┘
```

### Device Management

```
┌──────────────────────────────────────────┐
│ Device: Felix's Laptop                   │
├──────────────────────────────────────────┤
│ Name: [Felix's Laptop____________]       │
│                                          │
│ Visibility:                              │
│ ○ Private (only you can see)             │
│ ○ Shared (visible to everyone)           │
│                                          │
│ Status: Online                           │
│ Last seen: Just now                      │
│ IP: 192.168.1.100                        │
│                                          │
│ [Remove Device]                          │
└──────────────────────────────────────────┘
```

## Migration Strategy

### For Existing Salita Instances

```rust
// Migration helper: Assign existing devices to default user
pub fn migrate_existing_nodes() {
    // 1. Create default user "Owner"
    let default_user_id = create_user("owner", "Owner");

    // 2. Assign all existing nodes to this user
    conn.execute(
        "UPDATE mesh_nodes SET owned_by = ?1 WHERE owned_by IS NULL",
        params![default_user_id]
    )?;

    // 3. Make all devices private by default
    conn.execute(
        "UPDATE mesh_nodes SET visibility = 'private' WHERE visibility IS NULL",
        []
    )?;
}
```

### Backward Compatibility

- Existing PIN-based pairing still works → Creates devices under "Owner" user
- Gradual migration: Add passkey auth alongside existing auth
- Default user created for single-user instances

## Implementation Phases

### Phase 1: User Model Foundation
- ✅ Update database schema (users, device ownership)
- ✅ Create user CRUD operations
- ✅ Add migration for existing devices

### Phase 2: Passkey User Auth
- ✅ User registration via passkey
- ✅ User authentication via passkey
- ✅ Session management
- ✅ Link passkeys to users (not devices)

### Phase 3: Device Groups
- ✅ Invitation codes for adding devices
- ✅ Device ownership enforcement
- ✅ Visibility controls (private/shared)

### Phase 4: UI Updates
- ✅ Login screen (passkey auth)
- ✅ Device list (filtered by user)
- ✅ Device management (rename, visibility)
- ✅ Add device flow (QR code invitation)

### Phase 5: Multi-User Polish
- ✅ User switching (multiple accounts on one device)
- ✅ Shared device indicators
- ✅ Permissions enforcement in GraphQL
- ✅ Admin user concept (optional)

## Security Considerations

**User Isolation:**
- ✅ API endpoints check user_id from session
- ✅ Database queries filter by owned_by
- ✅ GraphQL resolvers enforce permissions

**Shared Devices:**
- ✅ Owner can change visibility
- ✅ Shared = visible but not manageable by others
- ✅ Clear ownership indicator in UI

**Session Security:**
- ✅ HttpOnly cookies
- ✅ CSRF protection
- ✅ Session expiry (30 days)
- ✅ Device fingerprinting (optional)

## Examples

### Felix Adding His Phone

```javascript
// 1. On laptop, generate invitation
const invite = await fetch('/auth/invitations/create', {
    method: 'POST',
    headers: { 'Authorization': `Bearer ${sessionToken}` }
}).then(r => r.json());

// Shows QR: https://salita.local/join?invite=abc123&user=felix-uuid

// 2. Phone scans QR, lands on /join page
// Page shows: "Join Felix's Salita"

// 3. Phone clicks "Continue as Felix"
const assertion = await navigator.credentials.get({
    publicKey: {
        challenge: challengeFromServer,
        rpId: 'salita.local'
    }
});

// 4. Server verifies passkey, creates device
await fetch('/auth/devices/add', {
    method: 'POST',
    body: JSON.stringify({
        invitation_code: 'abc123',
        device_name: "Felix's Phone",
        passkey_assertion: assertion
    })
});

// 5. Phone now belongs to Felix's device group
```

### Alice Creating Her Account

```javascript
// 1. Navigate to salita.local
// 2. Click "Create Account"
// 3. Enter display name: "Alice"

const credential = await navigator.credentials.create({
    publicKey: {
        challenge: challengeFromServer,
        rp: { name: 'Salita', id: 'salita.local' },
        user: {
            id: new Uint8Array(16), // Server generates
            name: 'alice',
            displayName: 'Alice'
        },
        pubKeyCredParams: [{ type: 'public-key', alg: -7 }]
    }
});

// Server creates:
// - User: Alice
// - Device: Alice's Phone (owned by Alice)
// - Passkey credential

// 4. Alice redirected to dashboard (sees only her devices)
```

## Open Questions

1. **Admin concept?** Should there be a special "admin" user who can see all devices?
   - Proposal: `users.is_admin` flag, admin sees all devices

2. **Device transfer?** Can devices be transferred between users?
   - Proposal: Owner can transfer via "Transfer Device" → Generates code for recipient

3. **Guest access?** Temporary access for guests?
   - Proposal: Guest sessions (read-only, time-limited)

4. **Username requirements?** Auto-generate vs user-chosen?
   - Proposal: Auto-generate from display name ("Felix" → "felix"), allow customization

## Success Criteria

✅ Felix can register his laptop (creates user + device)
✅ Felix can add his phone to his device group
✅ Alice can create her own account
✅ Felix can't see Alice's devices (privacy)
✅ Shared devices visible to all users
✅ Users can rename their devices
✅ Users can change device visibility
✅ Passkey-based authentication works smoothly
✅ Existing PIN-based pairing continues to work

## Next Steps

1. Implement user model database schema
2. Add user registration/authentication endpoints
3. Update dashboard to show user-filtered devices
4. Add invitation code generation
5. Build "Add Device" flow with QR codes
6. Test multi-user scenarios
