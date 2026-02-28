mod config;
mod db;
mod discovery;
mod error;
mod files;
mod http;
mod mcp;
mod node;
mod peer_client;

use clap::Parser;
use rusqlite::params;
use tracing_subscriber::EnvFilter;

use crate::config::{Cli, Command, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // In MCP mode, tracing must go to stderr (stdout is the MCP transport)
    let is_mcp = matches!(cli.command, Command::Mcp);

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(if is_mcp { "warn" } else { "info" })),
        )
        .with_writer(if is_mcp {
            std::io::stderr as fn() -> std::io::Stderr
        } else {
            std::io::stderr as fn() -> std::io::Stderr
        })
        .init();

    let data_dir = Config::data_dir(&cli);
    std::fs::create_dir_all(&data_dir)?;

    let config = Config::load(&cli)?;
    let db_path = Config::db_path(&cli);
    let pool = db::create_pool(&db_path)?;
    db::run_migrations(&pool)?;

    let node_identity = node::NodeIdentity::load_or_create(&data_dir)?;
    tracing::info!("Node: {} ({})", node_identity.name, node_identity.id);

    // Register self in devices table
    {
        let conn = pool.get()?;
        conn.execute(
            "INSERT INTO devices (id, name, endpoint, port, status, last_seen, is_self)
             VALUES (?1, ?2, ?3, ?4, 'online', datetime('now'), 1)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               endpoint = excluded.endpoint,
               port = excluded.port,
               status = 'online',
               last_seen = datetime('now'),
               is_self = 1",
            params![
                &node_identity.id,
                &node_identity.name,
                "localhost",
                config.server.port
            ],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO current_node (node_id) VALUES (?1)",
            params![&node_identity.id],
        )?;
    }

    match cli.command {
        Command::Serve { .. } => {
            http::run_serve(config, pool, node_identity).await?;
        }
        Command::Mcp => {
            mcp::run_mcp(config, pool).await?;
        }
    }

    Ok(())
}
