use askama::Template;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::auth::RequestOrigin;
use crate::error::AppResult;
use crate::extractors::MaybeUser;
use crate::routes::home::Html;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "components/settings_modal.html")]
pub struct SettingsModalTemplate {
    pub username: String,
    pub show_signout: bool,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/settings/modal", get(settings_modal))
}

async fn settings_modal(
    State(_state): State<AppState>,
    origin: RequestOrigin,
    maybe_user: MaybeUser,
) -> AppResult<Response> {
    // Get username - default to "localhost" for localhost users
    let username = match maybe_user.0 {
        Some(user) => user.username,
        None => "Guest".to_string(),
    };

    // Only show sign out if not on localhost (localhost users don't need to sign out)
    let show_signout = origin != RequestOrigin::Localhost;

    Ok(Html(SettingsModalTemplate {
        username,
        show_signout,
    })
    .into_response())
}
