use rand::Rng;
use rusqlite::params;

/// Generate a cryptographically secure 64-character hex token
pub fn generate_secure_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Store peer token (token we use to call another device)
pub fn store_peer_token(
    conn: &rusqlite::Connection,
    peer_id: &str,
    token: &str,
    permissions: &[String],
    expires_at: &str,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO peer_tokens (peer_node_id, token, permissions, expires_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(peer_node_id) DO UPDATE SET
           token = excluded.token,
           expires_at = excluded.expires_at,
           permissions = excluded.permissions",
        params![
            peer_id,
            token,
            serde_json::to_string(&permissions)?,
            expires_at
        ],
    )?;
    Ok(())
}

/// Issue token to another device (they will use this to call us)
pub fn issue_token(
    conn: &rusqlite::Connection,
    to_node_id: &str,
    permissions: &[String],
    expires_at: &str,
) -> anyhow::Result<String> {
    let token = generate_secure_token();
    conn.execute(
        "INSERT INTO issued_tokens (token, issued_to_node_id, permissions, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            &token,
            to_node_id,
            serde_json::to_string(&permissions)?,
            expires_at
        ],
    )?;
    Ok(token)
}

/// Default permissions for paired devices
pub fn default_permissions() -> Vec<String> {
    vec![
        "posts:read".to_string(),
        "posts:create".to_string(),
        "media:read".to_string(),
        "media:upload".to_string(),
        "comments:create".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_is_64_hex_chars() {
        let token = generate_secure_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_unique() {
        let t1 = generate_secure_token();
        let t2 = generate_secure_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn default_permissions_include_basic_operations() {
        let perms = default_permissions();
        assert!(perms.contains(&"posts:read".to_string()));
        assert!(perms.contains(&"posts:create".to_string()));
        assert!(perms.contains(&"media:read".to_string()));
        assert!(!perms.contains(&"admin:all".to_string()));
    }
}
