use rand::Rng;
use rusqlite::params;

use crate::state::DbPool;

/// Create a new session for a user. Returns the session token.
pub fn create_session(pool: &DbPool, user_id: &str, hours: u64) -> Result<String, rusqlite::Error> {
    let conn = pool.get().map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(e.to_string()),
        )
    })?;

    let token = generate_token();
    let id = uuid::Uuid::now_v7().to_string();

    conn.execute(
        "INSERT INTO sessions (id, user_id, token, expires_at) VALUES (?1, ?2, ?3, datetime('now', ?4))",
        params![id, user_id, token, format!("+{} hours", hours)],
    )?;

    Ok(token)
}

/// Delete a session by token.
pub fn delete_session(pool: &DbPool, token: &str) -> Result<(), rusqlite::Error> {
    let conn = pool.get().map_err(|e| {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(e.to_string()),
        )
    })?;

    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}

/// Generate a cryptographically random 32-byte hex token.
fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_is_64_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_unique() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
    }
}
