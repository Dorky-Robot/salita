use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::header;
use axum::http::request::Parts;
use rusqlite::params;
use std::net::SocketAddr;

use crate::auth::{detect_origin, RequestOrigin};
use crate::error::AppError;
use crate::state::AppState;

/// Represents the currently authenticated user.
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
}

/// Extractor that requires authentication.
/// Returns 401 if no valid session found.
/// Localhost requests are auto-authenticated with a synthetic user (unless disabled in config).
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract request origin to check if localhost
        let origin = RequestOrigin::from_request_parts(parts, state).await?;

        // Auto-authenticate localhost requests (unless disabled in config)
        if origin == RequestOrigin::Localhost && !state.config.auth.disable_localhost_bypass {
            return Ok(CurrentUser {
                id: "localhost".to_string(),
                username: "localhost".to_string(),
                is_admin: true,
            });
        }

        // For non-localhost, require session authentication
        let token = extract_session_token(parts).ok_or(AppError::Unauthorized)?;

        let conn = state.db.get()?;
        conn.query_row(
            "SELECT u.id, u.username, u.is_admin FROM sessions s \
             JOIN users u ON u.id = s.user_id \
             WHERE s.token = ?1 AND s.expires_at > datetime('now')",
            params![token],
            |row| {
                Ok(CurrentUser {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    is_admin: row.get(2)?,
                })
            },
        )
        .map_err(|_| AppError::Unauthorized)
    }
}

/// Optional user extractor — returns None instead of 401 when not authenticated.
/// Localhost requests are auto-authenticated with a synthetic user (unless disabled in config).
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract request origin to check if localhost
        let origin = RequestOrigin::from_request_parts(parts, state).await?;

        // Auto-authenticate localhost requests (unless disabled in config)
        if origin == RequestOrigin::Localhost && !state.config.auth.disable_localhost_bypass {
            return Ok(MaybeUser(Some(CurrentUser {
                id: "localhost".to_string(),
                username: "localhost".to_string(),
                is_admin: true,
            })));
        }

        // For non-localhost, try session authentication but don't fail
        match CurrentUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(MaybeUser(Some(user))),
            Err(_) => Ok(MaybeUser(None)),
        }
    }
}

/// Extractor for RequestOrigin based on socket address and Host header
impl FromRequestParts<AppState> for RequestOrigin {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Extract ConnectInfo to get socket address
        let connect_info = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(|| AppError::Internal("Missing ConnectInfo extension".into()))?;

        // Extract Host header
        let host = parts
            .headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok());

        Ok(detect_origin(connect_info, host))
    }
}

fn extract_session_token(parts: &Parts) -> Option<&str> {
    parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|cookie| {
            let mut split = cookie.splitn(2, '=');
            let key = split.next()?.trim();
            let val = split.next()?.trim();
            if key == "salita_session" {
                Some(val)
            } else {
                None
            }
        })
}
