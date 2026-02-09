use std::path::PathBuf;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tokio::sync::Mutex;
use webauthn_rs::Webauthn;

use crate::auth::join_tokens::JoinTokenStore;
use crate::auth::webauthn::CeremonyStore;
use crate::config::Config;
use crate::graphql::MeshSchema;

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: Config,
    pub data_dir: PathBuf,
    pub webauthn: Arc<Webauthn>,
    pub ceremonies: Arc<Mutex<CeremonyStore>>,
    pub join_tokens: Arc<Mutex<JoinTokenStore>>,
    pub graphql_schema: MeshSchema,
}
