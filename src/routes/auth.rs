use axum::routing::{get, post};
use axum::Router;

use crate::auth::{handlers, handlers_v2};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/setup", get(handlers::setup_page))
        .route("/auth/setup/start", post(handlers::setup_start))
        .route("/auth/setup/finish", post(handlers::setup_finish))
        .route("/auth/login", get(handlers::login_page))
        .route("/auth/login/start", post(handlers::login_start))
        .route("/auth/login/finish", post(handlers::login_finish))
        .route("/auth/logout", post(handlers::logout))
        .route("/auth/context", get(handlers::auth_context))
        // V2 pairing endpoints (refactored with domain model + repository)
        .route("/auth/pair/start/v2", post(handlers_v2::start_pairing))
        .route("/auth/pair/connect/v2", post(handlers_v2::connect_device))
        .route("/auth/pair/verify/v2", post(handlers_v2::verify_pin))
        .route("/auth/pair/status/v2", get(handlers_v2::pairing_status))
}
