use async_graphql::*;
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::graphql::types::{
    MeshNode, NodeOperationResult, RegisterNodeInput, UpdateNodeStatusInput,
};

// Helper to parse datetime from database string
fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .ok()
        .and_then(|dt| Some(dt.with_timezone(&Utc)))
        .unwrap_or_else(|| Utc::now())
}

/// GraphQL Mutation root
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Register a new node in the mesh
    async fn register_node(
        &self,
        ctx: &Context<'_>,
        input: RegisterNodeInput,
    ) -> Result<NodeOperationResult> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let node_id = uuid::Uuid::now_v7().to_string();
        let now = Utc::now().to_rfc3339();
        let capabilities_json = serde_json::to_string(&input.capabilities.unwrap_or_default())
            .unwrap_or_else(|_| "[]".to_string());

        let result = conn.execute(
            "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
             VALUES (?1, ?2, ?3, ?4, 'offline', ?5, ?6, ?7, ?8, 0)",
            params![
                node_id,
                input.name,
                input.hostname,
                input.port,
                capabilities_json,
                now,
                now,
                input.metadata,
            ],
        );

        match result {
            Ok(_) => {
                // Fetch the created node
                let node: Result<MeshNode, rusqlite::Error> = conn.query_row(
                    "SELECT id, name, hostname, port, status, capabilities, last_seen, created_at, metadata
                     FROM mesh_nodes WHERE id = ?1",
                    params![node_id],
                    |row| {
                        Ok(MeshNode {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            hostname: row.get(2)?,
                            port: row.get(3)?,
                            status: crate::graphql::types::NodeStatus::Offline,
                            capabilities: serde_json::from_str(&row.get::<_, String>(5)?)
                                .unwrap_or_default(),
                            last_seen: parse_datetime(row.get(6)?),
                            created_at: parse_datetime(row.get(7)?),
                            metadata: row.get(8)?,
                        })
                    },
                );

                Ok(NodeOperationResult {
                    success: true,
                    message: format!("Node '{}' registered successfully", input.name),
                    node: node.ok(),
                })
            }
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to register node: {}", e),
                node: None,
            }),
        }
    }

    /// Update the status of a node
    async fn update_node_status(
        &self,
        ctx: &Context<'_>,
        input: UpdateNodeStatusInput,
    ) -> Result<NodeOperationResult> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let status_str = match input.status {
            crate::graphql::types::NodeStatus::Online => "online",
            crate::graphql::types::NodeStatus::Offline => "offline",
            crate::graphql::types::NodeStatus::Degraded => "degraded",
        };

        let now = Utc::now().to_rfc3339();
        let result = conn.execute(
            "UPDATE mesh_nodes SET status = ?1, last_seen = ?2 WHERE id = ?3",
            params![status_str, now, input.node_id],
        );

        match result {
            Ok(rows) if rows > 0 => {
                // Fetch the updated node
                let node: Result<MeshNode, rusqlite::Error> = conn.query_row(
                    "SELECT id, name, hostname, port, status, capabilities, last_seen, created_at, metadata
                     FROM mesh_nodes WHERE id = ?1",
                    params![input.node_id],
                    |row| {
                        Ok(MeshNode {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            hostname: row.get(2)?,
                            port: row.get(3)?,
                            status: input.status,
                            capabilities: serde_json::from_str(&row.get::<_, String>(5)?)
                                .unwrap_or_default(),
                            last_seen: parse_datetime(row.get(6)?),
                            created_at: parse_datetime(row.get(7)?),
                            metadata: row.get(8)?,
                        })
                    },
                );

                Ok(NodeOperationResult {
                    success: true,
                    message: format!("Node status updated to {:?}", input.status),
                    node: node.ok(),
                })
            }
            Ok(_) => Ok(NodeOperationResult {
                success: false,
                message: "Node not found".to_string(),
                node: None,
            }),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to update node status: {}", e),
                node: None,
            }),
        }
    }

    /// Remove a node from the mesh
    async fn remove_node(&self, ctx: &Context<'_>, node_id: String) -> Result<NodeOperationResult> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let result = conn.execute("DELETE FROM mesh_nodes WHERE id = ?1", params![node_id]);

        match result {
            Ok(rows) if rows > 0 => Ok(NodeOperationResult {
                success: true,
                message: "Node removed successfully".to_string(),
                node: None,
            }),
            Ok(_) => Ok(NodeOperationResult {
                success: false,
                message: "Node not found".to_string(),
                node: None,
            }),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to remove node: {}", e),
                node: None,
            }),
        }
    }

    /// Heartbeat - update this node's last_seen timestamp
    async fn heartbeat(&self, ctx: &Context<'_>) -> Result<NodeOperationResult> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let now = Utc::now().to_rfc3339();
        let result = conn.execute(
            "UPDATE mesh_nodes SET last_seen = ?1, status = 'online' WHERE is_current = 1",
            params![now],
        );

        match result {
            Ok(_) => Ok(NodeOperationResult {
                success: true,
                message: "Heartbeat recorded".to_string(),
                node: None,
            }),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to record heartbeat: {}", e),
                node: None,
            }),
        }
    }
}
