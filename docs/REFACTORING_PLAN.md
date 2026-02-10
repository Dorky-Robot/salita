# Device Pairing Refactoring Plan
## Applying Functional Principles to Push Side Effects to System Edges

**Goal**: Create a maintainable, testable codebase where business logic is pure and side effects are isolated at boundaries.

**Principles**:
- Pure functions at the core (no I/O, no mutations)
- Side effects only at system edges (HTTP handlers, database layer)
- Immutable data structures
- Explicit state transitions
- Test business logic without mocking

---

## Phase 1: Domain Model & Pure Business Logic

### 1.1 Define Core Domain Types (Pure, Immutable)

**Create**: `src/pairing/domain.rs`

```rust
// Core domain types - no side effects, no I/O
use chrono::{DateTime, Utc};

/// Pairing request from a device wanting to join
#[derive(Debug, Clone, PartialEq)]
pub struct PairingRequest {
    pub device_node_id: NodeId,
    pub device_ip: IpAddress,
    pub join_token: JoinToken,
}

/// State machine for pairing process
#[derive(Debug, Clone, PartialEq)]
pub enum PairingState {
    /// Token created, waiting for device to scan
    TokenCreated {
        token: JoinToken,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },

    /// Device scanned QR, PIN generated
    DeviceConnected {
        token: JoinToken,
        device_ip: IpAddress,
        device_node_id: Option<NodeId>,
        pin: Pin,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },

    /// Desktop verified PIN, session created
    PinVerified {
        token: JoinToken,
        device_ip: IpAddress,
        device_node_id: NodeId,
        session_token: SessionToken,
        created_at: DateTime<Utc>,
    },

    /// Device fully registered in mesh
    DeviceRegistered {
        node: MeshNode,
        peer_token: PeerToken,
        session_token: SessionToken,
    },

    /// Pairing failed or expired
    Failed {
        reason: PairingFailure,
        state_at_failure: Box<PairingState>,
    },
}

/// Pure state transitions - no side effects!
impl PairingState {
    /// Transition: Token → DeviceConnected
    pub fn connect_device(
        self,
        device_ip: IpAddress,
        now: DateTime<Utc>,
    ) -> Result<(Self, Pin), PairingError> {
        match self {
            Self::TokenCreated { token, expires_at, .. } => {
                if now > expires_at {
                    return Err(PairingError::TokenExpired);
                }

                let pin = Pin::generate_random(); // Pure: uses RNG passed as param

                Ok((
                    Self::DeviceConnected {
                        token,
                        device_ip,
                        device_node_id: None,
                        pin: pin.clone(),
                        created_at: now,
                        expires_at,
                    },
                    pin,
                ))
            }
            other => Err(PairingError::InvalidTransition {
                from: other.name(),
                to: "DeviceConnected",
            }),
        }
    }

    /// Transition: DeviceConnected → PinVerified
    pub fn verify_pin(
        self,
        provided_pin: &Pin,
        device_node_id: NodeId,
        session_token: SessionToken,
        now: DateTime<Utc>,
    ) -> Result<Self, PairingError> {
        match self {
            Self::DeviceConnected { token, device_ip, pin, expires_at, .. } => {
                if now > expires_at {
                    return Err(PairingError::TokenExpired);
                }

                if &pin != provided_pin {
                    return Err(PairingError::InvalidPin);
                }

                Ok(Self::PinVerified {
                    token,
                    device_ip,
                    device_node_id,
                    session_token,
                    created_at: now,
                })
            }
            other => Err(PairingError::InvalidTransition {
                from: other.name(),
                to: "PinVerified",
            }),
        }
    }

    /// Transition: PinVerified → DeviceRegistered
    pub fn register_device(
        self,
        node: MeshNode,
        peer_token: PeerToken,
    ) -> Result<Self, PairingError> {
        match self {
            Self::PinVerified { session_token, .. } => {
                Ok(Self::DeviceRegistered {
                    node,
                    peer_token,
                    session_token,
                })
            }
            other => Err(PairingError::InvalidTransition {
                from: other.name(),
                to: "DeviceRegistered",
            }),
        }
    }

    /// Check if state is expired
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self {
            Self::TokenCreated { expires_at, .. } => now > *expires_at,
            Self::DeviceConnected { expires_at, .. } => now > *expires_at,
            Self::PinVerified { .. } => false, // No expiry after PIN verified
            Self::DeviceRegistered { .. } => false,
            Self::Failed { .. } => true,
        }
    }
}

/// New types for type safety
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinToken(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin(String);

impl Pin {
    pub fn generate_random() -> Self {
        // Pure: accepts RNG as parameter
        use rand::Rng;
        let pin = rand::thread_rng().gen_range(100000..=999999);
        Self(pin.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToken(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpAddress(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerToken(String);
```

**Tests**: `src/pairing/domain_tests.rs`
```rust
#[test]
fn test_valid_state_transitions() {
    let token = JoinToken::new("test123");
    let now = Utc::now();
    let expires = now + Duration::minutes(5);

    // Start state
    let state = PairingState::TokenCreated {
        token: token.clone(),
        created_at: now,
        expires_at: expires,
    };

    // Transition 1: Connect device
    let (state, pin) = state.connect_device(IpAddress("192.168.1.100"), now).unwrap();
    assert!(matches!(state, PairingState::DeviceConnected { .. }));

    // Transition 2: Verify PIN
    let state = state.verify_pin(
        &pin,
        NodeId::new("node123"),
        SessionToken::new("session456"),
        now,
    ).unwrap();
    assert!(matches!(state, PairingState::PinVerified { .. }));

    // Transition 3: Register device
    let node = MeshNode::new("Test Device", "192.168.1.100", 6969);
    let peer_token = PeerToken::new("peer789");
    let state = state.register_device(node, peer_token).unwrap();
    assert!(matches!(state, PairingState::DeviceRegistered { .. }));
}

#[test]
fn test_invalid_transition_rejected() {
    let state = PairingState::TokenCreated {
        token: JoinToken::new("test"),
        created_at: Utc::now(),
        expires_at: Utc::now() + Duration::minutes(5),
    };

    // Cannot verify PIN from TokenCreated state
    let result = state.verify_pin(
        &Pin::new("123456"),
        NodeId::new("node"),
        SessionToken::new("session"),
        Utc::now(),
    );

    assert!(matches!(result, Err(PairingError::InvalidTransition { .. })));
}

#[test]
fn test_expired_token_rejected() {
    let now = Utc::now();
    let state = PairingState::TokenCreated {
        token: JoinToken::new("test"),
        created_at: now - Duration::minutes(10),
        expires_at: now - Duration::minutes(5), // Already expired
    };

    assert!(state.is_expired(now));

    let result = state.connect_device(IpAddress("192.168.1.100"), now);
    assert!(matches!(result, Err(PairingError::TokenExpired)));
}
```

### 1.2 Pure Business Logic - Pairing Coordinator

**Create**: `src/pairing/coordinator.rs`

```rust
/// Pure coordinator - all inputs explicit, returns decisions
pub struct PairingCoordinator;

impl PairingCoordinator {
    /// Decide if device can join mesh (pure business logic)
    pub fn can_register_device(
        device_node_id: &NodeId,
        device_ip: &IpAddress,
        existing_nodes: &[MeshNode],
    ) -> Result<RegistrationDecision, RegistrationError> {
        // Check for duplicate node_id
        if let Some(existing) = existing_nodes.iter().find(|n| n.id == *device_node_id) {
            // Same device re-registering - allow with update
            return Ok(RegistrationDecision::UpdateExisting {
                node_id: device_node_id.clone(),
                changes: NodeChanges {
                    new_ip: Some(device_ip.clone()),
                    ..Default::default()
                },
            });
        }

        // Check for IP conflict (different device, same IP)
        if let Some(existing) = existing_nodes.iter().find(|n| n.ip == *device_ip) {
            return Err(RegistrationError::IpConflict {
                existing_device: existing.name.clone(),
                ip: device_ip.clone(),
            });
        }

        // New device - allow registration
        Ok(RegistrationDecision::RegisterNew {
            node_id: device_node_id.clone(),
        })
    }

    /// Calculate token expiration (pure)
    pub fn calculate_expiry(created_at: DateTime<Utc>, ttl_seconds: u64) -> DateTime<Utc> {
        created_at + chrono::Duration::seconds(ttl_seconds as i64)
    }

    /// Validate pairing can proceed (pure validation)
    pub fn validate_pairing_request(
        state: &PairingState,
        pin: &Pin,
        now: DateTime<Utc>,
    ) -> Result<(), PairingValidationError> {
        // Check not expired
        if state.is_expired(now) {
            return Err(PairingValidationError::Expired);
        }

        // Check in correct state
        match state {
            PairingState::DeviceConnected { pin: stored_pin, .. } => {
                if stored_pin != pin {
                    return Err(PairingValidationError::InvalidPin);
                }
                Ok(())
            }
            _ => Err(PairingValidationError::WrongState),
        }
    }
}
```

**Tests**: All pure, no mocking needed!
```rust
#[test]
fn test_duplicate_node_id_triggers_update() {
    let existing = vec![
        MeshNode { id: NodeId("node1"), ip: IpAddress("192.168.1.100"), .. },
    ];

    let decision = PairingCoordinator::can_register_device(
        &NodeId("node1"), // Same ID
        &IpAddress("192.168.1.200"), // Different IP
        &existing,
    ).unwrap();

    assert!(matches!(decision, RegistrationDecision::UpdateExisting { .. }));
}

#[test]
fn test_ip_conflict_rejected() {
    let existing = vec![
        MeshNode { id: NodeId("node1"), ip: IpAddress("192.168.1.100"), .. },
    ];

    let result = PairingCoordinator::can_register_device(
        &NodeId("node2"), // Different ID
        &IpAddress("192.168.1.100"), // Same IP
        &existing,
    );

    assert!(matches!(result, Err(RegistrationError::IpConflict { .. })));
}
```

---

## Phase 2: Side Effect Boundaries

### 2.1 Repository Pattern for Database

**Create**: `src/pairing/repository.rs`

```rust
/// Repository trait - side effects isolated
#[async_trait]
pub trait PairingRepository: Send + Sync {
    /// Load current pairing state
    async fn load_pairing_state(&self, token: &JoinToken) -> Result<Option<PairingState>, DbError>;

    /// Save pairing state (idempotent)
    async fn save_pairing_state(&self, state: &PairingState) -> Result<(), DbError>;

    /// Load all existing mesh nodes (for conflict detection)
    async fn load_mesh_nodes(&self) -> Result<Vec<MeshNode>, DbError>;

    /// Atomically register node + issue token (transaction)
    async fn register_node_with_token(
        &self,
        node: &MeshNode,
        peer_token: &PeerToken,
        session_token: &SessionToken,
    ) -> Result<(), DbError>;

    /// Clean up expired states
    async fn purge_expired(&self, before: DateTime<Utc>) -> Result<u64, DbError>;
}

/// SQLite implementation
pub struct SqlitePairingRepository {
    pool: DbPool,
}

#[async_trait]
impl PairingRepository for SqlitePairingRepository {
    async fn save_pairing_state(&self, state: &PairingState) -> Result<(), DbError> {
        let conn = self.pool.get()?;

        // Serialize state to JSON for storage
        let state_json = serde_json::to_string(state)?;
        let token = state.token();

        conn.execute(
            "INSERT INTO pairing_states (token, state, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(token) DO UPDATE SET
               state = excluded.state,
               updated_at = excluded.updated_at",
            params![token.as_str(), state_json],
        )?;

        Ok(())
    }

    async fn register_node_with_token(
        &self,
        node: &MeshNode,
        peer_token: &PeerToken,
        session_token: &SessionToken,
    ) -> Result<(), DbError> {
        let conn = self.pool.get()?;

        // ATOMIC TRANSACTION - all or nothing!
        conn.execute("BEGIN IMMEDIATE", [])?;

        let result: Result<(), DbError> = (|| {
            // 1. Insert/update node
            conn.execute(
                "INSERT INTO mesh_nodes (id, name, hostname, port, created_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                   hostname = excluded.hostname,
                   updated_at = datetime('now')",
                params![
                    node.id.as_str(),
                    &node.name,
                    node.ip.as_str(),
                    node.port,
                ],
            )?;

            // 2. Issue peer token
            conn.execute(
                "INSERT INTO issued_tokens (token, issued_to_node_id, permissions, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    peer_token.as_str(),
                    node.id.as_str(),
                    serde_json::to_string(&default_permissions())?,
                    (Utc::now() + Duration::days(30)).to_rfc3339(),
                ],
            )?;

            // 3. Link session to device
            conn.execute(
                "INSERT INTO device_sessions (session_token, node_id, created_at)
                 VALUES (?1, ?2, datetime('now'))",
                params![session_token.as_str(), node.id.as_str()],
            )?;

            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                Err(e)
            }
        }
    }
}
```

### 2.2 HTTP Handler Layer (Side Effects Only)

**Create**: `src/pairing/handlers.rs`

```rust
/// HTTP handlers - orchestrate pure logic + side effects
pub async fn start_pairing(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let now = Utc::now();

    // PURE: Calculate expiry
    let expires_at = PairingCoordinator::calculate_expiry(now, 300);

    // SIDE EFFECT: Generate token (uses RNG)
    let token = JoinToken::generate();

    // PURE: Create initial state
    let pairing_state = PairingState::TokenCreated {
        token: token.clone(),
        created_at: now,
        expires_at,
    };

    // SIDE EFFECT: Save to database
    state.pairing_repo.save_pairing_state(&pairing_state).await?;

    // SIDE EFFECT: Get LAN IP
    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "192.168.1.x".to_string());

    // Render response (pure template data)
    Ok(Json(StartPairingResponse {
        token: token.as_str().to_string(),
        qr_url: format!("http://{}:6969/join?token={}", lan_ip, token.as_str()),
        expires_at: expires_at.to_rfc3339(),
    }))
}

pub async fn connect_device(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<JoinQuery>,
) -> Result<impl IntoResponse, AppError> {
    let token = query.token.ok_or(AppError::BadRequest("Token required"))?;
    let device_ip = IpAddress(addr.ip().to_string());
    let now = Utc::now();

    // SIDE EFFECT: Load current state
    let current_state = state
        .pairing_repo
        .load_pairing_state(&token)
        .await?
        .ok_or(AppError::NotFound("Token not found"))?;

    // PURE: Transition state
    let (new_state, pin) = current_state
        .connect_device(device_ip, now)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // SIDE EFFECT: Save new state
    state.pairing_repo.save_pairing_state(&new_state).await?;

    // Render response
    Ok(Html(JoinMeshTemplate {
        pin: pin.as_str().to_string(),
        local_ip: get_local_ip(),
    }))
}

pub async fn verify_pin(
    State(state): State<AppState>,
    Json(request): Json<VerifyPinRequest>,
) -> Result<impl IntoResponse, AppError> {
    let now = Utc::now();

    // SIDE EFFECT: Load state
    let current_state = state
        .pairing_repo
        .load_pairing_state(&request.token)
        .await?
        .ok_or(AppError::NotFound("Token not found"))?;

    // PURE: Validate
    PairingCoordinator::validate_pairing_request(
        &current_state,
        &request.pin,
        now,
    ).map_err(|e| AppError::BadRequest(e.to_string()))?;

    // SIDE EFFECT: Create session
    let session_token = SessionToken::generate();
    let user_id = get_or_create_default_user(&state.db).await?;
    create_session(&state.db, &user_id, &session_token).await?;

    // PURE: Transition state
    let new_state = current_state
        .verify_pin(&request.pin, request.device_node_id, session_token.clone(), now)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // SIDE EFFECT: Save state
    state.pairing_repo.save_pairing_state(&new_state).await?;

    Ok(Json(VerifyPinResponse {
        success: true,
        session_token: session_token.as_str().to_string(),
    }))
}
```

---

## Phase 3: Testing Strategy

### 3.1 Pure Logic Tests (No Mocking)

```rust
// Domain logic - pure functions, no mocking needed
mod domain_tests {
    #[test]
    fn state_transitions_are_deterministic() {
        // Test all state transitions with known inputs
        // No database, no HTTP, just pure functions
    }

    #[test]
    fn expired_tokens_always_rejected() {
        // Pure time-based logic
    }

    #[test]
    fn registration_conflicts_detected() {
        // Pure conflict detection
    }
}
```

### 3.2 Repository Tests (Real Database)

```rust
// Integration tests with real SQLite
mod repository_tests {
    #[tokio::test]
    async fn save_and_load_pairing_state() {
        let temp_db = create_test_db().await;
        let repo = SqlitePairingRepository::new(temp_db);

        let state = PairingState::TokenCreated { .. };
        repo.save_pairing_state(&state).await.unwrap();

        let loaded = repo.load_pairing_state(&token).await.unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[tokio::test]
    async fn register_node_is_atomic() {
        // Test transaction rollback on error
        // Verify all-or-nothing behavior
    }
}
```

### 3.3 Handler Tests (Mock Repository)

```rust
// Mock repository for handler tests
struct MockPairingRepo {
    states: Arc<Mutex<HashMap<JoinToken, PairingState>>>,
}

#[async_trait]
impl PairingRepository for MockPairingRepo {
    async fn save_pairing_state(&self, state: &PairingState) -> Result<(), DbError> {
        let mut states = self.states.lock().unwrap();
        states.insert(state.token().clone(), state.clone());
        Ok(())
    }

    async fn load_pairing_state(&self, token: &JoinToken) -> Result<Option<PairingState>, DbError> {
        let states = self.states.lock().unwrap();
        Ok(states.get(token).cloned())
    }
}

mod handler_tests {
    #[tokio::test]
    async fn verify_pin_transitions_state() {
        let repo = Arc::new(MockPairingRepo::new());
        let state = AppState { pairing_repo: repo.clone(), .. };

        // Setup: Create pairing in DeviceConnected state
        repo.save_pairing_state(&initial_state).await.unwrap();

        // Act: Verify PIN
        let response = verify_pin(State(state), Json(request)).await.unwrap();

        // Assert: State transitioned
        let new_state = repo.load_pairing_state(&token).await.unwrap().unwrap();
        assert!(matches!(new_state, PairingState::PinVerified { .. }));
    }
}
```

### 3.4 End-to-End Tests

```rust
mod e2e_tests {
    #[tokio::test]
    async fn complete_pairing_flow() {
        let server = spawn_test_server().await;

        // 1. Desktop starts pairing
        let start_resp: StartPairingResponse = server
            .post("/pairing/start")
            .await
            .json();

        // 2. Phone connects
        let connect_resp: JoinPageHtml = server
            .get(&format!("/join?token={}", start_resp.token))
            .await
            .text();
        let pin = extract_pin(&connect_resp);

        // 3. Desktop verifies PIN
        let verify_resp: VerifyPinResponse = server
            .post("/pairing/verify")
            .json(&json!({ "token": start_resp.token, "pin": pin }))
            .await
            .json();

        // 4. Assert device registered
        let nodes: Vec<MeshNode> = server
            .get("/graphql")
            .json(&json!({ "query": "{ nodes { id } }" }))
            .await
            .json();

        assert_eq!(nodes.len(), 2); // Desktop + Phone
    }
}
```

---

## Phase 4: Migration Path

### Step-by-Step Migration (Safe, Incremental)

#### Step 1: Add Domain Types Alongside Existing Code
- Create `src/pairing/domain.rs`
- Define types, no behavior yet
- Add basic unit tests
- **No breaking changes**

#### Step 2: Implement Repository Pattern
- Create `src/pairing/repository.rs`
- Implement `SqlitePairingRepository`
- Create `pairing_states` table
- Add repository to `AppState`
- **Existing code still works**

#### Step 3: Create New Handlers (Parallel)
- Create `src/pairing/handlers.rs`
- Implement new endpoints: `/pairing/v2/*`
- Use new domain types + repository
- **Old endpoints still exist**

#### Step 4: Migrate Templates (One at a Time)
- Update `join_modal.html` to call v2 endpoints
- Test thoroughly
- Update `join_mesh.html` to call v2 endpoints
- Test thoroughly

#### Step 5: Deprecate Old Code
- Mark old handlers as `#[deprecated]`
- Add warnings in logs
- Monitor usage

#### Step 6: Remove Old Code
- Delete deprecated handlers
- Delete `join_tokens.rs` (replaced by repository)
- Clean up AppState

---

## Phase 5: Additional Improvements

### 5.1 Persistent State Table

**Migration**: `009_pairing_state_persistence.sql`
```sql
CREATE TABLE pairing_states (
    token TEXT PRIMARY KEY,
    state TEXT NOT NULL,  -- JSON serialized PairingState
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_pairing_states_updated ON pairing_states(updated_at);

-- Link sessions to devices explicitly
CREATE TABLE device_sessions (
    session_token TEXT PRIMARY KEY,
    node_id TEXT NOT NULL REFERENCES mesh_nodes(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_device_sessions_node ON device_sessions(node_id);
```

### 5.2 Event Sourcing for Audit Trail

```rust
pub enum PairingEvent {
    TokenCreated { token: JoinToken, at: DateTime<Utc> },
    DeviceScanned { token: JoinToken, ip: IpAddress, at: DateTime<Utc> },
    PinGenerated { token: JoinToken, pin: Pin, at: DateTime<Utc> },
    PinVerified { token: JoinToken, at: DateTime<Utc> },
    DeviceRegistered { token: JoinToken, node_id: NodeId, at: DateTime<Utc> },
    PairingFailed { token: JoinToken, reason: String, at: DateTime<Utc> },
}

// Store all events for debugging
CREATE TABLE pairing_events (
    id INTEGER PRIMARY KEY,
    token TEXT NOT NULL,
    event TEXT NOT NULL,  -- JSON
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

### 5.3 Background Cleanup Job

```rust
/// Periodically purge expired states
pub async fn cleanup_expired_pairings(repo: Arc<dyn PairingRepository>) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        let cutoff = Utc::now() - Duration::hours(1);
        match repo.purge_expired(cutoff).await {
            Ok(count) => {
                if count > 0 {
                    tracing::info!("Purged {} expired pairing states", count);
                }
            }
            Err(e) => {
                tracing::error!("Failed to purge expired states: {}", e);
            }
        }
    }
}
```

---

## Benefits Summary

### Testability
- **Before**: 60% test coverage, E2E tests ignored
- **After**: 90%+ coverage, all tests enabled
- Pure functions = fast tests, no mocking

### Maintainability
- **Before**: Business logic scattered across 5 files + templates
- **After**: Domain logic in one place, clear separation
- Easy to reason about state transitions

### Reliability
- **Before**: Partial failures leave inconsistent state
- **After**: Atomic transactions, explicit state machine
- Impossible states are unrepresentable

### Performance
- **Before**: Multiple database calls, no caching
- **After**: Repository can cache, batch operations
- Stateless handlers = horizontal scaling

### Security
- **Before**: Session created before device registered
- **After**: Atomic registration + session creation
- State machine prevents invalid transitions

---

## Estimated Effort

| Phase | Effort | Risk |
|-------|--------|------|
| 1. Domain Model | 2 days | Low - Pure code, well-tested |
| 2. Repository | 3 days | Medium - Database migrations |
| 3. Handlers | 2 days | Low - Thin layer over domain |
| 4. Migration | 3 days | Medium - Need thorough testing |
| 5. Improvements | 2 days | Low - Optional enhancements |
| **Total** | **12 days** | **Medium** |

---

## Success Criteria

- [ ] All E2E tests passing (currently ignored)
- [ ] 90%+ test coverage on pairing flow
- [ ] Zero database inconsistencies in production
- [ ] State machine prevents all invalid transitions
- [ ] Atomic transactions ensure consistency
- [ ] Pure business logic testable without mocking
- [ ] Repository pattern allows easy database swapping
- [ ] Background cleanup prevents state buildup
- [ ] Event sourcing provides audit trail
- [ ] Old code completely removed

---

## Open Questions

1. **Session Management**: Should sessions be scoped to devices or users?
   - Current: One default user, sessions shared
   - Proposed: One user, but sessions linked to specific device

2. **Token Revocation**: How to handle device removal?
   - Need to revoke all issued tokens when device removed
   - Add `revoked_at` column to `issued_tokens`

3. **Multi-Device Sessions**: Can one user pair multiple devices?
   - Currently: Yes, but no limit
   - Proposed: Add device limit (configurable)

4. **Pairing Timeout**: What happens if PIN never verified?
   - Currently: Token expires after 5 minutes, but no cleanup
   - Proposed: Background job purges after 1 hour

5. **Concurrent Pairings**: Can multiple devices pair simultaneously?
   - Currently: Yes, but potential race conditions
   - Proposed: Lock on user_id during pairing

---

## Next Steps

1. Review this plan with team
2. Prioritize phases based on immediate needs
3. Create feature branch: `refactor/pairing-flow`
4. Implement Phase 1 (Domain Model) first
5. Get approval before proceeding to Phase 2

**Questions? Concerns? Feedback welcome!**
