use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Not found")]
    NotFound,

    #[error("Unauthorized")]
    Unauthorized,

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("WebAuthn error: {0}")]
    Webauthn(#[from] webauthn_rs::prelude::WebauthnError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found".to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Database(e) => {
                tracing::error!("Database error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Pool(e) => {
                tracing::error!("Pool error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            AppError::Webauthn(e) => {
                tracing::error!("WebAuthn error: {}", e);
                (StatusCode::BAD_REQUEST, "WebAuthn error".to_string())
            }
            AppError::Json(e) => {
                tracing::error!("JSON error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
        };

        (status, message).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn response_status(err: AppError) -> StatusCode {
        let response = err.into_response();
        response.status()
    }

    #[test]
    fn not_found_returns_404() {
        assert_eq!(response_status(AppError::NotFound), StatusCode::NOT_FOUND);
    }

    #[test]
    fn unauthorized_returns_401() {
        assert_eq!(
            response_status(AppError::Unauthorized),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn bad_request_returns_400() {
        assert_eq!(
            response_status(AppError::BadRequest("oops".into())),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn internal_returns_500() {
        assert_eq!(
            response_status(AppError::Internal("boom".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
