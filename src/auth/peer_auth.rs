use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::{Duration, Utc};

#[derive(Clone, Debug)]
pub struct PeerNode {
    pub node_id: String,
    pub permissions: Vec<String>,
}

/// Middleware to verify peer-to-peer authentication tokens
pub async fn verify_peer_token(
    State(state): State<crate::state::AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract bearer token
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up token
    let (node_id, permissions, expires_at, revoked_at) = conn
        .query_row(
            "SELECT issued_to_node_id, permissions, expires_at, revoked_at
             FROM issued_tokens
             WHERE token = ?",
            [token],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Check revoked
    if revoked_at.is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Check expired (with 5-minute grace period for clock skew)
    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = Utc::now();
    let grace_period = Duration::minutes(5);

    if expires_at + grace_period < now {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Auto-renew if close to expiring (< 7 days)
    if expires_at < now + Duration::days(7) {
        let new_expires = now + Duration::days(30);
        conn.execute(
            "UPDATE issued_tokens
             SET expires_at = ?, last_used_at = datetime('now')
             WHERE token = ?",
            rusqlite::params![new_expires.to_rfc3339(), token],
        )
        .ok(); // Don't fail request if renewal fails
    } else {
        conn.execute(
            "UPDATE issued_tokens SET last_used_at = datetime('now') WHERE token = ?",
            [token],
        )
        .ok();
    }

    // Parse permissions
    let permissions: Vec<String> = serde_json::from_str(&permissions).unwrap_or_default();

    // Store peer identity in request extensions
    req.extensions_mut().insert(PeerNode {
        node_id,
        permissions,
    });

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_node_can_be_created() {
        let peer = PeerNode {
            node_id: "test-node-id".to_string(),
            permissions: vec!["posts:read".to_string()],
        };
        assert_eq!(peer.node_id, "test-node-id");
        assert_eq!(peer.permissions.len(), 1);
    }
}
