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
    let dirs: Vec<String> = state.config.directories.iter().map(|d| d.label.clone()).collect();
    Json(NodeInfo {
        id: state.node_identity.id.clone(),
        name: state.node_identity.name.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        directories: dirs,
    })
}

async fn list_directories(State(state): State<HttpState>) -> Json<Vec<String>> {
    let dirs: Vec<String> = state.config.directories.iter().map(|d| d.label.clone()).collect();
    Json(dirs)
}

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/api/v1/node", get(get_node))
        .route("/api/v1/directories", get(list_directories))
}
