use std::collections::HashMap;
use std::time::Instant;

const LINK_TTL_SECS: u64 = 300; // 5 minutes

/// Purpose of a linking code
#[derive(Debug, Clone)]
pub enum LinkPurpose {
    PairDevice,  // Link new PIN session to existing user
    #[allow(dead_code)]
    AddPasskey,  // Add passkey to existing PIN-only user
}

/// A linking code for device/account verification
#[derive(Debug, Clone)]
pub struct LinkingCode {
    pub user_id: String,
    #[allow(dead_code)]
    pub code: String,
    pub expires_at: Instant,
    pub purpose: LinkPurpose,
}

/// Store for linking codes (DNS-style verification)
pub struct LinkingCodeStore {
    pub(crate) codes: HashMap<String, LinkingCode>,
}

impl LinkingCodeStore {
    pub fn new() -> Self {
        Self {
            codes: HashMap::new(),
        }
    }

    /// Generate a new linking code for a user
    pub fn generate(&mut self, user_id: String, purpose: LinkPurpose) -> String {
        self.clear_stale();
        let code = generate_human_readable_code();
        self.codes.insert(
            code.clone(),
            LinkingCode {
                user_id,
                code: code.clone(),
                expires_at: Instant::now() + std::time::Duration::from_secs(LINK_TTL_SECS),
                purpose,
            },
        );
        code
    }

    /// Verify and consume a linking code
    pub fn verify(&mut self, code: &str) -> Option<LinkingCode> {
        self.clear_stale();
        let linking = self.codes.remove(code)?;

        if Instant::now() >= linking.expires_at {
            None
        } else {
            Some(linking)
        }
    }

    /// Check if a code exists (without consuming it)
    #[allow(dead_code)]
    pub fn exists(&self, code: &str) -> bool {
        self.codes.contains_key(code)
    }

    /// Remove expired codes
    fn clear_stale(&mut self) {
        let now = Instant::now();
        self.codes.retain(|_, linking| now < linking.expires_at);
    }
}

/// Generate a human-readable linking code using NATO phonetic alphabet
/// Format: ALPHA-BRAVO-42
fn generate_human_readable_code() -> String {
    use rand::Rng;

    const WORDS: &[&str] = &[
        "ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO",
        "FOXTROT", "GOLF", "HOTEL", "INDIA", "JULIET",
        "KILO", "LIMA", "MIKE", "NOVEMBER", "OSCAR",
        "PAPA", "QUEBEC", "ROMEO", "SIERRA", "TANGO",
    ];

    let mut rng = rand::thread_rng();
    let word1 = WORDS[rng.gen_range(0..WORDS.len())];
    let word2 = WORDS[rng.gen_range(0..WORDS.len())];
    let num = rng.gen_range(10..100);

    format!("{}-{}-{}", word1, word2, num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code_format() {
        let code = generate_human_readable_code();
        let parts: Vec<&str> = code.split('-').collect();

        assert_eq!(parts.len(), 3, "Code should have 3 parts");
        assert!(parts[0].chars().all(|c| c.is_uppercase() || c == '-'));
        assert!(parts[1].chars().all(|c| c.is_uppercase() || c == '-'));
        assert!(parts[2].parse::<u32>().is_ok(), "Third part should be a number");
    }

    #[test]
    fn test_linking_store_generate_and_verify() {
        let mut store = LinkingCodeStore::new();
        let user_id = "test-user".to_string();

        let code = store.generate(user_id.clone(), LinkPurpose::PairDevice);

        let linking = store.verify(&code);
        assert!(linking.is_some());

        let linking = linking.unwrap();
        assert_eq!(linking.user_id, user_id);
        assert_eq!(linking.code, code);

        // Code should be consumed
        assert!(store.verify(&code).is_none());
    }

    #[test]
    fn test_linking_store_nonexistent_code() {
        let mut store = LinkingCodeStore::new();
        assert!(store.verify("INVALID-CODE-99").is_none());
    }

    #[test]
    fn test_linking_store_clear_stale() {
        let mut store = LinkingCodeStore::new();

        // Insert a code
        let code = store.generate("user1".to_string(), LinkPurpose::PairDevice);

        // Code should exist
        assert!(store.exists(&code));

        // Manually expire it
        if let Some(linking) = store.codes.get_mut(&code) {
            linking.expires_at = Instant::now() - std::time::Duration::from_secs(1);
        }

        // Verify should fail and clean up
        assert!(store.verify(&code).is_none());
        assert!(!store.exists(&code));
    }
}
