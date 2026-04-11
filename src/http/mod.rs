mod content;
mod files;
mod mesh;

use axum::routing::get;
use axum::Router;
use tokio::sync::watch;

use crate::config::Config;
use crate::db::DbPool;
use crate::discovery::MdnsDiscovery;
use crate::node::NodeIdentity;

#[derive(Clone)]
pub struct HttpState {
    pub config: Config,
    pub db: DbPool,
    pub node_identity: NodeIdentity,
}

pub async fn run_serve(
    config: Config,
    pool: DbPool,
    node_identity: NodeIdentity,
) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let mdns = MdnsDiscovery::start(
        &node_identity.id,
        &node_identity.name,
        config.server.port,
        pool.clone(),
        shutdown_rx,
    )?;

    let state = HttpState {
        config: config.clone(),
        db: pool,
        node_identity,
    };

    let app = Router::new()
        .route("/health", get(health))
        .merge(mesh::router())
        .merge(files::router())
        .merge(content::router())
        .with_state(state);

    let addr: std::net::SocketAddr =
        format!("{}:{}", config.server.host, config.server.port).parse()?;
    tracing::info!("Salita daemon listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    let _ = shutdown_tx.send(true);
    mdns.shutdown();

    Ok(())
}

async fn health() -> &'static str {
    "ok"
}
