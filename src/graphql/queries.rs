use async_graphql::*;
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::graphql::types::{MeshNode, NodeConnection, NodeStatus};

// Helper to parse datetime from database string
fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .ok()
        .and_then(|dt| Some(dt.with_timezone(&Utc)))
        .unwrap_or_else(|| Utc::now())
}

// Helper to parse optional datetime from database string
fn parse_optional_datetime(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// GraphQL Query root
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Get information about this node
    async fn current_node(&self, ctx: &Context<'_>) -> Result<MeshNode> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        // Get the current node (there should only be one with is_current = true)
        let node: Result<MeshNode, rusqlite::Error> = conn.query_row(
            "SELECT id, name, hostname, port, status, capabilities, last_seen, created_at, metadata
             FROM mesh_nodes WHERE is_current = 1",
            [],
            |row| {
                Ok(MeshNode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hostname: row.get(2)?,
                    port: row.get(3)?,
                    status: parse_status(row.get::<_, String>(4)?),
                    capabilities: parse_capabilities(row.get::<_, String>(5)?),
                    last_seen: parse_datetime(row.get(6)?),
                    created_at: parse_datetime(row.get(7)?),
                    metadata: row.get(8)?,
                })
            },
        );

        node.map_err(|e| Error::new(format!("Failed to get current node: {}", e)))
    }

    /// Get a specific node by ID
    async fn node(&self, ctx: &Context<'_>, id: String) -> Result<Option<MeshNode>> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let node: Result<MeshNode, rusqlite::Error> = conn.query_row(
            "SELECT id, name, hostname, port, status, capabilities, last_seen, created_at, metadata
             FROM mesh_nodes WHERE id = ?1",
            params![id],
            |row| {
                Ok(MeshNode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hostname: row.get(2)?,
                    port: row.get(3)?,
                    status: parse_status(row.get::<_, String>(4)?),
                    capabilities: parse_capabilities(row.get::<_, String>(5)?),
                    last_seen: parse_datetime(row.get(6)?),
                    created_at: parse_datetime(row.get(7)?),
                    metadata: row.get(8)?,
                })
            },
        );

        match node {
            Ok(n) => Ok(Some(n)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::new(format!("Database error: {}", e))),
        }
    }

    /// List all nodes in the mesh
    async fn nodes(&self, ctx: &Context<'_>) -> Result<Vec<MeshNode>> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT id, name, hostname, port, status, capabilities, last_seen, created_at, metadata
             FROM mesh_nodes ORDER BY last_seen DESC",
        )?;

        let nodes: Vec<MeshNode> = stmt
            .query_map([], |row| {
                Ok(MeshNode {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    hostname: row.get(2)?,
                    port: row.get(3)?,
                    status: parse_status(row.get::<_, String>(4)?),
                    capabilities: parse_capabilities(row.get::<_, String>(5)?),
                    last_seen: parse_datetime(row.get(6)?),
                    created_at: parse_datetime(row.get(7)?),
                    metadata: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(nodes)
    }

    /// List all connections between nodes
    async fn connections(&self, ctx: &Context<'_>) -> Result<Vec<NodeConnection>> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT from_node_id, to_node_id, connection_type, status, last_ping, latency_ms
             FROM node_connections ORDER BY last_ping DESC",
        )?;

        let connections: Vec<NodeConnection> = stmt
            .query_map([], |row| {
                Ok(NodeConnection {
                    from_node_id: row.get(0)?,
                    to_node_id: row.get(1)?,
                    connection_type: parse_connection_type(row.get::<_, String>(2)?),
                    status: parse_connection_status(row.get::<_, String>(3)?),
                    last_ping: parse_optional_datetime(row.get(4)?),
                    latency_ms: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(connections)
    }

    /// Get connections for a specific node
    async fn node_connections(
        &self,
        ctx: &Context<'_>,
        node_id: String,
    ) -> Result<Vec<NodeConnection>> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let mut stmt = conn.prepare(
            "SELECT from_node_id, to_node_id, connection_type, status, last_ping, latency_ms
             FROM node_connections
             WHERE from_node_id = ?1 OR to_node_id = ?1
             ORDER BY last_ping DESC",
        )?;

        let connections: Vec<NodeConnection> = stmt
            .query_map(params![node_id], |row| {
                Ok(NodeConnection {
                    from_node_id: row.get(0)?,
                    to_node_id: row.get(1)?,
                    connection_type: parse_connection_type(row.get::<_, String>(2)?),
                    status: parse_connection_status(row.get::<_, String>(3)?),
                    last_ping: parse_optional_datetime(row.get(4)?),
                    latency_ms: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(connections)
    }
}

// Helper functions to parse enum values from database strings
fn parse_status(s: String) -> NodeStatus {
    match s.as_str() {
        "online" => NodeStatus::Online,
        "offline" => NodeStatus::Offline,
        "degraded" => NodeStatus::Degraded,
        _ => NodeStatus::Offline,
    }
}

fn parse_capabilities(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn parse_connection_type(s: String) -> crate::graphql::types::ConnectionType {
    use crate::graphql::types::ConnectionType;
    match s.as_str() {
        "webrtc" => ConnectionType::WebRtc,
        "http" => ConnectionType::Http,
        _ => ConnectionType::Unknown,
    }
}

fn parse_connection_status(s: String) -> crate::graphql::types::ConnectionStatus {
    use crate::graphql::types::ConnectionStatus;
    match s.as_str() {
        "active" => ConnectionStatus::Active,
        "idle" => ConnectionStatus::Idle,
        "disconnected" => ConnectionStatus::Disconnected,
        _ => ConnectionStatus::Disconnected,
    }
}
