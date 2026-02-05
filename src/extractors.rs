use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use rusqlite::params;

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
impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
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
pub struct MaybeUser(pub Option<CurrentUser>);

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match CurrentUser::from_request_parts(parts, state).await {
            Ok(user) => Ok(MaybeUser(Some(user))),
            Err(_) => Ok(MaybeUser(None)),
        }
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
