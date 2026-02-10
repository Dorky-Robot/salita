use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::auth::peer_auth::PeerNode;
use crate::state::AppState;

/// Middleware that accepts EITHER session auth OR peer token auth
/// Useful for GraphQL and API endpoints that can be called by users or peers
pub async fn flexible_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Try peer token first (Bearer token in Authorization header)
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                // Verify peer token
                match verify_peer_token_internal(&state, token).await {
                    Ok(peer_node) => {
                        req.extensions_mut().insert(peer_node);
                        return Ok(next.run(req).await);
                    }
                    Err(_) => {
                        // Invalid token - fall through to session auth
                    }
                }
            }
        }
    }

    // Fall back to session cookie auth
    // The CurrentUser extractor will handle this
    // For now, just pass through - routes with CurrentUser will enforce session auth
    Ok(next.run(req).await)
}

async fn verify_peer_token_internal(state: &AppState, token: &str) -> Result<PeerNode, StatusCode> {
    use chrono::{Duration, Utc};

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
        .ok();
    } else {
        conn.execute(
            "UPDATE issued_tokens SET last_used_at = datetime('now') WHERE token = ?",
            [token],
        )
        .ok();
    }

    // Parse permissions
    let permissions: Vec<String> = serde_json::from_str(&permissions).unwrap_or_default();

    Ok(PeerNode {
        node_id,
        permissions,
    })
}
