use std::collections::HashMap;
use std::time::Instant;

const TOKEN_TTL_SECS: u64 = 300; // 5 minutes

/// A one-time join token for accessing the /join page
#[derive(Debug, Clone)]
pub struct JoinToken {
    pub token: String,
    pub created_by: String, // Node ID that created it
    pub expires_at: Instant,
    pub used: bool,
    pub device_ip: Option<String>,      // IP of device that used the token
    pub pin: Option<String>,            // PIN shown on device for verification
    pub session_token: Option<String>,  // Session token after PIN verification
}

/// Store for ephemeral join tokens
pub struct JoinTokenStore {
    pub(crate) tokens: HashMap<String, JoinToken>,
}

impl JoinTokenStore {
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Generate a new join token
    pub fn generate(&mut self, created_by: String) -> String {
        self.clear_stale();

        // Generate cryptographically secure random token
        let token = generate_secure_token();

        self.tokens.insert(
            token.clone(),
            JoinToken {
                token: token.clone(),
                created_by,
                expires_at: Instant::now() + std::time::Duration::from_secs(TOKEN_TTL_SECS),
                used: false,
                device_ip: None,
                pin: None,
                session_token: None,
            },
        );

        token
    }

    /// Validate and mark a token as used (single-use)
    /// Generates a PIN for the device to show
    pub fn use_token(&mut self, token: &str, device_ip: String) -> Option<JoinToken> {
        self.clear_stale();

        let join_token = self.tokens.get_mut(token)?;

        // Check if expired
        if Instant::now() >= join_token.expires_at {
            tracing::warn!("Token {} expired", token);
            self.tokens.remove(token);
            return None;
        }

        // Check if already used
        if join_token.used {
            tracing::warn!("Token {} already used", token);
            return None;
        }

        // Generate PIN for this device
        let pin = crate::auth::pairing::generate_pin();
        tracing::info!("Generated PIN {} for token {} from device {}", pin, token, device_ip);

        // Mark as used and store device IP + PIN
        join_token.used = true;
        join_token.device_ip = Some(device_ip.clone());
        join_token.pin = Some(pin.clone());

        Some(join_token.clone())
    }

    /// Verify PIN for a token
    pub fn verify_pin(&self, token: &str, pin: &str) -> bool {
        if let Some(join_token) = self.tokens.get(token) {
            let stored_pin = join_token.pin.as_ref();
            let is_used = join_token.used;
            let pins_match = stored_pin.map(|p| p == pin).unwrap_or(false);

            tracing::info!("Verifying token {} - used: {}, stored_pin: {:?}, provided_pin: {}, match: {}",
                token, is_used, stored_pin, pin, pins_match);

            is_used && pins_match
        } else {
            tracing::warn!("Token {} not found for verification", token);
            false
        }
    }

    /// Check if token exists and is valid (without consuming it)
    pub fn is_valid(&self, token: &str) -> bool {
        if let Some(join_token) = self.tokens.get(token) {
            !join_token.used && Instant::now() < join_token.expires_at
        } else {
            false
        }
    }

    /// Remove expired tokens
    fn clear_stale(&mut self) {
        let now = Instant::now();
        self.tokens.retain(|_, token| now < token.expires_at);
    }
}

/// Generate a cryptographically secure random token
fn generate_secure_token() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const TOKEN_LEN: usize = 32;

    let mut rng = rand::thread_rng();
    let token: String = (0..TOKEN_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        assert_eq!(token.len(), 32);
        assert!(store.is_valid(&token));
    }

    #[test]
    fn test_single_use_token() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        // First use should work
        let result = store.use_token(&token, "192.168.1.1".to_string());
        assert!(result.is_some());

        // Second use should fail
        let result = store.use_token(&token, "192.168.1.1".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_invalid_token() {
        let mut store = JoinTokenStore::new();

        let result = store.use_token("invalid-token", "192.168.1.1".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_pin_generation() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        // Use token to generate PIN
        let join_token = store.use_token(&token, "192.168.1.1".to_string()).unwrap();

        // PIN should be 6 digits
        assert!(join_token.pin.is_some());
        let pin = join_token.pin.unwrap();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_pin_verification_success() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        // Use token to generate PIN
        let join_token = store.use_token(&token, "192.168.1.1".to_string()).unwrap();
        let pin = join_token.pin.unwrap();

        // Verify correct PIN
        assert!(store.verify_pin(&token, &pin));
    }

    #[test]
    fn test_pin_verification_wrong_pin() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        // Use token to generate PIN
        store.use_token(&token, "192.168.1.1".to_string()).unwrap();

        // Verify wrong PIN
        assert!(!store.verify_pin(&token, "000000"));
    }

    #[test]
    fn test_pin_verification_unused_token() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        // Don't use the token, just try to verify
        assert!(!store.verify_pin(&token, "123456"));
    }

    #[test]
    fn test_device_ip_stored() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        let device_ip = "192.168.1.100".to_string();
        let join_token = store.use_token(&token, device_ip.clone()).unwrap();

        assert_eq!(join_token.device_ip, Some(device_ip));
    }

    #[test]
    fn test_token_uniqueness() {
        let mut store = JoinTokenStore::new();
        let token1 = store.generate("node-1".to_string());
        let token2 = store.generate("node-1".to_string());

        assert_ne!(token1, token2, "Tokens should be unique");
    }

    #[test]
    fn test_pin_uniqueness() {
        let mut store = JoinTokenStore::new();

        let token1 = store.generate("node-1".to_string());
        let join1 = store.use_token(&token1, "192.168.1.1".to_string()).unwrap();

        let token2 = store.generate("node-2".to_string());
        let join2 = store.use_token(&token2, "192.168.1.2".to_string()).unwrap();

        // PINs should be different (statistically extremely likely)
        assert_ne!(join1.pin, join2.pin, "PINs should be unique");
    }

    #[test]
    fn test_token_created_by() {
        let mut store = JoinTokenStore::new();
        let node_id = "node-123".to_string();
        let token = store.generate(node_id.clone());

        // Check internal state (tokens HashMap)
        let join_token = store.tokens.get(&token).unwrap();
        assert_eq!(join_token.created_by, node_id);
    }

    #[test]
    fn test_multiple_tokens() {
        let mut store = JoinTokenStore::new();

        let token1 = store.generate("node-1".to_string());
        let token2 = store.generate("node-2".to_string());
        let token3 = store.generate("node-3".to_string());

        assert!(store.is_valid(&token1));
        assert!(store.is_valid(&token2));
        assert!(store.is_valid(&token3));
    }

    #[test]
    fn test_secure_token_charset() {
        // Verify generate_secure_token uses alphanumeric only
        for _ in 0..100 {
            let token = generate_secure_token();
            assert_eq!(token.len(), 32);
            assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
        }
    }

    #[test]
    fn test_clear_stale_removes_nothing_when_valid() {
        let mut store = JoinTokenStore::new();
        let token = store.generate("node-1".to_string());

        store.clear_stale();

        // Token should still be valid
        assert!(store.is_valid(&token));
    }
}
