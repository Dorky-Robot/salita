// V2 Pairing Handlers - Uses domain model + repository pattern
// This is the refactored version that pushes side effects to the edges

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::pairing::{
    IpAddress, JoinToken, NodeId, PairingCoordinator, PairingError, PairingRepository, PeerToken,
    Pin, SessionToken, SqlitePairingRepository,
};
use crate::state::AppState;

const PAIRING_TTL_SECS: u64 = 300; // 5 minutes

// -- Request/Response types --

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPairingRequest {
    pub created_by_node_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPairingResponse {
    pub token: String,
    pub expires_at: String,
    pub qr_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectDeviceRequest {
    pub token: String,
    pub device_ip: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectDeviceResponse {
    pub pin: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPinRequest {
    pub token: String,
    pub pin: String,
    pub device_node_id: String,
    pub device_name: String,
    pub device_port: u16,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPinResponse {
    pub success: bool,
    pub message: String,
    pub session_token: Option<String>,
    pub peer_token: Option<String>,
    pub peer_token_expires_at: Option<String>,
    pub permissions: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingStatusRequest {
    pub token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingStatusResponse {
    pub state: String,
    pub expired: bool,
    pub failed: bool,
    pub failure_reason: Option<String>,
}

// -- Error conversion --

impl From<PairingError> for AppError {
    fn from(err: PairingError) -> Self {
        match err {
            PairingError::InvalidTransition(msg) => AppError::BadRequest(msg),
            PairingError::Expired(msg) => AppError::BadRequest(msg),
            PairingError::PinMismatch => AppError::BadRequest("Incorrect PIN".into()),
            PairingError::MissingNodeId => {
                AppError::BadRequest("Device node ID is required".into())
            }
            PairingError::TokenExpired => AppError::BadRequest("Pairing token expired".into()),
            PairingError::InvalidPin => AppError::BadRequest("Invalid PIN".into()),
            PairingError::DeviceAlreadyRegistered => {
                AppError::BadRequest("Device already registered".into())
            }
            PairingError::IpConflict { existing_device } => AppError::BadRequest(format!(
                "IP conflict with existing device: {}",
                existing_device
            )),
        }
    }
}

// -- Handlers --

/// POST /auth/pair/start/v2
/// Creates a new pairing session
pub async fn start_pairing(
    State(state): State<AppState>,
    Json(req): Json<StartPairingRequest>,
) -> AppResult<Response> {
    let repo = SqlitePairingRepository::new(state.db.clone());

    let token = JoinToken::generate();
    let now = Utc::now();

    // Pure domain logic - create pairing state
    let pairing_state = PairingCoordinator::create_pairing(token.clone(), now, PAIRING_TTL_SECS);

    // Side effect - persist to database
    repo.save(&pairing_state)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to save pairing state: {}", e)))?;

    // Side effect - log event for audit trail
    repo.log_event(
        &token,
        "created",
        Some(format!(r#"{{"created_by": "{}"}}"#, req.created_by_node_id)),
    )
    .await
    .ok(); // Don't fail if logging fails

    // Build response
    let expires_at = pairing_state.expires_at().to_rfc3339();
    let qr_url = format!("/join?token={}", token.as_str());

    let response = StartPairingResponse {
        token: token.as_str().to_string(),
        expires_at,
        qr_url,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// POST /auth/pair/connect/v2
/// Device connects and receives a PIN
pub async fn connect_device(
    State(state): State<AppState>,
    Json(req): Json<ConnectDeviceRequest>,
) -> AppResult<Response> {
    let repo = SqlitePairingRepository::new(state.db.clone());

    let token = JoinToken::new(&req.token);
    let device_ip = IpAddress::new(&req.device_ip);
    let now = Utc::now();

    // Side effect - load state from database
    let current_state = repo
        .load(&token)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to load pairing state: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Pairing token not found".into()))?;

    // Pure domain logic - transition to connected state and generate PIN
    let (new_state, pin) = current_state
        .connect_device(device_ip, now)
        .map_err(AppError::from)?;

    // Side effect - persist updated state
    repo.save(&new_state)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to save pairing state: {}", e)))?;

    // Side effect - log event
    repo.log_event(
        &token,
        "connected",
        Some(format!(r#"{{"device_ip": "{}"}}"#, req.device_ip)),
    )
    .await
    .ok();

    let response = ConnectDeviceResponse {
        pin: pin.as_str().to_string(),
        expires_at: new_state.expires_at().to_rfc3339(),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// POST /auth/pair/verify/v2
/// Verify PIN and complete device registration
pub async fn verify_pin(
    State(state): State<AppState>,
    Json(req): Json<VerifyPinRequest>,
) -> AppResult<Response> {
    let repo = SqlitePairingRepository::new(state.db.clone());

    let token = JoinToken::new(&req.token);
    let pin = Pin::new(&req.pin);
    let node_id = NodeId::new(&req.device_node_id);
    let device_ip = IpAddress::new("0.0.0.0"); // Placeholder, will be updated
    let now = Utc::now();

    // Side effect - load state
    let current_state = repo
        .load(&token)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to load pairing state: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Pairing token not found".into()))?;

    // Ensure we have device_node_id in state
    let current_state = if current_state.device_node_id().is_none() {
        // Update state with node_id
        current_state
            .set_device_node_id(node_id.clone())
            .map_err(AppError::from)?
    } else {
        current_state
    };

    // Pure domain logic - verify PIN and generate session token
    let session_token = SessionToken::generate();
    let verified_state = current_state
        .verify_pin(&pin, session_token.clone(), now)
        .map_err(|e| {
            // Log failed verification attempt
            let _ = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    repo.log_event(&token, "pin_verification_failed", Some(e.to_string()))
                        .await
                })
            });
            AppError::from(e)
        })?;

    // Pure domain logic - generate peer token and register device
    let peer_token = PeerToken::generate();
    let registered_state = verified_state
        .register_device(peer_token.clone())
        .map_err(AppError::from)?;

    // Side effect - atomic registration of node + session + peer token
    let session_expires_at = Utc::now() + chrono::Duration::hours(24);
    repo.register_node_atomic(
        &node_id,
        &req.device_name,
        &device_ip,
        req.device_port,
        &session_token,
        session_expires_at,
        &peer_token,
    )
    .await
    .map_err(|e| AppError::Internal(format!("Failed to register device: {}", e)))?;

    // Side effect - save final state
    repo.save(&registered_state)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to save pairing state: {}", e)))?;

    // Side effect - log success event
    repo.log_event(
        &token,
        "registered",
        Some(format!(
            r#"{{"node_id": "{}", "name": "{}"}}"#,
            node_id.as_str(),
            req.device_name
        )),
    )
    .await
    .ok();

    let peer_token_expires_at = Utc::now() + chrono::Duration::days(30);
    let permissions = vec![
        "posts:read".to_string(),
        "posts:create".to_string(),
        "media:read".to_string(),
        "media:upload".to_string(),
        "comments:create".to_string(),
    ];

    let response = VerifyPinResponse {
        success: true,
        message: "Device registered successfully".to_string(),
        session_token: Some(session_token.as_str().to_string()),
        peer_token: Some(peer_token.as_str().to_string()),
        peer_token_expires_at: Some(peer_token_expires_at.to_rfc3339()),
        permissions: Some(permissions),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// GET /auth/pair/status/v2?token=xxx
/// Check pairing status
pub async fn pairing_status(
    State(state): State<AppState>,
    axum::extract::Query(req): axum::extract::Query<PairingStatusRequest>,
) -> AppResult<Response> {
    let repo = SqlitePairingRepository::new(state.db.clone());

    let token = JoinToken::new(&req.token);
    let now = Utc::now();

    // Side effect - load state
    let current_state = repo
        .load(&token)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to load pairing state: {}", e)))?
        .ok_or_else(|| AppError::BadRequest("Pairing token not found".into()))?;

    let state_name = current_state.state_name();
    let expired = current_state.is_expired(now);
    let failed = current_state.is_failed();
    let failure_reason = current_state.failure_reason();

    let response = PairingStatusResponse {
        state: state_name.to_string(),
        expired,
        failed,
        failure_reason: failure_reason.map(|s| s.to_string()),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

    fn create_test_state() -> (AppState, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let pool = db::create_pool(&db_path).unwrap();
        db::run_migrations(&pool).unwrap();

        // Create a minimal AppState for testing
        let state = AppState {
            db: pool,
            config: crate::config::Config::default(),
            data_dir: temp_dir.path().to_path_buf(),
            webauthn: std::sync::Arc::new(
                crate::auth::webauthn::build_webauthn("https://localhost:6969").unwrap(),
            ),
            ceremonies: std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::auth::webauthn::CeremonyStore::new(),
            )),
            join_tokens: std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::auth::join_tokens::JoinTokenStore::new(),
            )),
            graphql_schema: crate::graphql::schema::build_schema(),
        };

        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_start_pairing() {
        let (state, _temp) = create_test_state();

        let req = StartPairingRequest {
            created_by_node_id: "test-node".to_string(),
        };

        let response = start_pairing(State(state), Json(req)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_connect_device() {
        let (state, _temp) = create_test_state();

        // First create a pairing
        let start_req = StartPairingRequest {
            created_by_node_id: "test-node".to_string(),
        };
        let start_response = start_pairing(State(state.clone()), Json(start_req))
            .await
            .unwrap();

        // Extract token from response (this is a bit hacky for testing)
        // In real tests, we'd parse the JSON response properly
        let repo = SqlitePairingRepository::new(state.db.clone());
        let token = JoinToken::generate();
        let pairing_state =
            PairingCoordinator::create_pairing(token.clone(), Utc::now(), PAIRING_TTL_SECS);
        repo.save(&pairing_state).await.unwrap();

        // Now connect a device
        let connect_req = ConnectDeviceRequest {
            token: token.as_str().to_string(),
            device_ip: "192.168.1.100".to_string(),
        };

        let response = connect_device(State(state), Json(connect_req)).await;
        assert!(response.is_ok());
    }

    #[tokio::test]
    async fn test_full_pairing_flow() {
        let (state, _temp) = create_test_state();
        let repo = SqlitePairingRepository::new(state.db.clone());

        // 1. Create pairing
        let token = JoinToken::generate();
        let pairing_state =
            PairingCoordinator::create_pairing(token.clone(), Utc::now(), PAIRING_TTL_SECS);
        repo.save(&pairing_state).await.unwrap();

        // 2. Connect device
        let (connected_state, pin) = pairing_state
            .connect_device(IpAddress::new("192.168.1.100"), Utc::now())
            .unwrap();
        repo.save(&connected_state).await.unwrap();

        // 3. Verify PIN
        let verify_req = VerifyPinRequest {
            token: token.as_str().to_string(),
            pin: pin.as_str().to_string(),
            device_node_id: "test-device-123".to_string(),
            device_name: "Test Device".to_string(),
            device_port: 6969,
        };

        let response = verify_pin(State(state), Json(verify_req)).await;
        assert!(response.is_ok());
    }
}
