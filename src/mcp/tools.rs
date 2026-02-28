use rmcp::ErrorData as McpError;
use rmcp::model::*;
use rusqlite::params;

use crate::db::DbPool;
use crate::peer_client::PeerClient;

use super::types::*;
use super::SalitaMcp;

/// Lookup device info from the database. Returns (is_self, endpoint, port).
fn lookup_device(
    pool: &DbPool,
    device: &str,
) -> Result<(bool, String, u16), McpError> {
    let conn = pool.get().map_err(|e| {
        McpError::internal_error(format!("Database error: {}", e), None)
    })?;

    let result = conn.query_row(
        "SELECT is_self, endpoint, port FROM devices WHERE id = ?1 OR name = ?1",
        params![device],
        |row| {
            Ok((
                row.get::<_, bool>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u16>(2)?,
            ))
        },
    );

    match result {
        Ok(r) => Ok(r),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(McpError::invalid_params(format!("Device not found: {}", device), None))
        }
        Err(e) => {
            Err(McpError::internal_error(format!("Database error: {}", e), None))
        }
    }
}

/// Check if a request targets a remote device, returning (endpoint, port) if so
fn remote_target(
    pool: &DbPool,
    device: &Option<String>,
) -> Result<Option<(String, u16)>, McpError> {
    match device {
        None => Ok(None),
        Some(dev) => {
            let (is_self, endpoint, port) = lookup_device(pool, dev)?;
            if is_self {
                Ok(None)
            } else {
                Ok(Some((endpoint, port)))
            }
        }
    }
}

impl SalitaMcp {
    pub(crate) fn list_devices_impl(&self) -> Result<CallToolResult, McpError> {
        let conn = self.pool.get().map_err(|e| {
            McpError::internal_error(format!("Database error: {}", e), None)
        })?;

        let mut stmt = conn
            .prepare("SELECT id, name, endpoint, port, is_self, status, last_seen FROM devices")
            .map_err(|e| McpError::internal_error(format!("Query error: {}", e), None))?;

        let devices: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "endpoint": row.get::<_, Option<String>>(2)?,
                    "port": row.get::<_, i64>(3)?,
                    "is_self": row.get::<_, bool>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "last_seen": row.get::<_, Option<String>>(6)?,
                }))
            })
            .map_err(|e| McpError::internal_error(format!("Query error: {}", e), None))?
            .filter_map(|r| r.ok())
            .collect();

        let text = serde_json::to_string_pretty(&devices)
            .unwrap_or_else(|_| "[]".to_string());

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    pub(crate) fn list_files_impl(
        &self,
        params: ListFilesParams,
    ) -> Result<CallToolResult, McpError> {
        let path = params.path.as_deref().unwrap_or("");

        if let Some((endpoint, port)) = remote_target(&self.pool, &params.device)? {
            let client = PeerClient::new();
            let entries = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    client.list_files(&endpoint, port, &params.directory, path),
                )
            })
            .map_err(|e| McpError::internal_error(format!("Peer error: {}", e), None))?;

            let text = serde_json::to_string_pretty(&entries)
                .unwrap_or_else(|_| "[]".to_string());
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        let entries = crate::files::list_files(&self.config, &params.directory, path)
            .map_err(|e| McpError::internal_error(format!("{}", e), None))?;

        let text = serde_json::to_string_pretty(&entries)
            .unwrap_or_else(|_| "[]".to_string());

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    pub(crate) fn search_files_impl(
        &self,
        params: SearchFilesParams,
    ) -> Result<CallToolResult, McpError> {
        if let Some((endpoint, port)) = remote_target(&self.pool, &params.device)? {
            let client = PeerClient::new();
            let entries = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    client.search_files(&endpoint, port, &params.pattern, params.directory.as_deref()),
                )
            })
            .map_err(|e| McpError::internal_error(format!("Peer error: {}", e), None))?;

            let text = serde_json::to_string_pretty(&entries)
                .unwrap_or_else(|_| "[]".to_string());
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        let entries = crate::files::search_files(
            &self.config,
            &params.pattern,
            params.directory.as_deref(),
        )
        .map_err(|e| McpError::internal_error(format!("{}", e), None))?;

        let text = serde_json::to_string_pretty(&entries)
            .unwrap_or_else(|_| "[]".to_string());

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    pub(crate) fn read_file_impl(
        &self,
        params: ReadFileParams,
    ) -> Result<CallToolResult, McpError> {
        if let Some((endpoint, port)) = remote_target(&self.pool, &params.device)? {
            let client = PeerClient::new();
            let content = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    client.read_file(&endpoint, port, &params.directory, &params.path),
                )
            })
            .map_err(|e| McpError::internal_error(format!("Peer error: {}", e), None))?;

            return Ok(CallToolResult::success(vec![Content::text(content)]));
        }

        let content = crate::files::read_file(&self.config, &params.directory, &params.path)
            .map_err(|e| McpError::internal_error(format!("{}", e), None))?;

        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    pub(crate) fn file_info_impl(
        &self,
        params: FileInfoParams,
    ) -> Result<CallToolResult, McpError> {
        if let Some((endpoint, port)) = remote_target(&self.pool, &params.device)? {
            let client = PeerClient::new();
            let info = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    client.file_info(&endpoint, port, &params.directory, &params.path),
                )
            })
            .map_err(|e| McpError::internal_error(format!("Peer error: {}", e), None))?;

            let text = serde_json::to_string_pretty(&info)
                .unwrap_or_else(|_| "{}".to_string());
            return Ok(CallToolResult::success(vec![Content::text(text)]));
        }

        let info = crate::files::file_info(&self.config, &params.directory, &params.path)
            .map_err(|e| McpError::internal_error(format!("{}", e), None))?;

        let text = serde_json::to_string_pretty(&info)
            .unwrap_or_else(|_| "{}".to_string());

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}
