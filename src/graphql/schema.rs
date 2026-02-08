use async_graphql::{EmptySubscription, Schema};

use super::mutations::MutationRoot;
use super::queries::QueryRoot;

/// GraphQL Schema type
pub type MeshSchema = Schema<QueryRoot, MutationRoot, EmptySubscription>;

/// Build the GraphQL schema
pub fn build_schema() -> MeshSchema {
    Schema::build(QueryRoot, MutationRoot, EmptySubscription).finish()
}
