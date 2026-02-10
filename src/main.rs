mod auth;
mod config;
mod db;
mod error;
mod extractors;
mod graphql;
mod mesh;
mod pairing;
mod routes;
mod state;
mod tls;

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

use crate::auth::join_tokens::JoinTokenStore;
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

    // Initialize node identity
    let node_identity = mesh::node_identity::NodeIdentity::load_or_create(&data_dir)?;
    tracing::info!(
        "Node ID: {}, Name: {}",
        node_identity.id,
        node_identity.name
    );

    // Ensure current_node table is populated
    {
        let conn = pool.get()?;
        // First, ensure this node exists in mesh_nodes
        conn.execute(
            "INSERT OR REPLACE INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, metadata, is_current)
             VALUES (?1, ?2, ?3, ?4, 'online', '[]', datetime('now'), NULL, 1)",
            params![
                &node_identity.id,
                &node_identity.name,
                "localhost",
                config.server.port
            ],
        )?;

        // Then populate current_node
        conn.execute(
            "INSERT OR REPLACE INTO current_node (node_id) VALUES (?1)",
            params![&node_identity.id],
        )?;
    }

    // Build WebAuthn instance
    let site_url = config.site_url();
    tracing::info!("Site URL (WebAuthn origin): {}", site_url);
    let webauthn =
        auth::webauthn::build_webauthn(&site_url).expect("Failed to build WebAuthn instance");

    // Build GraphQL schema
    let graphql_schema = graphql::build_schema();

    // Build app state
    let state = AppState {
        db: pool,
        config: config.clone(),
        data_dir: data_dir.clone(),
        webauthn: Arc::new(webauthn),
        ceremonies: Arc::new(Mutex::new(CeremonyStore::new())),
        join_tokens: Arc::new(Mutex::new(JoinTokenStore::new())),
        graphql_schema,
    };

    // Build main app router
    let mut app = Router::new()
        .route("/", get(routes::home::index))
        .route("/assets/{*path}", get(routes::assets::serve))
        .merge(routes::auth::router())
        .merge(routes::dashboard::router())
        .merge(routes::stream::router())
        .merge(routes::settings::router())
        .merge(routes::graphql::router());

    // Test-only seed endpoint: creates a user + session, returns session cookie
    if std::env::var("SALITA_TEST_SEED").is_ok() {
        app = app.route("/test/seed", get(test_seed));
    }

    let app = app
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    if config.tls_enabled() {
        // TLS mode: HTTPS on main port + HTTP on onboarding port
        let tls_paths = tls::ensure_certs(&data_dir, &config.instance_name)?;
        let rustls_config = tls::load_rustls_config(&tls_paths).await?;

        let https_addr: SocketAddr =
            format!("{}:{}", config.server.host, config.server.port).parse()?;
        tracing::info!("HTTPS server listening on https://{}", https_addr);

        // Build HTTP-only router for trust/onboarding (no redirects)
        // ONLY serves the certificate trust page - everything else must be HTTPS
        let http_app = routes::trust::router()
            .layer(TraceLayer::new_for_http())
            .with_state(state.clone());

        let http_addr: SocketAddr =
            format!("{}:{}", config.server.host, config.tls.http_port).parse()?;
        tracing::info!("HTTP onboarding server listening on http://{}", http_addr);

        // Run both servers concurrently
        let https_server = axum_server::bind_rustls(https_addr, rustls_config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>());

        let http_listener = tokio::net::TcpListener::bind(http_addr).await?;
        let http_server = axum::serve(
            http_listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        );

        println!("\n📋 Setup Instructions:");
        println!(
            "   1. Trust certificate: http://localhost:{}/connect/trust",
            config.tls.http_port
        );
        println!(
            "   2. Access app:        https://localhost:{}\n",
            config.server.port
        );

        tokio::select! {
            result = https_server => { result?; }
            result = http_server => { result?; }
        }
    } else {
        // Plain HTTP mode (backward compatible)
        let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
        tracing::info!("Listening on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
    }

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
