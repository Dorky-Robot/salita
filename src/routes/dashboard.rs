use askama::Template;
use axum::extract::{ConnectInfo, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use futures::stream::{self, Stream};
use serde::Deserialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;
use tokio_stream::StreamExt as _;

use crate::error::{AppError, AppResult};
use crate::extractors::CurrentUser;
use crate::routes::home::Html;
use crate::state::AppState;

#[derive(Deserialize)]
struct JoinQuery {
    token: Option<String>,
}

#[derive(Template)]
#[template(path = "pages/dashboard.html")]
struct DashboardTemplate;

#[derive(Template)]
#[template(path = "components/add_device_modal.html")]
struct AddDeviceModalTemplate;

#[derive(Template)]
#[template(path = "components/join_modal.html")]
struct JoinModalTemplate {
    join_url: String,
    lan_ip: String,
}

#[derive(Template)]
#[template(path = "pages/join_mesh.html")]
struct JoinMeshTemplate {
    local_ip: String,
    pin: String,
}

/// Dashboard page showing all mesh nodes
async fn dashboard(_user: CurrentUser) -> AppResult<impl IntoResponse> {
    let template = DashboardTemplate;
    Ok(Html(template))
}

/// Add device modal
async fn add_device_modal(_user: CurrentUser) -> AppResult<impl IntoResponse> {
    let template = AddDeviceModalTemplate;
    Ok(Html(template))
}

/// Join modal - unified QR and manual flow
async fn join_modal(State(state): State<AppState>) -> AppResult<impl IntoResponse> {
    // Generate ephemeral join token (5 min TTL, single-use)
    let token = {
        let mut join_tokens = state.join_tokens.lock().await;
        join_tokens.generate("current-node".to_string())
    };

    // Get LAN IP for the join URL
    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "192.168.1.x".to_string());

    // Include token in URL for security
    // Use HTTPS - mobile must trust cert first via Step 1
    let join_url = format!(
        "https://{}:{}/join?token={}",
        lan_ip, state.config.server.port, token
    );

    let template = JoinModalTemplate {
        join_url,
        lan_ip,
    };
    Ok(Html(template))
}

/// Join mesh page for unpaired nodes
/// Shows PIN and instructions for adding this device to a mesh
/// Requires valid ephemeral token for security
async fn join_mesh(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<JoinQuery>,
) -> AppResult<impl IntoResponse> {
    // Validate token (required, single-use, 5-min TTL)
    let token = query
        .token
        .ok_or_else(|| AppError::BadRequest("Join token required".into()))?;

    // Get device IP from socket address
    let device_ip = addr.ip().to_string();

    // Use token (marks as used, validates expiry, stores device IP, generates PIN)
    let join_token = {
        let mut join_tokens = state.join_tokens.lock().await;
        join_tokens.use_token(&token, device_ip).ok_or_else(|| {
            AppError::BadRequest("Invalid or expired join token. Please generate a new one.".into())
        })?
    };

    // Extract PIN to show on device
    let pin = join_token
        .pin
        .clone()
        .unwrap_or_else(|| "000000".to_string());

    // Get local IP address
    let local_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "192.168.1.x".to_string());

    let template = JoinMeshTemplate { local_ip, pin };
    Ok(Html(template))
}

/// Verify PIN for a join token
#[derive(Deserialize)]
struct VerifyPinRequest {
    token: String,
    pin: String,
}

async fn verify_join_pin(
    State(state): State<AppState>,
    axum::Json(request): axum::Json<VerifyPinRequest>,
) -> AppResult<axum::Json<serde_json::Value>> {
    tracing::info!("Verifying PIN - token: {}, pin: {}", request.token, request.pin);

    let is_valid = {
        let join_tokens = state.join_tokens.lock().await;

        // Debug: log what we have stored
        if let Some(stored_token) = join_tokens.tokens.get(&request.token) {
            tracing::info!("Found token - used: {}, stored_pin: {:?}",
                stored_token.used, stored_token.pin);
        } else {
            tracing::warn!("Token not found in store");
        }

        join_tokens.verify_pin(&request.token, &request.pin)
    };

    if is_valid {
        Ok(axum::Json(serde_json::json!({ "valid": true })))
    } else {
        Err(AppError::BadRequest("Invalid PIN".into()))
    }
}

/// SSE endpoint for join token events
/// Streams events when a join token is used
async fn join_events(
    State(state): State<AppState>,
    Query(query): Query<JoinQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, AppError> {
    let token = query
        .token
        .ok_or_else(|| AppError::BadRequest("Token required".into()))?;

    // Validate token exists (can be used or unused)
    {
        let join_tokens = state.join_tokens.lock().await;
        if !join_tokens.tokens.contains_key(&token) {
            return Err(AppError::BadRequest("Invalid token".into()));
        }
    }

    // Create a stream that checks token status periodically
    let stream = stream::unfold(
        (state.clone(), token.clone(), false),
        move |(state, token, mut sent)| async move {
            // Check if token has been used and get device IP
            let token_info = {
                let join_tokens = state.join_tokens.lock().await;
                join_tokens
                    .tokens
                    .get(&token)
                    .map(|t| (t.used, t.device_ip.clone()))
            };

            if let Some((is_used, device_ip)) = token_info {
                if is_used && !sent {
                    // Token was used! Send event with device IP
                    sent = true;
                    let ip = device_ip.unwrap_or_else(|| "Unknown".to_string());
                    let data = format!(r#"{{"device_ip":"{}","status":"connected"}}"#, ip);
                    let event = Event::default()
                        .event("token-used")
                        .data(data);

                    return Some((Ok(event), (state, token, sent)));
                }
            }

            // Check again in 500ms
            tokio::time::sleep(Duration::from_millis(500)).await;

            // Keep connection alive
            if !sent {
                let event = Event::default().comment("keep-alive");
                Some((Ok(event), (state, token, sent)))
            } else {
                None // Close stream after sending event
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Dashboard router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/dashboard", get(dashboard))
        .route("/dashboard/add-device-modal", get(add_device_modal))
        .route("/mesh/join-modal", get(join_modal))
        .route("/mesh/join-events", get(join_events))
        .route("/mesh/verify-join-pin", post(verify_join_pin))
        .route("/join", get(join_mesh))
}
