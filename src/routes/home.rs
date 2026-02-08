use askama::Template;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};

use crate::error::AppResult;
use crate::extractors::MaybeUser;
use crate::state::AppState;

#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct HomeTemplate {
    pub user_count: i64,
}

/// Wrapper to render askama templates as axum responses
pub struct Html<T: Template>(pub T);

impl<T: Template> IntoResponse for Html<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(body) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                body,
            )
                .into_response(),
            Err(e) => {
                tracing::error!("Template render error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Template error").into_response()
            }
        }
    }
}

pub async fn index(
    State(state): State<AppState>,
    maybe_user: MaybeUser,
) -> AppResult<Response> {
    // If user is authenticated, redirect to dashboard
    if maybe_user.0.is_some() {
        return Ok(Redirect::to("/dashboard").into_response());
    }

    // Otherwise show the home page
    let conn = state.db.get()?;
    let user_count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;

    Ok(Html(HomeTemplate { user_count }).into_response())
}
