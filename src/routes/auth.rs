use axum::routing::{get, post};
use axum::Router;

use crate::auth::handlers;
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
}
