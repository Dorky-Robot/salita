mod auth;
mod config;
mod db;
mod error;
mod extractors;
mod routes;
mod state;

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use clap::Parser;
use rusqlite::params;
use std::net::SocketAddr;
use tokio::sync::Mutex;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::auth::webauthn::CeremonyStore;
use crate::config::{Cli, Config};
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Parse CLI args and load config
    let cli = Cli::parse();
    let data_dir = Config::data_dir(&cli);
    std::fs::create_dir_all(&data_dir)?;
    tracing::info!("Data directory: {}", data_dir.display());

    let config = Config::load(&cli)?;

    // Ensure uploads directory exists
    std::fs::create_dir_all(config.uploads_path())?;

    // Initialize database
    let pool = db::create_pool(config.db_path())?;
    db::run_migrations(&pool)?;

    // Build WebAuthn instance
    let webauthn = auth::webauthn::build_webauthn(config.server.port)
        .expect("Failed to build WebAuthn instance");

    // Build app state
    let state = AppState {
        db: pool,
        config: config.clone(),
        webauthn: Arc::new(webauthn),
        ceremonies: Arc::new(Mutex::new(CeremonyStore::new())),
    };

    // Build router
    let mut app = Router::new()
        .route("/", get(routes::home::index))
        .route("/assets/{*path}", get(routes::assets::serve))
        .merge(routes::auth::router())
        .merge(routes::stream::router());

    // Test-only seed endpoint: creates a user + session, returns session cookie
    if std::env::var("SALITA_TEST_SEED").is_ok() {
        app = app.route("/test/seed", get(test_seed));
    }

    let app = app.layer(TraceLayer::new_for_http()).with_state(state);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Test-only: seed a user + session and return the session cookie.
/// Only mounted when SALITA_TEST_SEED env var is set.
async fn test_seed(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.get().unwrap();
    let user_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO users (id, username, is_admin) VALUES (?1, 'testuser', 1)",
        params![user_id],
    )
    .unwrap();

    // Get the actual user id (may already exist from previous seed call)
    let uid: String = conn
        .query_row(
            "SELECT id FROM users WHERE username = 'testuser'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let token =
        auth::session::create_session(&state.db, &uid, state.config.auth.session_hours).unwrap();

    let cookie = format!(
        "salita_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=3600",
        token
    );

    (
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        format!("{{\"user_id\":\"{}\",\"username\":\"testuser\"}}", uid),
    )
}
