use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use tokio::sync::Mutex;
use webauthn_rs::Webauthn;

use crate::auth::pairing::PairingStore;
use crate::auth::webauthn::CeremonyStore;
use crate::config::Config;

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: Config,
    pub webauthn: Arc<Webauthn>,
    pub ceremonies: Arc<Mutex<CeremonyStore>>,
    pub pairings: Arc<Mutex<PairingStore>>,
}
