use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use qrcode::render::svg;
use qrcode::QrCode;

use crate::error::AppResult;
use crate::routes::home::Html;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "components/connect_modal.html")]
pub struct ConnectModalTemplate {
    pub qr_svg: String,
    pub lan_url: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/connect/qr", get(qr_fragment))
}

async fn qr_fragment(State(state): State<AppState>) -> AppResult<Response> {
    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());

    let lan_url = if state.config.tls_enabled() {
        format!(
            "http://{}:{}/connect/trust",
            lan_ip, state.config.tls.http_port
        )
    } else {
        format!("http://{}:{}", lan_ip, state.config.server.port)
    };

    let code = QrCode::new(lan_url.as_bytes()).map_err(|e| {
        tracing::error!("QR code generation failed: {}", e);
        crate::error::AppError::Internal("QR code generation failed".into())
    })?;

    let qr_svg = code
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .max_dimensions(300, 300)
        .dark_color(svg::Color("#1c1917"))
        .light_color(svg::Color("#ffffff"))
        .build();

    Ok(Html(ConnectModalTemplate { qr_svg, lan_url }).into_response())
}
