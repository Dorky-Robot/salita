use async_graphql::*;
use chrono::{DateTime, Duration, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;

use crate::graphql::types::{
    MeshNode, NodeOperationResult, RegisterNodeInput, UpdateNodeStatusInput,
};
use crate::mesh::tokens;

// Helper to parse datetime from database string
fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .ok()
        .and_then(|dt| Some(dt.with_timezone(&Utc)))
        .unwrap_or_else(|| Utc::now())
}

// Helper to issue a token to a peer node
fn issue_peer_token(
    conn: &rusqlite::Connection,
    to_node_id: &str,
) -> Result<(String, String, Vec<String>), Box<dyn std::error::Error>> {
    let permissions = tokens::default_permissions();
    let expires_at = Utc::now() + Duration::days(30);
    let expires_at_str = expires_at.to_rfc3339();

    let token = tokens::issue_token(conn, to_node_id, &permissions, &expires_at_str)?;

    Ok((token, expires_at_str, permissions))
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

        // Use provided node_id if available, otherwise generate a new one
        let node_id = input
            .node_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

        // Check if a device with this hostname already exists (but NOT this node_id)
        // If same node_id is reconnecting, we'll update it (upsert)
        let existing: Result<(String, String), rusqlite::Error> = conn.query_row(
            "SELECT name, id FROM mesh_nodes WHERE hostname = ?1 AND is_current = 0",
            params![input.hostname],
            |row| Ok((row.get(0)?, row.get(1)?)),
        );

        if let Ok((existing_name, existing_id)) = existing {
            // If it's a different device trying to use the same IP, reject it
            if existing_id != node_id {
                return Ok(NodeOperationResult {
                    success: false,
                    message: format!(
                        "Device already connected as '{}'. Each device can only be added once.",
                        existing_name
                    ),
                    node: None,
                    access_token: None,
                    expires_at: None,
                    permissions: None,
                });
            }
            // If it's the same device (same node_id), we'll update below
        }

        let now = Utc::now().to_rfc3339();
        let capabilities_json = serde_json::to_string(&input.capabilities.unwrap_or_default())
            .unwrap_or_else(|_| "[]".to_string());

        // Use INSERT OR REPLACE to handle re-registration of the same device
        let result = conn.execute(
            "INSERT INTO mesh_nodes (id, name, hostname, port, status, capabilities, last_seen, created_at, metadata, is_current)
             VALUES (?1, ?2, ?3, ?4, 'offline', ?5, ?6, COALESCE((SELECT created_at FROM mesh_nodes WHERE id = ?1), ?7), ?8, 0)
             ON CONFLICT(id) DO UPDATE SET
               name = excluded.name,
               hostname = excluded.hostname,
               port = excluded.port,
               capabilities = excluded.capabilities,
               last_seen = excluded.last_seen,
               metadata = excluded.metadata",
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

                // Issue access token for peer-to-peer authentication
                match issue_peer_token(&conn, &node_id) {
                    Ok((access_token, expires_at, permissions)) => {
                        Ok(NodeOperationResult::with_token(
                            true,
                            format!("Node '{}' registered successfully", input.name),
                            node.ok(),
                            access_token,
                            expires_at,
                            permissions,
                        ))
                    }
                    Err(e) => {
                        tracing::error!("Failed to issue token: {}", e);
                        // Still return success for registration, just without token
                        Ok(NodeOperationResult::without_token(
                            true,
                            format!("Node '{}' registered (token issuance failed)", input.name),
                            node.ok(),
                        ))
                    }
                }
            }
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to register node: {}", e),
                node: None,
                access_token: None,
                expires_at: None,
                permissions: None,
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
                    access_token: None,
                    expires_at: None,
                    permissions: None,
                })
            }
            Ok(_) => Ok(NodeOperationResult::without_token(
                false,
                "Node not found".to_string(),
                None,
            )),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to update node status: {}", e),
                node: None,
                access_token: None,
                expires_at: None,
                permissions: None,
            }),
        }
    }

    /// Remove a node from the mesh
    async fn remove_node(&self, ctx: &Context<'_>, node_id: String) -> Result<NodeOperationResult> {
        let pool = ctx.data::<Pool<SqliteConnectionManager>>()?;
        let conn = pool.get()?;

        let result = conn.execute("DELETE FROM mesh_nodes WHERE id = ?1", params![node_id]);

        match result {
            Ok(rows) if rows > 0 => Ok(NodeOperationResult::without_token(
                true,
                "Node removed successfully".to_string(),
                None,
            )),
            Ok(_) => Ok(NodeOperationResult::without_token(
                false,
                "Node not found".to_string(),
                None,
            )),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to remove node: {}", e),
                node: None,
                access_token: None,
                expires_at: None,
                permissions: None,
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
            Ok(_) => Ok(NodeOperationResult::without_token(
                true,
                "Heartbeat recorded".to_string(),
                None,
            )),
            Err(e) => Ok(NodeOperationResult {
                success: false,
                message: format!("Failed to record heartbeat: {}", e),
                node: None,
                access_token: None,
                expires_at: None,
                permissions: None,
            }),
        }
    }
}
