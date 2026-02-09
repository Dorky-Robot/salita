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

    let template = JoinModalTemplate { join_url, lan_ip };
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

    // Get local IP address (server's IP for display purposes)
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
) -> AppResult<impl axum::response::IntoResponse> {
    tracing::info!(
        "Verifying PIN - token: {}, pin: {}",
        request.token,
        request.pin
    );

    // Get device info and verify PIN
    let device_info = {
        let join_tokens = state.join_tokens.lock().await;

        // Debug: log what we have stored
        if let Some(stored_token) = join_tokens.tokens.get(&request.token) {
            tracing::info!(
                "Found token - used: {}, stored_pin: {:?}",
                stored_token.used,
                stored_token.pin
            );
        } else {
            tracing::warn!("Token not found in store");
        }

        let is_valid = join_tokens.verify_pin(&request.token, &request.pin);
        if !is_valid {
            return Err(AppError::BadRequest("Invalid PIN".into()));
        }

        // Get device info from token
        join_tokens.tokens.get(&request.token).cloned()
    };

    let _device_info = device_info.ok_or_else(|| AppError::BadRequest("Token not found".into()))?;

    tracing::info!("PIN verified successfully for device");

    // NOTE: Device will be registered by the join modal's GraphQL mutation
    // We only create the session here so the phone can authenticate

    let conn = state
        .db
        .get()
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Create or get default user
    let user_id = conn
        .query_row(
            "SELECT id FROM users WHERE username = 'default' LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| {
            // Create default user
            let uid = uuid::Uuid::now_v7().to_string();
            tracing::info!("Creating default user with id: {}", uid);
            conn.execute(
                "INSERT INTO users (id, username, is_admin) VALUES (?1, 'default', 1)",
                rusqlite::params![&uid],
            )
            .ok();
            uid
        });

    tracing::info!("Using user_id: {}", user_id);

    // Create session
    let session_token =
        crate::auth::session::create_session(&state.db, &user_id, state.config.auth.session_hours)
            .map_err(|e| AppError::Internal(format!("Failed to create session: {}", e)))?;

    tracing::info!("Created session token: {}", &session_token[..16]);

    // Store session token in join token so phone can claim it
    {
        let mut join_tokens = state.join_tokens.lock().await;
        if let Some(join_token) = join_tokens.tokens.get_mut(&request.token) {
            join_token.session_token = Some(session_token.clone());
            tracing::info!("Stored session token in join token for phone to claim");
        }
    }

    // Return success (desktop doesn't need the cookie)
    tracing::info!("PIN verification complete");

    Ok(axum::Json(serde_json::json!({ "valid": true })))
}

/// Issue a peer token to another device (for bidirectional auth)
#[derive(Deserialize)]
struct IssuePeerTokenRequest {
    peer_node_id: String,
    peer_name: String,
}

#[derive(serde::Serialize)]
struct IssuePeerTokenResponse {
    access_token: String,
    expires_at: String,
    permissions: Vec<String>,
    issuer_node_id: String,
}

async fn issue_peer_token(
    State(state): State<AppState>,
    axum::Json(request): axum::Json<IssuePeerTokenRequest>,
) -> AppResult<impl axum::response::IntoResponse> {
    use crate::mesh::tokens;
    use chrono::{Duration, Utc};

    tracing::info!("Issuing peer token to node: {}", request.peer_node_id);

    let conn = state
        .db
        .get()
        .map_err(|e| AppError::Internal(format!("Database error: {}", e)))?;

    // Get current node's ID
    let current_node_id: String = conn
        .query_row(
            "SELECT id FROM mesh_nodes WHERE is_current = 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .map_err(|e| AppError::Internal(format!("Failed to get current node: {}", e)))?;

    // Issue token
    let permissions = tokens::default_permissions();
    let expires_at = Utc::now() + Duration::days(30);
    let expires_at_str = expires_at.to_rfc3339();

    let token = tokens::issue_token(&conn, &request.peer_node_id, &permissions, &expires_at_str)
        .map_err(|e| AppError::Internal(format!("Failed to issue token: {}", e)))?;

    tracing::info!("Issued token to {}: {}", request.peer_name, &token[..16]);

    Ok(axum::Json(IssuePeerTokenResponse {
        access_token: token,
        expires_at: expires_at_str,
        permissions,
        issuer_node_id: current_node_id,
    }))
}

/// Update join token with device's node ID
/// The phone calls this to register its persistent node ID during pairing
#[derive(Deserialize)]
struct UpdateNodeIdRequest {
    token: String,
    node_id: String,
}

async fn update_node_id(
    State(state): State<AppState>,
    axum::Json(request): axum::Json<UpdateNodeIdRequest>,
) -> AppResult<impl axum::response::IntoResponse> {
    tracing::info!(
        "Phone sending node_id: {} for token: {}",
        request.node_id,
        request.token
    );

    // Store node_id in join token
    {
        let mut join_tokens = state.join_tokens.lock().await;
        if let Some(join_token) = join_tokens.tokens.get_mut(&request.token) {
            join_token.device_node_id = Some(request.node_id);
            tracing::info!("Stored device node_id in join token");
        } else {
            return Err(AppError::BadRequest("Invalid token".into()));
        }
    }

    Ok(axum::Json(serde_json::json!({ "success": true })))
}

/// Claim session cookie after PIN verification
/// The phone calls this to get its session after desktop verifies the PIN
async fn claim_session(
    State(state): State<AppState>,
    Query(query): Query<JoinQuery>,
) -> AppResult<impl axum::response::IntoResponse> {
    use axum::http::header;

    let token = query
        .token
        .ok_or_else(|| AppError::BadRequest("Token required".into()))?;

    tracing::info!("Phone claiming session for token: {}", token);

    // Get session token from join token
    let session_token = {
        let join_tokens = state.join_tokens.lock().await;
        let join_token = join_tokens
            .tokens
            .get(&token)
            .ok_or_else(|| AppError::BadRequest("Invalid token".into()))?;

        // Check if PIN was verified (device_ip should be set and used should be true)
        if !join_token.used || join_token.device_ip.is_none() {
            tracing::warn!("Token not verified yet");
            return Err(AppError::BadRequest("Token not verified yet".into()));
        }

        // Get session token
        join_token
            .session_token
            .clone()
            .ok_or_else(|| AppError::BadRequest("Session not ready yet".into()))?
    };

    tracing::info!("Returning session cookie to phone");

    // Set session cookie
    let cookie = format!(
        "salita_session={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        session_token,
        state.config.auth.session_hours * 3600
    );

    Ok((
        [(header::SET_COOKIE, cookie)],
        axum::Json(serde_json::json!({ "success": true })),
    ))
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
            // Check if token has been used and get device info
            let token_info = {
                let join_tokens = state.join_tokens.lock().await;
                join_tokens
                    .tokens
                    .get(&token)
                    .map(|t| (t.used, t.device_ip.clone(), t.device_node_id.clone()))
            };

            if let Some((is_used, device_ip, device_node_id)) = token_info {
                if is_used && !sent {
                    // Token was used! Send event with device IP and node ID
                    sent = true;
                    let ip = device_ip.unwrap_or_else(|| "Unknown".to_string());
                    let node_id = device_node_id.unwrap_or_else(|| "".to_string());
                    let data = format!(
                        r#"{{"device_ip":"{}","device_node_id":"{}","status":"connected"}}"#,
                        ip, node_id
                    );
                    let event = Event::default().event("token-used").data(data);

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
        .route("/mesh/update-node-id", post(update_node_id))
        .route("/mesh/issue-peer-token", post(issue_peer_token))
        .route("/mesh/claim-session", get(claim_session))
        .route("/join", get(join_mesh))
}
