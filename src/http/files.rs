use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use super::HttpState;
use crate::error::AppResult;
use crate::files;

#[derive(Deserialize)]
struct ListParams {
    dir: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct SearchParams {
    pattern: String,
    dir: Option<String>,
}

#[derive(Deserialize)]
struct FileParams {
    dir: String,
    path: String,
}

async fn list_files_handler(
    State(state): State<HttpState>,
    Query(params): Query<ListParams>,
) -> AppResult<Json<Vec<files::FileEntry>>> {
    let entries = files::list_files(&state.config, &params.dir, &params.path)?;
    Ok(Json(entries))
}

async fn search_files_handler(
    State(state): State<HttpState>,
    Query(params): Query<SearchParams>,
) -> AppResult<Json<Vec<files::FileEntry>>> {
    let entries = files::search_files(&state.config, &params.pattern, params.dir.as_deref())?;
    Ok(Json(entries))
}

async fn read_file_handler(
    State(state): State<HttpState>,
    Query(params): Query<FileParams>,
) -> AppResult<Response> {
    let bytes = files::read_file_bytes(&state.config, &params.dir, &params.path)?;

    let file_path = std::path::Path::new(&params.path);
    let content_type = mime_guess::from_path(file_path)
        .first_or_octet_stream()
        .to_string();

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("inline; filename=\"{}\"", filename),
        )
        .body(Body::from(bytes))
        .unwrap())
}

async fn file_info_handler(
    State(state): State<HttpState>,
    Query(params): Query<FileParams>,
) -> AppResult<Json<files::FileInfo>> {
    let info = files::file_info(&state.config, &params.dir, &params.path)?;
    Ok(Json(info))
}

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/api/v1/files", get(list_files_handler))
        .route("/api/v1/files/search", get(search_files_handler))
        .route("/api/v1/files/read", get(read_file_handler))
        .route("/api/v1/files/info", get(file_info_handler))
}
