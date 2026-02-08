use askama::Template;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{extract::State, Router};

use crate::error::{AppError, AppResult};
use crate::routes::home::Html;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "pages/trust.html")]
pub struct TrustTemplate {
    pub https_url: String,
    pub instance_name: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/connect/trust", get(trust_page))
        .route("/connect/trust/ca.mobileconfig", get(download_mobileconfig))
        .route("/connect/trust/ca.crt", get(download_ca_cert))
}

async fn trust_page(State(state): State<AppState>) -> AppResult<Response> {
    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let https_url = format!("https://{}:{}", lan_ip, state.config.server.port);

    let instance_name = state.config.instance_name.clone();

    Ok(Html(TrustTemplate {
        https_url,
        instance_name,
    })
    .into_response())
}

async fn download_mobileconfig(State(state): State<AppState>) -> AppResult<Response> {
    let paths = crate::tls::TlsPaths::new(&state.data_dir);

    let mobileconfig =
        crate::tls::generate_mobileconfig(&paths.ca_cert, &state.config.instance_name)
            .map_err(|e| {
        tracing::error!("Failed to generate mobileconfig: {}", e);
        AppError::Internal("Failed to generate mobileconfig".into())
    })?;

    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/x-apple-aspen-config".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"salita.mobileconfig\"".to_string(),
            ),
        ],
        mobileconfig,
    )
        .into_response())
}

async fn download_ca_cert(State(state): State<AppState>) -> AppResult<Response> {
    let paths = crate::tls::TlsPaths::new(&state.data_dir);

    let ca_bytes = std::fs::read(&paths.ca_cert).map_err(|e| {
        tracing::error!("Failed to read CA certificate: {}", e);
        AppError::Internal("Failed to read CA certificate".into())
    })?;

    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/x-x509-ca-cert".to_string(),
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"salita-ca.crt\"".to_string(),
            ),
        ],
        ca_bytes,
    )
        .into_response())
}
