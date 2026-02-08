use std::collections::HashMap;
use std::time::Instant;

const PAIR_TTL_SECS: u64 = 60;

/// A pending pairing challenge with code, PIN, and expiry.
#[derive(Debug, Clone)]
pub struct PairingChallenge {
    pub pin: String,
    pub expires_at: Instant,
    pub completed: bool,
}

/// Ephemeral in-memory store for LAN pairing challenges.
/// Each entry is keyed by a random code and expires after 30 seconds.
pub struct PairingStore {
    pub(crate) challenges: HashMap<String, PairingChallenge>,
}

impl PairingStore {
    pub fn new() -> Self {
        Self {
            challenges: HashMap::new(),
        }
    }

    /// Store a new pairing challenge with the given code and PIN.
    pub fn insert(&mut self, code: String, pin: String) {
        self.clear_stale();
        self.challenges.insert(
            code,
            PairingChallenge {
                pin,
                expires_at: Instant::now() + std::time::Duration::from_secs(PAIR_TTL_SECS),
                completed: false,
            },
        );
    }

    /// Mark a pairing challenge as completed (for desktop polling).
    pub fn mark_completed(&mut self, code: &str) {
        if let Some(challenge) = self.challenges.get_mut(code) {
            challenge.completed = true;
        }
    }

    /// Check if a pairing challenge is completed.
    pub fn is_completed(&self, code: &str) -> bool {
        self.challenges
            .get(code)
            .map(|c| c.completed)
            .unwrap_or(false)
    }

    /// Retrieve and remove a pairing challenge by code.
    /// Returns None if the code doesn't exist or has expired.
    #[allow(dead_code)]
    pub fn take(&mut self, code: &str) -> Option<PairingChallenge> {
        self.clear_stale();
        let challenge = self.challenges.remove(code)?;

        if Instant::now() >= challenge.expires_at {
            None
        } else {
            Some(challenge)
        }
    }

    /// Remove all expired challenges.
    fn clear_stale(&mut self) {
        let now = Instant::now();
        self.challenges.retain(|_, challenge| now < challenge.expires_at);
    }

    /// Get the number of active challenges (for debugging).
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.challenges.len()
    }
}

/// Generate a random 6-digit PIN.
pub fn generate_pin() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let pin: u32 = rng.gen_range(100000..1000000);
    pin.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pin() {
        let pin = generate_pin();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_pairing_store_insert_and_take() {
        let mut store = PairingStore::new();
        let code = "test-code".to_string();
        let pin = "123456".to_string();

        store.insert(code.clone(), pin.clone());

        let challenge = store.take(&code);
        assert!(challenge.is_some());
        assert_eq!(challenge.unwrap().pin, pin);

        // Second take should return None
        assert!(store.take(&code).is_none());
    }

    #[test]
    fn test_pairing_store_nonexistent() {
        let mut store = PairingStore::new();
        assert!(store.take("nonexistent").is_none());
    }
}
