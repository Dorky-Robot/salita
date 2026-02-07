use askama::Template;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use axum::Json;
use rusqlite::params;
use serde::Deserialize;
use webauthn_rs::prelude::*;

use crate::auth::session;
use crate::auth::webauthn::PendingRegistration;
use crate::error::{AppError, AppResult};
use crate::routes::home::Html;
use crate::state::AppState;

// -- Templates --

#[derive(Template)]
#[template(path = "pages/setup.html")]
pub struct SetupTemplate;

#[derive(Template)]
#[template(path = "pages/login.html")]
pub struct LoginTemplate;

#[derive(Template)]
#[template(path = "pages/pair.html")]
pub struct PairTemplate;

// -- Request types --

#[derive(Deserialize)]
pub struct SetupStartRequest {
    pub username: String,
    pub display_name: Option<String>,
}

// -- Cookie helpers --

fn session_cookie(token: &str, max_age_hours: u64) -> String {
    let max_age_secs = max_age_hours * 3600;
    format!(
        "salita_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        token, max_age_secs
    )
}

fn ceremony_cookie(ceremony_id: &str) -> String {
    format!(
        "salita_ceremony={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=300",
        ceremony_id
    )
}

fn clear_session_cookie() -> String {
    "salita_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0".to_string()
}

fn clear_ceremony_cookie() -> String {
    "salita_ceremony=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0".to_string()
}

fn get_cookie_value<'a>(parts: &'a axum::http::request::Parts, name: &str) -> Option<&'a str> {
    parts
        .headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(';'))
        .map(|s| s.trim())
        .find_map(|cookie| {
            let mut split = cookie.splitn(2, '=');
            let key = split.next()?.trim();
            let val = split.next()?.trim();
            if key == name {
                Some(val)
            } else {
                None
            }
        })
}

// -- Setup handlers --

/// GET /auth/setup — render setup page (only if no users exist)
pub async fn setup_page(State(state): State<AppState>) -> AppResult<Response> {
    let conn = state.db.get()?;
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;

    if user_count > 0 {
        return Ok(Redirect::to("/").into_response());
    }

    Ok(Html(SetupTemplate).into_response())
}

/// POST /auth/setup/start — begin passkey registration ceremony
pub async fn setup_start(
    State(state): State<AppState>,
    Json(req): Json<SetupStartRequest>,
) -> AppResult<Response> {
    let conn = state.db.get()?;
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;

    if user_count > 0 {
        return Err(AppError::BadRequest("Admin account already exists".into()));
    }

    let username = req.username.trim().to_string();
    if username.is_empty() {
        return Err(AppError::BadRequest("Username is required".into()));
    }

    let display_name = req
        .display_name
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(&username)
        .to_string();

    // Generate a user ID (will be stored when registration completes)
    let user_id = uuid::Uuid::now_v7();

    // Start WebAuthn registration
    let (ccr, reg_state) = state.webauthn.start_passkey_registration(
        user_id,
        &username,
        &display_name,
        None, // no existing credentials to exclude
    )?;

    // Store ceremony state + user metadata together in the CeremonyStore.
    // Only the plain ceremony ID goes into the cookie (no JSON — raw JSON
    // characters are invalid in cookie values per RFC 6265).
    let ceremony_id = uuid::Uuid::now_v7().to_string();
    {
        let mut ceremonies = state.ceremonies.lock().await;
        ceremonies.insert_registration(
            ceremony_id.clone(),
            PendingRegistration {
                reg_state,
                user_id: user_id.to_string(),
                username,
                display_name,
            },
        );
    }

    let ceremony_cookie_val = ceremony_cookie(&ceremony_id);

    let body = serde_json::to_string(&ccr)?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::SET_COOKIE, ceremony_cookie_val),
        ],
        body,
    )
        .into_response())
}

/// POST /auth/setup/finish — complete passkey registration
pub async fn setup_finish(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> AppResult<Response> {
    let (parts, body) = request.into_parts();

    // The cookie holds only the plain ceremony ID string.
    let ceremony_id = get_cookie_value(&parts, "salita_ceremony")
        .ok_or_else(|| AppError::BadRequest("Missing ceremony cookie".into()))?;

    // Retrieve ceremony state + user metadata from the in-memory store.
    let pending = {
        let mut ceremonies = state.ceremonies.lock().await;
        ceremonies
            .take_registration(ceremony_id)
            .ok_or_else(|| AppError::BadRequest("Ceremony expired or not found".into()))?
    };

    let PendingRegistration {
        reg_state,
        user_id,
        username,
        display_name,
    } = pending;

    // Parse the credential response from body
    let body_bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .map_err(|_| AppError::BadRequest("Invalid request body".into()))?;
    let reg_response: RegisterPublicKeyCredential = serde_json::from_slice(&body_bytes)?;

    // Finish registration
    let passkey = state
        .webauthn
        .finish_passkey_registration(&reg_response, &reg_state)?;

    // Store user and passkey in DB
    let passkey_json = serde_json::to_string(&passkey)?;
    let passkey_id = uuid::Uuid::now_v7().to_string();

    let conn = state.db.get()?;
    conn.execute(
        "INSERT INTO users (id, username, display_name, is_admin) VALUES (?1, ?2, ?3, 1)",
        params![user_id, username, display_name],
    )?;
    conn.execute(
        "INSERT INTO passkey_credentials (id, user_id, passkey_json) VALUES (?1, ?2, ?3)",
        params![passkey_id, user_id, passkey_json],
    )?;

    // Create session
    let token = session::create_session(&state.db, &user_id, state.config.auth.session_hours)?;

    let body = serde_json::json!({ "status": "ok" });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json".to_string())],
        AppendHeaders([
            (
                header::SET_COOKIE,
                session_cookie(&token, state.config.auth.session_hours),
            ),
            (header::SET_COOKIE, clear_ceremony_cookie()),
        ]),
        serde_json::to_string(&body)?,
    )
        .into_response())
}

// -- Login handlers --

/// GET /auth/login — render login page
pub async fn login_page() -> Html<LoginTemplate> {
    Html(LoginTemplate)
}

/// POST /auth/login/start — begin passkey authentication ceremony
pub async fn login_start(State(state): State<AppState>) -> AppResult<Response> {
    // Load all passkeys from DB (scoped so conn is dropped before await)
    let (rcr, auth_state) = {
        let conn = state.db.get()?;
        let mut stmt = conn.prepare("SELECT passkey_json FROM passkey_credentials")?;
        let passkeys: Vec<Passkey> = stmt
            .query_map([], |row| {
                let json: String = row.get(0)?;
                Ok(json)
            })?
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str(&json).ok())
            .collect();

        if passkeys.is_empty() {
            return Err(AppError::BadRequest("No passkeys registered".into()));
        }

        state.webauthn.start_passkey_authentication(&passkeys)?
    };

    let ceremony_id = uuid::Uuid::now_v7().to_string();
    {
        let mut ceremonies = state.ceremonies.lock().await;
        ceremonies.insert_authentication(ceremony_id.clone(), auth_state);
    }

    let body = serde_json::to_string(&rcr)?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json".to_string()),
            (header::SET_COOKIE, ceremony_cookie(&ceremony_id)),
        ],
        body,
    )
        .into_response())
}

/// POST /auth/login/finish — complete passkey authentication
pub async fn login_finish(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> AppResult<Response> {
    let (parts, body) = request.into_parts();

    let ceremony_id = get_cookie_value(&parts, "salita_ceremony")
        .ok_or_else(|| AppError::BadRequest("Missing ceremony cookie".into()))?;

    let auth_state = {
        let mut ceremonies = state.ceremonies.lock().await;
        ceremonies
            .take_authentication(ceremony_id)
            .ok_or_else(|| AppError::BadRequest("Ceremony expired or not found".into()))?
    };

    let body_bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .map_err(|_| AppError::BadRequest("Invalid request body".into()))?;
    let auth_response: PublicKeyCredential = serde_json::from_slice(&body_bytes)?;

    let auth_result = state
        .webauthn
        .finish_passkey_authentication(&auth_response, &auth_state)?;

    // Find and update the passkey that was used
    let conn = state.db.get()?;
    let mut stmt = conn.prepare("SELECT id, user_id, passkey_json FROM passkey_credentials")?;
    let rows: Vec<(String, String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut matched_user_id = None;
    for (cred_id, user_id, passkey_json) in &rows {
        let mut passkey: Passkey = serde_json::from_str(passkey_json)
            .map_err(|e| AppError::Internal(format!("Failed to parse passkey: {}", e)))?;

        if let Some(changed) = passkey.update_credential(&auth_result) {
            // Some(_) means the credential matched. Save back if anything changed.
            if changed {
                let updated_json = serde_json::to_string(&passkey)?;
                conn.execute(
                    "UPDATE passkey_credentials SET passkey_json = ?1 WHERE id = ?2",
                    params![updated_json, cred_id],
                )?;
            }
            matched_user_id = Some(user_id.clone());
            break;
        }
    }

    let user_id = matched_user_id
        .ok_or_else(|| AppError::Internal("Authenticated passkey not found in database".into()))?;

    let token = session::create_session(&state.db, &user_id, state.config.auth.session_hours)?;

    let body = serde_json::json!({ "status": "ok" });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json".to_string())],
        AppendHeaders([
            (
                header::SET_COOKIE,
                session_cookie(&token, state.config.auth.session_hours),
            ),
            (header::SET_COOKIE, clear_ceremony_cookie()),
        ]),
        serde_json::to_string(&body)?,
    )
        .into_response())
}

// -- Logout handler --

/// POST /auth/logout — delete session and redirect
pub async fn logout(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> AppResult<Response> {
    let (parts, _body) = request.into_parts();

    if let Some(token) = get_cookie_value(&parts, "salita_session") {
        let _ = session::delete_session(&state.db, token);
    }

    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/".to_string()),
            (header::SET_COOKIE, clear_session_cookie()),
        ],
        "",
    )
        .into_response())
}

// -- LAN Pairing handlers --

#[derive(Deserialize)]
pub struct PairVerifyRequest {
    pub code: String,
    pub pin: String,
}

/// GET /pair — render pairing page (mobile side)
pub async fn pair_page() -> Html<PairTemplate> {
    Html(PairTemplate)
}

#[derive(serde::Deserialize)]
pub struct PairCheckQuery {
    pub code: String,
}

/// GET /auth/pair/check?code=xxx — check if a pairing code was used (for polling)
/// Returns: { completed: true/false }
pub async fn pair_check(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<PairCheckQuery>,
) -> AppResult<Response> {
    let completed = {
        let pairings = state.pairings.lock().await;
        pairings.is_completed(&query.code)
    };

    let body = serde_json::json!({ "completed": completed });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&body)?,
    )
        .into_response())
}

/// POST /auth/pair/start — generate pairing code + PIN (desktop side)
/// Returns: { code, pin, url, expires_at }
pub async fn pair_start(State(state): State<AppState>) -> AppResult<Response> {
    // Generate random code and PIN
    let code = uuid::Uuid::new_v4().to_string();
    let pin = crate::auth::pairing::generate_pin();

    // Calculate expiry timestamp (30 seconds from now)
    let expires_at = chrono::Utc::now().timestamp_millis() + 30_000;

    // Store challenge
    {
        let mut pairings = state.pairings.lock().await;
        pairings.insert(code.clone(), pin.clone());
    }

    // Build pairing URL using LAN IP (HTTPS main port)
    let lan_ip = local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string());
    let pair_url = format!("https://{}:{}/pair?code={}", lan_ip, state.config.server.port, code);

    let body = serde_json::json!({
        "code": code,
        "pin": pin,
        "url": pair_url,
        "expiresAt": expires_at,
    });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&body)?,
    )
        .into_response())
}

/// POST /auth/pair/verify — verify code + PIN and create session (mobile side)
/// Returns: { ok: true } with session cookie
pub async fn pair_verify(
    State(state): State<AppState>,
    Json(req): Json<PairVerifyRequest>,
) -> AppResult<Response> {
    // Retrieve challenge (but don't remove it yet - mark as completed instead)
    let challenge = {
        let pairings = state.pairings.lock().await;
        pairings.challenges.get(&req.code).cloned()
    };

    let challenge = challenge.ok_or_else(|| {
        AppError::BadRequest("Invalid or expired pairing code".into())
    })?;

    // Normalize PIN (strip non-digits)
    let submitted_pin = req.pin.chars().filter(|c| c.is_ascii_digit()).collect::<String>();

    if challenge.pin != submitted_pin {
        return Err(AppError::BadRequest("Invalid PIN".into()));
    }

    // Mark as completed (for desktop polling)
    {
        let mut pairings = state.pairings.lock().await;
        pairings.mark_completed(&req.code);
    }

    // Find or create a user (for now, we'll create a default paired user)
    // In the future, you might want to link this to an existing user
    let conn = state.db.get()?;

    let user_id: Option<String> = conn
        .query_row(
            "SELECT id FROM users WHERE username = 'paired-user' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let user_id = if let Some(id) = user_id {
        id
    } else {
        // Create a default paired user
        let new_user_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO users (id, username, display_name, is_admin) VALUES (?1, 'paired-user', 'Paired User', 1)",
            params![new_user_id],
        )?;
        new_user_id
    };

    // Create session
    let token = session::create_session(&state.db, &user_id, state.config.auth.session_hours)?;

    let body = serde_json::json!({ "ok": true });

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        AppendHeaders([(
            header::SET_COOKIE,
            session_cookie(&token, state.config.auth.session_hours),
        )]),
        serde_json::to_string(&body)?,
    )
        .into_response())
}
