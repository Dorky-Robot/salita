// Repository pattern - isolates all database side effects
use crate::pairing::domain::*;
use crate::state::DbPool;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::params;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] r2d2::Error),

    #[error("SQL error: {0}")]
    Sql(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),
}

/// Repository trait - all database operations
#[async_trait]
pub trait PairingRepository: Send + Sync {
    /// Load pairing state by token
    async fn load(&self, token: &JoinToken) -> Result<Option<PairingState>, RepositoryError>;

    /// Save pairing state (idempotent upsert)
    async fn save(&self, state: &PairingState) -> Result<(), RepositoryError>;

    /// Delete pairing state
    #[allow(dead_code)]
    async fn delete(&self, token: &JoinToken) -> Result<bool, RepositoryError>;

    /// Purge expired states (returns count deleted)
    #[allow(dead_code)]
    async fn purge_expired(&self, before: DateTime<Utc>) -> Result<u64, RepositoryError>;

    /// Log pairing event for audit trail
    async fn log_event(
        &self,
        token: &JoinToken,
        event_type: &str,
        event_data: Option<String>,
    ) -> Result<(), RepositoryError>;

    /// Atomically register node with session and token
    async fn register_node_atomic(
        &self,
        node_id: &NodeId,
        name: &str,
        ip: &IpAddress,
        port: u16,
        session_token: &SessionToken,
        session_expires_at: DateTime<Utc>,
        peer_token: &PeerToken,
    ) -> Result<(), RepositoryError>;
}

/// SQLite implementation
pub struct SqlitePairingRepository {
    pool: DbPool,
}

impl SqlitePairingRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PairingRepository for SqlitePairingRepository {
    async fn load(&self, token: &JoinToken) -> Result<Option<PairingState>, RepositoryError> {
        let conn = self.pool.get()?;

        let result: Result<String, rusqlite::Error> = conn.query_row(
            "SELECT state_json FROM pairing_states WHERE token = ?1",
            params![token.as_str()],
            |row| row.get(0),
        );

        match result {
            Ok(json) => {
                let state: PairingState = serde_json::from_str(&json)?;
                Ok(Some(state))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn save(&self, state: &PairingState) -> Result<(), RepositoryError> {
        let conn = self.pool.get()?;

        let state_json = serde_json::to_string(state)?;
        let token = state.token();

        conn.execute(
            "INSERT INTO pairing_states (token, state_json, updated_at)
             VALUES (?1, ?2, datetime('now'))
             ON CONFLICT(token) DO UPDATE SET
               state_json = excluded.state_json,
               updated_at = excluded.updated_at",
            params![token.as_str(), state_json],
        )?;

        Ok(())
    }

    async fn delete(&self, token: &JoinToken) -> Result<bool, RepositoryError> {
        let conn = self.pool.get()?;

        let rows = conn.execute(
            "DELETE FROM pairing_states WHERE token = ?1",
            params![token.as_str()],
        )?;

        Ok(rows > 0)
    }

    async fn purge_expired(&self, before: DateTime<Utc>) -> Result<u64, RepositoryError> {
        let conn = self.pool.get()?;

        // Load all states, check expiration, delete expired ones
        // (We could optimize this by storing expiry separately, but for now keep it simple)
        let mut stmt =
            conn.prepare("SELECT token, state_json FROM pairing_states WHERE created_at < ?1")?;

        let cutoff = before.to_rfc3339();
        let states: Vec<(String, String)> = stmt
            .query_map(params![cutoff], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut deleted = 0u64;

        for (token_str, state_json) in states {
            if let Ok(state) = serde_json::from_str::<PairingState>(&state_json) {
                if state.is_expired(before) || state.is_failed() {
                    conn.execute(
                        "DELETE FROM pairing_states WHERE token = ?1",
                        params![token_str],
                    )?;
                    deleted += 1;
                }
            }
        }

        Ok(deleted)
    }

    async fn log_event(
        &self,
        token: &JoinToken,
        event_type: &str,
        event_data: Option<String>,
    ) -> Result<(), RepositoryError> {
        let conn = self.pool.get()?;

        conn.execute(
            "INSERT INTO pairing_events (token, event_type, event_data)
             VALUES (?1, ?2, ?3)",
            params![token.as_str(), event_type, event_data],
        )?;

        Ok(())
    }

    async fn register_node_atomic(
        &self,
        node_id: &NodeId,
        name: &str,
        ip: &IpAddress,
        port: u16,
        session_token: &SessionToken,
        session_expires_at: DateTime<Utc>,
        peer_token: &PeerToken,
    ) -> Result<(), RepositoryError> {
        let conn = self.pool.get()?;

        // ATOMIC TRANSACTION - all or nothing!
        conn.execute("BEGIN IMMEDIATE", [])?;

        let result: Result<(), RepositoryError> = (|| {
            // 1. Insert/update node
            conn.execute(
                "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, is_current)
                 VALUES (?1, ?2, ?3, ?4, 'offline', '[]', datetime('now'), datetime('now'), 0)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   hostname = excluded.hostname,
                   port = excluded.port,
                   last_seen = datetime('now')",
                params![node_id.as_str(), name, ip.as_str(), port],
            )?;

            // 2. Create device session
            conn.execute(
                "INSERT INTO device_sessions (session_token, node_id, expires_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    session_token.as_str(),
                    node_id.as_str(),
                    session_expires_at.to_rfc3339()
                ],
            )?;

            // 3. Issue peer token
            let default_permissions = vec![
                "posts:read".to_string(),
                "posts:create".to_string(),
                "media:read".to_string(),
                "media:upload".to_string(),
                "comments:create".to_string(),
            ];

            let expires_at = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();

            conn.execute(
                "INSERT INTO issued_tokens (token, issued_to_node_id, permissions, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    peer_token.as_str(),
                    node_id.as_str(),
                    serde_json::to_string(&default_permissions)?,
                    expires_at
                ],
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

/// Type alias for Arc-wrapped repository (for AppState)
pub type DynPairingRepository = Arc<dyn PairingRepository>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::Duration;
    use tempfile::TempDir;

    fn create_test_repo() -> (SqlitePairingRepository, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = db::create_pool(&db_path).unwrap();
        db::run_migrations(&pool).unwrap();

        (SqlitePairingRepository::new(pool), temp_dir)
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let (repo, _temp) = create_test_repo();

        let token = JoinToken::new("test123");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token.clone(), now, 300);

        // Save
        repo.save(&state).await.unwrap();

        // Load
        let loaded = repo.load(&token).await.unwrap();
        assert_eq!(loaded, Some(state));
    }

    #[tokio::test]
    async fn test_save_is_idempotent() {
        let (repo, _temp) = create_test_repo();

        let token = JoinToken::new("test123");
        let now = Utc::now();
        let state1 = PairingCoordinator::create_pairing(token.clone(), now, 300);

        // Save twice
        repo.save(&state1).await.unwrap();
        repo.save(&state1).await.unwrap();

        // Should only have one entry
        let loaded = repo.load(&token).await.unwrap();
        assert_eq!(loaded, Some(state1));
    }

    #[tokio::test]
    async fn test_state_transitions_persist() {
        let (repo, _temp) = create_test_repo();

        let token = JoinToken::new("test123");
        let now = Utc::now();

        // Create initial state
        let state = PairingCoordinator::create_pairing(token.clone(), now, 300);
        repo.save(&state).await.unwrap();

        // Transition to connected
        let loaded = repo.load(&token).await.unwrap().unwrap();
        let (state, _pin) = loaded
            .connect_device(IpAddress::new("192.168.1.100"), now)
            .unwrap();
        repo.save(&state).await.unwrap();

        // Load and verify state changed
        let loaded = repo.load(&token).await.unwrap().unwrap();
        assert_eq!(loaded.state_name(), "DeviceConnected");
    }

    #[tokio::test]
    async fn test_delete() {
        let (repo, _temp) = create_test_repo();

        let token = JoinToken::new("test123");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token.clone(), now, 300);

        repo.save(&state).await.unwrap();

        // Delete
        let deleted = repo.delete(&token).await.unwrap();
        assert!(deleted);

        // Should not exist
        let loaded = repo.load(&token).await.unwrap();
        assert_eq!(loaded, None);

        // Delete again should return false
        let deleted = repo.delete(&token).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_purge_expired() {
        let (repo, _temp) = create_test_repo();

        let now = Utc::now();
        let past = now - Duration::hours(2);

        // Create expired state
        let token1 = JoinToken::new("expired");
        let state1 = PairingCoordinator::create_pairing(token1.clone(), past, 300); // 5 min TTL, created 2 hours ago
        repo.save(&state1).await.unwrap();

        // Create valid state
        let token2 = JoinToken::new("valid");
        let state2 = PairingCoordinator::create_pairing(token2.clone(), now, 300);
        repo.save(&state2).await.unwrap();

        // Purge
        let deleted = repo.purge_expired(now).await.unwrap();
        assert_eq!(deleted, 1);

        // Expired should be gone
        assert_eq!(repo.load(&token1).await.unwrap(), None);

        // Valid should remain
        assert!(repo.load(&token2).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_log_event() {
        let (repo, _temp) = create_test_repo();

        let token = JoinToken::new("test123");

        // Log event
        repo.log_event(&token, "created", Some("test data".to_string()))
            .await
            .unwrap();

        // Verify it was stored (query directly)
        let conn = repo.pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pairing_events WHERE token = ?1",
                params![token.as_str()],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_register_node_atomic() {
        let (repo, _temp) = create_test_repo();

        let node_id = NodeId::new("node123");
        let session_token = SessionToken::new("session456");
        let peer_token = PeerToken::new("peer789");
        let expires_at = Utc::now() + Duration::hours(24);

        // Register atomically
        repo.register_node_atomic(
            &node_id,
            "Test Device",
            &IpAddress::new("192.168.1.100"),
            6969,
            &session_token,
            expires_at,
            &peer_token,
        )
        .await
        .unwrap();

        // Verify all three inserts happened
        let conn = repo.pool.get().unwrap();

        // Check node
        let node_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_nodes WHERE id = ?1",
                params![node_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(node_exists, 1);

        // Check session
        let session_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM device_sessions WHERE session_token = ?1",
                params![session_token.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_exists, 1);

        // Check token
        let token_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM issued_tokens WHERE token = ?1",
                params![peer_token.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token_exists, 1);
    }

    #[tokio::test]
    async fn test_register_node_atomic_rollback_on_error() {
        let (repo, _temp) = create_test_repo();

        // First, create a device session with this token
        let session_token = SessionToken::new("duplicate_session");
        let node_id1 = NodeId::new("node1");
        let expires_at = Utc::now() + Duration::hours(24);

        let conn = repo.pool.get().unwrap();
        conn.execute(
            "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, is_current)
             VALUES (?1, 'Device 1', '192.168.1.1', 6969, 'offline', '[]', datetime('now'), datetime('now'), 0)",
            params![node_id1.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO device_sessions (session_token, node_id, expires_at) VALUES (?1, ?2, ?3)",
            params![
                session_token.as_str(),
                node_id1.as_str(),
                expires_at.to_rfc3339()
            ],
        )
        .unwrap();

        drop(conn);

        // Now try to register a different node with the same session token
        // This should fail due to PRIMARY KEY constraint on session_token
        let node_id2 = NodeId::new("node2");
        let peer_token = PeerToken::new("peer789");

        let result = repo
            .register_node_atomic(
                &node_id2,
                "Test Device 2",
                &IpAddress::new("192.168.1.100"),
                6969,
                &session_token, // Duplicate!
                expires_at,
                &peer_token,
            )
            .await;

        // Should fail
        assert!(result.is_err());

        // Verify rollback - node2 should NOT exist
        let conn = repo.pool.get().unwrap();
        let node2_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mesh_nodes WHERE id = ?1",
                params![node_id2.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(node2_exists, 0);

        // peer_token should NOT exist
        let token_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM issued_tokens WHERE token = ?1",
                params![peer_token.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(token_exists, 0);
    }
}
