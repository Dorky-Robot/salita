use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::HttpState;

#[derive(Serialize)]
struct NodeInfo {
    id: String,
    name: String,
    version: String,
    directories: Vec<String>,
}

async fn get_node(State(state): State<HttpState>) -> Json<NodeInfo> {
    let dirs: Vec<String> = state
        .config
        .directories
        .iter()
        .map(|d| d.label.clone())
        .collect();
    Json(NodeInfo {
        id: state.node_identity.id.clone(),
        name: state.node_identity.name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        directories: dirs,
    })
}

async fn list_directories(State(state): State<HttpState>) -> Json<Vec<String>> {
    let dirs: Vec<String> = state
        .config
        .directories
        .iter()
        .map(|d| d.label.clone())
        .collect();
    Json(dirs)
}

async fn list_devices(
    State(state): State<HttpState>,
) -> Result<Json<Vec<serde_json::Value>>, crate::error::AppError> {
    let conn = state.db.get().map_err(|e| {
        crate::error::AppError::Internal(format!("Database error: {}", e))
    })?;

    let mut stmt = conn
        .prepare("SELECT id, name, endpoint, port, is_self, status, last_seen FROM devices")
        .map_err(|e| crate::error::AppError::Internal(format!("Query error: {}", e)))?;

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
        .map_err(|e| crate::error::AppError::Internal(format!("Query error: {}", e)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(devices))
}

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/api/v1/node", get(get_node))
        .route("/api/v1/directories", get(list_directories))
        .route("/api/v1/devices", get(list_devices))
}
