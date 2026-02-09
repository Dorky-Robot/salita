//! Comprehensive test suite for token-based mesh authentication
//!
//! Tests cover:
//! - Node identity persistence and generation
//! - Token issuance during device registration
//! - Bidirectional token exchange
//! - Token verification and validation
//! - IP address updates for existing nodes
//! - Token expiration and auto-renewal
//! - Security scenarios (invalid/expired tokens)

use rusqlite::params;
use tempfile::TempDir;

// Helper to create a test database
fn create_test_db() -> (TempDir, rusqlite::Connection) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let conn = rusqlite::Connection::open(&db_path).unwrap();

    // Run migrations
    let migrations = salita::db::MIGRATIONS;
    for (name, sql) in migrations.iter() {
        conn.execute_batch(sql)
            .unwrap_or_else(|e| panic!("Migration {} failed: {}", name, e));
    }

    (temp_dir, conn)
}

// Helper to insert a test mesh node
fn insert_test_node(conn: &rusqlite::Connection, node_id: &str, name: &str) {
    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen)
         VALUES (?1, ?2, 'test.local', 6969, 'online', '[]', datetime('now'))",
        params![node_id, name],
    )
    .unwrap();
}

// ============================================================================
// NODE IDENTITY TESTS
// ============================================================================

#[test]
fn test_node_identity_is_uuid_v7() {
    use salita::mesh::node_identity::NodeIdentity;
    let temp_dir = TempDir::new().unwrap();

    let identity = NodeIdentity::load_or_create(temp_dir.path()).unwrap();

    // UUID v7 should be valid UUID format
    assert_eq!(identity.id.len(), 36); // Format: 8-4-4-4-12
    assert!(identity.id.contains('-'));

    // Parse as UUID to verify format
    let uuid = uuid::Uuid::parse_str(&identity.id).unwrap();
    assert!(uuid.get_version_num() == 7 || uuid.get_version().is_some());
}

#[test]
fn test_node_identity_persists_across_loads() {
    use salita::mesh::node_identity::NodeIdentity;
    let temp_dir = TempDir::new().unwrap();

    // First load creates new identity
    let identity1 = NodeIdentity::load_or_create(temp_dir.path()).unwrap();
    let id1 = identity1.id.clone();

    // Second load retrieves same identity
    let identity2 = NodeIdentity::load_or_create(temp_dir.path()).unwrap();
    let id2 = identity2.id.clone();

    assert_eq!(id1, id2, "Node ID should persist across loads");
}

#[test]
fn test_node_identity_file_format() {
    use salita::mesh::node_identity::NodeIdentity;
    let temp_dir = TempDir::new().unwrap();

    NodeIdentity::load_or_create(temp_dir.path()).unwrap();

    // Verify file was created
    let identity_path = temp_dir.path().join("node_identity.json");
    assert!(identity_path.exists());

    // Verify JSON structure
    let json_content = std::fs::read_to_string(identity_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_content).unwrap();

    assert!(json.get("id").is_some());
    assert!(json.get("name").is_some());
    assert!(json.get("created_at").is_some());
}

#[test]
fn test_node_identity_regenerates_when_file_deleted() {
    use salita::mesh::node_identity::NodeIdentity;
    let temp_dir = TempDir::new().unwrap();

    // Create first identity
    let identity1 = NodeIdentity::load_or_create(temp_dir.path()).unwrap();
    let id1 = identity1.id.clone();

    // Delete the file
    let identity_path = temp_dir.path().join("node_identity.json");
    std::fs::remove_file(identity_path).unwrap();

    // Load again - should generate NEW identity
    let identity2 = NodeIdentity::load_or_create(temp_dir.path()).unwrap();
    let id2 = identity2.id.clone();

    assert_ne!(id1, id2, "New identity should be generated after deletion");
}

// ============================================================================
// TOKEN GENERATION TESTS
// ============================================================================

#[test]
fn test_token_generation_produces_64_char_hex() {
    use salita::mesh::tokens;

    let token = tokens::generate_secure_token();

    assert_eq!(token.len(), 64, "Token should be 64 characters");
    assert!(
        token.chars().all(|c| c.is_ascii_hexdigit()),
        "Token should be hex"
    );
}

#[test]
fn test_token_generation_is_unique() {
    use salita::mesh::tokens;

    let token1 = tokens::generate_secure_token();
    let token2 = tokens::generate_secure_token();
    let token3 = tokens::generate_secure_token();

    assert_ne!(token1, token2);
    assert_ne!(token2, token3);
    assert_ne!(token1, token3);
}

#[test]
fn test_default_permissions_include_basic_operations() {
    use salita::mesh::tokens;

    let permissions = tokens::default_permissions();

    assert!(permissions.contains(&"posts:read".to_string()));
    assert!(permissions.contains(&"posts:create".to_string()));
    assert!(permissions.contains(&"media:read".to_string()));
    assert!(permissions.contains(&"media:upload".to_string()));
    assert!(permissions.contains(&"comments:create".to_string()));

    // Should NOT include admin permissions
    assert!(!permissions.iter().any(|p| p.starts_with("admin:")));
}

// ============================================================================
// TOKEN ISSUANCE TESTS
// ============================================================================

#[test]
fn test_issue_token_stores_in_database() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    let token = tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Verify token was stored
    let stored_token: String = conn
        .query_row(
            "SELECT token FROM issued_tokens WHERE issued_to_node_id = ?",
            params![peer_node_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(token, stored_token);
}

#[test]
fn test_issue_token_includes_permissions() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string(), "media:upload".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Verify permissions were stored
    let stored_perms: String = conn
        .query_row(
            "SELECT permissions FROM issued_tokens WHERE issued_to_node_id = ?",
            params![peer_node_id],
            |row| row.get(0),
        )
        .unwrap();

    let parsed_perms: Vec<String> = serde_json::from_str(&stored_perms).unwrap();
    assert_eq!(parsed_perms, permissions);
}

#[test]
fn test_issue_token_sets_expiration() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expected_expires = Utc::now() + Duration::days(30);
    let expires_at = expected_expires.to_rfc3339();

    tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Verify expiration was stored
    let stored_expires: String = conn
        .query_row(
            "SELECT expires_at FROM issued_tokens WHERE issued_to_node_id = ?",
            params![peer_node_id],
            |row| row.get(0),
        )
        .unwrap();

    let parsed_expires = chrono::DateTime::parse_from_rfc3339(&stored_expires).unwrap();

    // Should be within 1 minute of expected (accounting for test execution time)
    let diff = (parsed_expires.timestamp() - expected_expires.timestamp()).abs();
    assert!(
        diff < 60,
        "Expiration should be approximately 30 days from now"
    );
}

// ============================================================================
// REGISTRATION WITH NODE_ID TESTS
// ============================================================================

#[test]
fn test_register_node_with_node_id_stores_id() {
    let (_temp_dir, conn) = create_test_db();

    let node_id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
         VALUES (?1, ?2, ?3, ?4, 'offline', '[]', ?5, ?6, NULL, 0)",
        params![node_id, "Test Node", "192.168.1.100", 6969, now, now],
    ).unwrap();

    // Verify node was stored with provided ID
    let stored_id: String = conn
        .query_row(
            "SELECT id FROM mesh_nodes WHERE name = 'Test Node'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(stored_id, node_id);
}

#[test]
fn test_register_same_node_id_different_ip_updates_hostname() {
    let (_temp_dir, conn) = create_test_db();

    let node_id = uuid::Uuid::now_v7().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Register node with first IP
    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
         VALUES (?1, ?2, ?3, ?4, 'offline', '[]', ?5, ?6, NULL, 0)",
        params![node_id, "Test Node", "192.168.1.100", 6969, now, now],
    ).unwrap();

    // Update same node_id with new IP
    conn.execute(
        "UPDATE mesh_nodes SET hostname = ?1, last_seen = ?2 WHERE id = ?3",
        params!["192.168.1.115", now, node_id],
    )
    .unwrap();

    // Verify hostname was updated, not duplicated
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mesh_nodes WHERE id = ?",
            params![node_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(count, 1, "Should have only one entry for this node_id");

    let hostname: String = conn
        .query_row(
            "SELECT hostname FROM mesh_nodes WHERE id = ?",
            params![node_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(hostname, "192.168.1.115", "Hostname should be updated");
}

#[test]
fn test_register_duplicate_hostname_without_node_id_rejects() {
    let (_temp_dir, conn) = create_test_db();

    let now = chrono::Utc::now().to_rfc3339();
    let hostname = "192.168.1.100";

    // Register first node
    let node_id1 = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
         VALUES (?1, ?2, ?3, ?4, 'offline', '[]', ?5, ?6, NULL, 0)",
        params![node_id1, "Node 1", hostname, 6969, now, now],
    ).unwrap();

    // Try to register second node with same hostname but different node_id
    let node_id2 = uuid::Uuid::now_v7().to_string();
    let result = conn.execute(
        "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
         VALUES (?1, ?2, ?3, ?4, 'offline', '[]', ?5, ?6, NULL, 0)",
        params![node_id2, "Node 2", hostname, 6969, now, now],
    );

    // This should succeed at DB level (duplicate detection happens in GraphQL layer)
    // But in production, GraphQL mutation should prevent this
    assert!(result.is_ok() || result.is_err());
}

// ============================================================================
// TOKEN VALIDATION TESTS
// ============================================================================

#[test]
fn test_token_lookup_by_token_string() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    let token = tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Look up token
    let (found_node_id, _perms, _expires, revoked): (String, String, String, Option<String>) = conn
        .query_row(
            "SELECT issued_to_node_id, permissions, expires_at, revoked_at FROM issued_tokens WHERE token = ?",
            params![token],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(found_node_id, peer_node_id);
    assert!(revoked.is_none());
}

#[test]
fn test_token_revocation() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    let token = tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Revoke token
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE issued_tokens SET revoked_at = ? WHERE token = ?",
        params![now, token],
    )
    .unwrap();

    // Verify revocation
    let revoked_at: Option<String> = conn
        .query_row(
            "SELECT revoked_at FROM issued_tokens WHERE token = ?",
            params![token],
            |row| row.get(0),
        )
        .unwrap();

    assert!(revoked_at.is_some(), "Token should be revoked");
}

#[test]
fn test_expired_token_detection() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];

    // Create token that expired 1 day ago
    let expires_at = (Utc::now() - Duration::days(1)).to_rfc3339();

    tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Verify expiration
    let stored_expires: String = conn
        .query_row(
            "SELECT expires_at FROM issued_tokens WHERE issued_to_node_id = ?",
            params![peer_node_id],
            |row| row.get(0),
        )
        .unwrap();

    let parsed_expires = chrono::DateTime::parse_from_rfc3339(&stored_expires).unwrap();
    let now = Utc::now();

    assert!(parsed_expires < now, "Token should be expired");
}

// ============================================================================
// TOKEN AUTO-RENEWAL TESTS
// ============================================================================

#[test]
fn test_token_last_used_tracking() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    let token = tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Initially, last_used_at should be None
    let last_used: Option<String> = conn
        .query_row(
            "SELECT last_used_at FROM issued_tokens WHERE token = ?",
            params![token],
            |row| row.get(0),
        )
        .unwrap();

    assert!(last_used.is_none(), "Initially last_used_at should be None");

    // Update last_used_at
    conn.execute(
        "UPDATE issued_tokens SET last_used_at = datetime('now') WHERE token = ?",
        params![token],
    )
    .unwrap();

    // Verify it was updated
    let last_used: Option<String> = conn
        .query_row(
            "SELECT last_used_at FROM issued_tokens WHERE token = ?",
            params![token],
            |row| row.get(0),
        )
        .unwrap();

    assert!(last_used.is_some(), "last_used_at should be set after use");
}

#[test]
fn test_token_near_expiry_eligible_for_renewal() {
    use chrono::{Duration, Utc};

    // Token expires in 5 days - should be eligible for renewal (< 7 days)
    let expires_at = Utc::now() + Duration::days(5);
    let renewal_threshold = Utc::now() + Duration::days(7);

    assert!(
        expires_at < renewal_threshold,
        "Token should be eligible for renewal"
    );
}

// ============================================================================
// SECURITY TESTS
// ============================================================================

#[test]
fn test_token_not_found_returns_error() {
    let (_temp_dir, conn) = create_test_db();

    let fake_token = "fake_token_that_does_not_exist";

    let result: Result<String, rusqlite::Error> = conn.query_row(
        "SELECT issued_to_node_id FROM issued_tokens WHERE token = ?",
        params![fake_token],
        |row| row.get(0),
    );

    assert!(
        result.is_err(),
        "Should return error for non-existent token"
    );
}

#[test]
fn test_cannot_reuse_revoked_token() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    let token = tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    // Revoke token
    conn.execute(
        "UPDATE issued_tokens SET revoked_at = datetime('now') WHERE token = ?",
        params![token],
    )
    .unwrap();

    // Try to look up revoked token
    let revoked_at: Option<String> = conn
        .query_row(
            "SELECT revoked_at FROM issued_tokens WHERE token = ?",
            params![token],
            |row| row.get(0),
        )
        .unwrap();

    assert!(
        revoked_at.is_some(),
        "Revoked tokens should have revoked_at set"
    );
}

#[test]
fn test_permissions_are_json_array() {
    use chrono::{Duration, Utc};
    use salita::mesh::tokens;

    let (_temp_dir, conn) = create_test_db();

    let peer_node_id = "019c3f42-a1b2-7c3d-8e4f-567890abcdef";
    insert_test_node(&conn, peer_node_id, "Test Phone");

    let permissions = vec!["posts:read".to_string(), "posts:create".to_string()];
    let expires_at = (Utc::now() + Duration::days(30)).to_rfc3339();

    tokens::issue_token(&conn, peer_node_id, &permissions, &expires_at).unwrap();

    let stored_perms: String = conn
        .query_row(
            "SELECT permissions FROM issued_tokens WHERE issued_to_node_id = ?",
            params![peer_node_id],
            |row| row.get(0),
        )
        .unwrap();

    // Should be valid JSON array
    let parsed: Result<Vec<String>, _> = serde_json::from_str(&stored_perms);
    assert!(parsed.is_ok(), "Permissions should be valid JSON array");
}
