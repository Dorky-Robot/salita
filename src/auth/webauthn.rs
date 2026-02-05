use std::collections::HashMap;
use std::time::Instant;
use webauthn_rs::prelude::*;
use webauthn_rs::Webauthn;

/// Build a Webauthn instance for localhost development.
pub fn build_webauthn(port: u16) -> Result<Webauthn, webauthn_rs::prelude::WebauthnError> {
    let rp_id = "localhost";
    let rp_origin =
        url::Url::parse(&format!("http://localhost:{}", port)).expect("Invalid origin URL");
    let builder = webauthn_rs::WebauthnBuilder::new(rp_id, &rp_origin)?;
    builder.build()
}

/// A pending registration bundles the WebAuthn ceremony state with the user
/// metadata needed to create the account when registration finishes.
pub struct PendingRegistration {
    pub reg_state: PasskeyRegistration,
    pub user_id: String,
    pub username: String,
    pub display_name: String,
}

/// Ephemeral in-memory store for WebAuthn registration/authentication ceremonies.
/// Each entry is keyed by a random ceremony ID and expires after 5 minutes.
pub struct CeremonyStore {
    registrations: HashMap<String, (Instant, PendingRegistration)>,
    authentications: HashMap<String, (Instant, PasskeyAuthentication)>,
}

impl CeremonyStore {
    pub fn new() -> Self {
        Self {
            registrations: HashMap::new(),
            authentications: HashMap::new(),
        }
    }

    /// Store a pending registration (ceremony state + user metadata).
    pub fn insert_registration(&mut self, id: String, pending: PendingRegistration) {
        self.clear_stale();
        self.registrations.insert(id, (Instant::now(), pending));
    }

    /// Retrieve and remove a pending registration.
    pub fn take_registration(&mut self, id: &str) -> Option<PendingRegistration> {
        self.registrations.remove(id).map(|(_, pending)| pending)
    }

    /// Store an authentication ceremony state. Clears any stale entries first.
    pub fn insert_authentication(&mut self, id: String, state: PasskeyAuthentication) {
        self.clear_stale();
        self.authentications.insert(id, (Instant::now(), state));
    }

    /// Retrieve and remove an authentication ceremony state.
    pub fn take_authentication(&mut self, id: &str) -> Option<PasskeyAuthentication> {
        self.authentications.remove(id).map(|(_, state)| state)
    }

    fn clear_stale(&mut self) {
        let cutoff = Instant::now() - std::time::Duration::from_secs(300);
        self.registrations.retain(|_, (t, _)| *t > cutoff);
        self.authentications.retain(|_, (t, _)| *t > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_webauthn_succeeds() {
        let wn = build_webauthn(3000);
        assert!(wn.is_ok());
    }

    #[test]
    fn ceremony_store_insert_and_take() {
        let mut store = CeremonyStore::new();
        // We can't easily construct a PasskeyRegistration without a real ceremony,
        // so we just test the store structure compiles and basic ops work.
        assert!(store.take_registration("nonexistent").is_none());
        assert!(store.take_authentication("nonexistent").is_none());
    }
}
