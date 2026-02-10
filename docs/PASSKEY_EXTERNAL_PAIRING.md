# Passkey-Based External Pairing Design

## Problem Statement

Currently, Salita supports local network pairing using QR codes and PINs. This works great for devices on the same LAN, but fails for:

- **Devices connecting via ngrok** (e.g., `https://abc123.ngrok.io`)
- **Public domain access** (e.g., `https://salita.example.com`)
- **Remote access scenarios** where QR code + PIN isn't practical

For external connections, we need a more secure authentication method that:
1. Works across the internet (not just LAN)
2. Doesn't rely on being on the same network
3. Provides stronger security than PIN codes
4. Generates tokens for API access

## Solution: Passkey + PRF Extension

### Architecture Overview

```
External Device (Phone/Laptop)
    ↓
    1. Navigate to https://salita.example.com/pair
    ↓
    2. Authenticate with Passkey (WebAuthn)
    ↓
    3. PRF extension derives 32-byte secret
    ↓
    4. Server issues long-lived API token
    ↓
    5. Device stores token + uses for API calls
```

### Key Components

#### 1. Passkey Registration Flow

```rust
// New endpoint: POST /auth/passkey/register
pub async fn register_passkey(
    State(state): State<AppState>,
    Json(req): Json<RegisterPasskeyRequest>,
) -> AppResult<Response> {
    // Start WebAuthn registration ceremony
    // Enable PRF extension during registration
    // Store passkey credential in database
    // Return challenge to client
}
```

#### 2. PRF-Based Authentication

The PRF (Pseudo-Random Function) extension allows deriving cryptographic secrets from passkeys:

- **Input**: 32-byte salt (server-generated)
- **Output**: 32-byte deterministic secret (derived from passkey)
- **Use case**: Generate API tokens without storing secrets

```javascript
// Client-side: Request PRF during authentication
const assertion = await navigator.credentials.get({
  publicKey: {
    challenge: serverChallenge,
    extensions: {
      prf: {
        eval: {
          first: saltFromServer  // 32-byte salt
        }
      }
    }
  }
});

// Extract PRF output
const prfSecret = assertion.getClientExtensionResults().prf.results.first;
// Use this to derive API token
```

#### 3. Token Exchange

After successful passkey authentication:

1. Server verifies passkey signature
2. Server derives API token from PRF output (or generates new token)
3. Server stores token → node_id mapping
4. Client receives token for API calls

```rust
// New endpoint: POST /auth/passkey/authenticate
pub async fn authenticate_passkey(
    State(state): State<AppState>,
    Json(req): Json<AuthenticatePasskeyRequest>,
) -> AppResult<Response> {
    // Verify WebAuthn assertion
    // Extract PRF output from client
    // Generate or derive API token
    // Return token + permissions
}
```

### Database Schema

```sql
-- New migration: 009_passkey_external_pairing.sql

-- Store PRF salts per user/device
CREATE TABLE prf_salts (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    salt BLOB NOT NULL,  -- 32-byte salt for PRF
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, node_id)
);

-- Link passkey credentials to nodes
CREATE TABLE passkey_nodes (
    credential_id TEXT NOT NULL REFERENCES passkey_credentials(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (credential_id, node_id)
);

CREATE INDEX idx_passkey_nodes_node ON passkey_nodes(node_id);
```

### PRF Extension Details

From research ([Corbado Blog](https://www.corbado.com/blog/passkeys-prf-webauthn), [W3C Explainer](https://github.com/w3c/webauthn/wiki/Explainer:-PRF-extension)):

**How it works:**
- PRF uses CTAP2's `hmac-secret` extension
- Takes a secret key (in authenticator) + input salt → deterministic output
- Same passkey + same salt = same 32-byte output
- Output can be used as symmetric encryption key or token seed

**Browser Support (2026):**
- ✅ Android (Chrome, Firefox)
- ⚠️ macOS/iOS (partial support)
- ❌ Windows Hello (not yet)

**Our Strategy:**
- Use PRF opportunistically (when available)
- Fallback: traditional token generation without PRF
- Detect PRF support on client and use it if available

### Implementation Plan

#### Phase 1: Basic Passkey Auth (No PRF)
1. Add passkey registration endpoint
2. Add passkey authentication endpoint
3. Generate standard API tokens after successful auth
4. Test with external ngrok URL

#### Phase 2: PRF Extension Integration
1. Enable PRF during registration (if supported)
2. Generate and store PRF salts per device
3. Use PRF output to derive API tokens
4. Add PRF availability detection

#### Phase 3: Token Management
1. Token rotation using PRF second salt
2. Token revocation
3. Device management UI (list/remove passkey devices)

### Security Considerations

**Advantages:**
- 🔐 Phishing-resistant (passkeys bound to domain)
- 🔐 No shared secrets (public key crypto)
- 🔐 PRF output never leaves authenticator
- 🔐 Replay attack protection (challenge-response)

**Risks & Mitigations:**
- **Risk**: PRF not supported → **Mitigation**: Graceful fallback
- **Risk**: Lost authenticator → **Mitigation**: Multiple passkeys per user
- **Risk**: Token theft → **Mitigation**: Short-lived tokens + refresh flow

### User Experience Flow

#### Desktop → Phone Pairing (External)

1. **Desktop**: Navigate to settings → "Add Device"
2. **Desktop**: Shows QR code with URL: `https://salita.example.com/pair?session=abc123`
3. **Phone**: Scans QR → Redirected to pairing page
4. **Phone**: Prompted for passkey authentication
5. **Phone**: Creates/uses passkey → PRF derives secret
6. **Server**: Issues API token to phone
7. **Phone**: Stores token → Can now call API

#### Comparison with Local Pairing

| Feature | Local (QR + PIN) | External (Passkey) |
|---------|------------------|-------------------|
| Network | Same LAN only | Any network |
| Security | 6-digit PIN | Public key crypto |
| Duration | Single-use | Multi-use passkey |
| Token | Generated | PRF-derived |
| Browser | Any | WebAuthn-capable |

### API Endpoints

```rust
// New routes in src/routes/auth.rs

// Start passkey registration
POST /auth/passkey/register/start
→ Returns WebAuthn challenge + options

// Finish passkey registration
POST /auth/passkey/register/finish
→ Stores credential, returns success

// Start passkey authentication
POST /auth/passkey/authenticate/start
→ Returns WebAuthn challenge + PRF salt

// Finish passkey authentication
POST /auth/passkey/authenticate/finish
→ Verifies assertion, returns API token

// Token refresh (using PRF)
POST /auth/passkey/token/refresh
→ Extends token expiry or issues new token
```

### WebAuthn-rs PRF Support Investigation

**Current Status:**
- webauthn-rs v0.5.x is in use
- PRF extension not explicitly documented in [webauthn-rs docs](https://docs.rs/webauthn-rs/latest/webauthn_rs/)
- Need to investigate webauthn-rs-proto types for PRF support

**Action Items:**
1. Check `webauthn-rs-proto` crate for `AuthenticationExtensionsPRFInputs`
2. Look at GitHub repo for PRF examples: https://github.com/kanidm/webauthn-rs
3. If not supported, consider contributing PRF support
4. Fallback: Implement without PRF for now, add later

### Testing Strategy

**Unit Tests:**
- Passkey registration/authentication flows
- PRF salt generation
- Token derivation from PRF output
- Fallback when PRF unavailable

**Integration Tests:**
- End-to-end pairing via passkey
- Token usage for API calls
- Multiple devices with same passkey

**Manual Tests:**
- Test with ngrok public URL
- Test on Android (good PRF support)
- Test on macOS/iOS (partial support)
- Test fallback on Windows

### Next Steps

1. ✅ Research PRF extension (DONE)
2. 🔲 Investigate webauthn-rs PRF support
3. 🔲 Implement basic passkey auth (no PRF)
4. 🔲 Add PRF extension if supported
5. 🔲 Build device management UI
6. 🔲 Test with ngrok/external domain

## References

- [WebAuthn PRF Extension Explainer (W3C)](https://github.com/w3c/webauthn/wiki/Explainer:-PRF-extension)
- [Passkeys & WebAuthn PRF for E2E Encryption (Corbado)](https://www.corbado.com/blog/passkeys-prf-webauthn)
- [MDN: Web Authentication Extensions](https://developer.mozilla.org/en-US/docs/Web/API/Web_Authentication_API/WebAuthn_extensions)
- [Yubico: PRF Extension](https://developers.yubico.com/WebAuthn/Concepts/PRF_Extension/)
- [webauthn-rs GitHub Repository](https://github.com/kanidm/webauthn-rs)
- [webauthn-rs crates.io](https://crates.io/crates/webauthn-rs)
