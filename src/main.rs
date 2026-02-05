mod auth;
mod config;
mod db;
mod error;
mod extractors;
mod routes;
mod state;

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use clap::Parser;
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
    let app = Router::new()
        .route("/", get(routes::home::index))
        .route("/assets/{*path}", get(routes::assets::serve))
        .merge(routes::auth::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    tracing::info!("Listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
