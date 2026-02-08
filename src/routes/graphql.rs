use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use axum::extract::State;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;

use crate::extractors::CurrentUser;
use crate::state::AppState;

/// GraphQL endpoint handler
async fn graphql_handler(
    State(state): State<AppState>,
    _user: CurrentUser, // Require authentication
    Json(req): Json<async_graphql::Request>,
) -> Json<async_graphql::Response> {
    let mut request = req;

    // Add database pool to GraphQL context
    request = request.data(state.db.clone());

    let response = state.graphql_schema.execute(request).await;
    Json(response)
}

/// GraphQL Playground UI (development tool)
async fn graphql_playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/graphql")))
}

/// GraphQL router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/graphql", post(graphql_handler))
        .route("/graphql/playground", get(graphql_playground))
}
