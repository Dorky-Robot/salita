// Domain types - Pure, immutable, no side effects
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// New types for compile-time safety
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JoinToken(pub String);

impl JoinToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn generate() -> Self {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::thread_rng();
        let token: String = (0..32)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();
        Self(token)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin(pub String); // Stores bcrypt hash, not plaintext

impl Pin {
    /// Create Pin from plaintext - hashes it for secure storage
    pub fn new(pin: impl Into<String>) -> Self {
        let plaintext = pin.into();
        let hash =
            bcrypt::hash(&plaintext, bcrypt::DEFAULT_COST).unwrap_or_else(|_| plaintext.clone()); // Fallback to plaintext if hashing fails (shouldn't happen)
        Self(hash)
    }

    /// Generate a random PIN - returns (plaintext_to_show_user, hashed_pin_to_store)
    pub fn generate() -> (String, Self) {
        use rand::Rng;
        let plaintext = rand::thread_rng().gen_range(100000..=999999).to_string();
        let hash =
            bcrypt::hash(&plaintext, bcrypt::DEFAULT_COST).unwrap_or_else(|_| plaintext.clone());
        (plaintext, Self(hash))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Verify plaintext PIN against stored hash - constant-time via bcrypt
    pub fn verify(&self, plaintext: &str) -> bool {
        bcrypt::verify(plaintext, &self.0).unwrap_or(false)
    }

    /// Constant-time comparison for hash-to-hash comparison (backward compat)
    pub fn constant_time_eq(&self, other: &Pin) -> bool {
        let a = self.0.as_bytes();
        let b = other.0.as_bytes();

        // If lengths differ, still compare to avoid timing leak
        let len_match = a.len() == b.len();
        let max_len = a.len().max(b.len());

        let mut result = 0u8;
        for i in 0..max_len {
            let byte_a = a.get(i).copied().unwrap_or(0);
            let byte_b = b.get(i).copied().unwrap_or(0);
            result |= byte_a ^ byte_b;
        }

        len_match && result == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionToken(pub String);

impl SessionToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn generate() -> Self {
        use rand::Rng;
        let token: String = (0..32)
            .map(|_| format!("{:02x}", rand::thread_rng().gen::<u8>()))
            .collect();
        Self(token)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpAddress(pub String);

impl IpAddress {
    pub fn new(ip: impl Into<String>) -> Self {
        Self(ip.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerToken(pub String);

impl PeerToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn generate() -> Self {
        use rand::Rng;
        let token: String = (0..32)
            .map(|_| format!("{:02x}", rand::thread_rng().gen::<u8>()))
            .collect();
        Self(token)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Pairing state machine - Pure, explicit state transitions
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PairingState {
    /// Token created, waiting for device to scan QR
    TokenCreated {
        token: JoinToken,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },

    /// Device scanned QR, PIN generated
    DeviceConnected {
        token: JoinToken,
        device_ip: IpAddress,
        device_node_id: Option<NodeId>,
        pin: Pin,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },

    /// Desktop verified PIN, session created
    PinVerified {
        token: JoinToken,
        device_ip: IpAddress,
        device_node_id: NodeId,
        session_token: SessionToken,
        created_at: DateTime<Utc>,
    },

    /// Device fully registered in mesh
    DeviceRegistered {
        token: JoinToken,
        node_id: NodeId,
        peer_token: PeerToken,
        session_token: SessionToken,
    },

    /// Pairing failed or expired
    Failed {
        token: JoinToken,
        reason: PairingFailure,
        failed_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PairingFailure {
    TokenExpired,
    InvalidPin,
    DeviceAlreadyRegistered,
    IpConflict { existing_device: String },
    Other(String),
}

impl fmt::Display for PairingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "Token expired"),
            Self::InvalidPin => write!(f, "Invalid PIN"),
            Self::DeviceAlreadyRegistered => write!(f, "Device already registered"),
            Self::IpConflict { existing_device } => {
                write!(f, "IP conflict with device: {}", existing_device)
            }
            Self::Other(msg) => write!(f, "{}", msg),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairingError {
    InvalidTransition(String),
    Expired(String),
    PinMismatch,
    MissingNodeId,
    TokenExpired,
    InvalidPin,
    DeviceAlreadyRegistered,
    IpConflict { existing_device: String },
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition(msg) => write!(f, "{}", msg),
            Self::Expired(msg) => write!(f, "{}", msg),
            Self::PinMismatch => write!(f, "Incorrect PIN"),
            Self::MissingNodeId => write!(f, "Device node ID is required"),
            Self::TokenExpired => write!(f, "Token expired"),
            Self::InvalidPin => write!(f, "Invalid PIN"),
            Self::DeviceAlreadyRegistered => write!(f, "Device already registered"),
            Self::IpConflict { existing_device } => {
                write!(f, "IP conflict with device: {}", existing_device)
            }
        }
    }
}

impl std::error::Error for PairingError {}

/// Pure state transitions - no side effects!
impl PairingState {
    /// Get the token from any state
    pub fn token(&self) -> &JoinToken {
        match self {
            Self::TokenCreated { token, .. } => token,
            Self::DeviceConnected { token, .. } => token,
            Self::PinVerified { token, .. } => token,
            Self::DeviceRegistered { token, .. } => token,
            Self::Failed { token, .. } => token,
        }
    }

    /// Get state name for debugging/logging
    pub fn state_name(&self) -> &'static str {
        match self {
            Self::TokenCreated { .. } => "TokenCreated",
            Self::DeviceConnected { .. } => "DeviceConnected",
            Self::PinVerified { .. } => "PinVerified",
            Self::DeviceRegistered { .. } => "DeviceRegistered",
            Self::Failed { .. } => "Failed",
        }
    }

    /// Get expires_at timestamp (if state has expiration)
    pub fn expires_at(&self) -> DateTime<Utc> {
        match self {
            Self::TokenCreated { expires_at, .. } => *expires_at,
            Self::DeviceConnected { expires_at, .. } => *expires_at,
            Self::PinVerified { created_at, .. } => *created_at + Duration::hours(24),
            Self::DeviceRegistered { .. } => Utc::now() + Duration::days(365),
            Self::Failed { failed_at, .. } => *failed_at,
        }
    }

    /// Get device node ID (if available)
    pub fn device_node_id(&self) -> Option<&NodeId> {
        match self {
            Self::DeviceConnected { device_node_id, .. } => device_node_id.as_ref(),
            Self::PinVerified { device_node_id, .. } => Some(device_node_id),
            Self::DeviceRegistered { node_id, .. } => Some(node_id),
            _ => None,
        }
    }

    /// Get device IP address (if available)
    pub fn device_ip(&self) -> Option<&IpAddress> {
        match self {
            Self::DeviceConnected { device_ip, .. } => Some(device_ip),
            Self::PinVerified { device_ip, .. } => Some(device_ip),
            _ => None,
        }
    }

    /// Get failure reason (if failed)
    pub fn failure_reason(&self) -> Option<&PairingFailure> {
        match self {
            Self::Failed { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// Transition: Token → DeviceConnected
    /// Returns (new_state, plaintext_pin_to_show_user)
    pub fn connect_device(
        self,
        device_ip: IpAddress,
        now: DateTime<Utc>,
    ) -> Result<(Self, String), PairingError> {
        match self {
            Self::TokenCreated {
                token, expires_at, ..
            } => {
                if now > expires_at {
                    return Err(PairingError::TokenExpired);
                }

                let (plaintext_pin, hashed_pin) = Pin::generate();

                Ok((
                    Self::DeviceConnected {
                        token,
                        device_ip,
                        device_node_id: None,
                        pin: hashed_pin, // Store hash, not plaintext
                        created_at: now,
                        expires_at,
                    },
                    plaintext_pin, // Return plaintext to show user
                ))
            }
            other => Err(PairingError::InvalidTransition(format!(
                "Cannot connect device from {} state",
                other.state_name()
            ))),
        }
    }

    /// Update device node ID (phone sends its persistent identity)
    pub fn set_device_node_id(self, node_id: NodeId) -> Result<Self, PairingError> {
        match self {
            Self::DeviceConnected {
                token,
                device_ip,
                pin,
                created_at,
                expires_at,
                ..
            } => Ok(Self::DeviceConnected {
                token,
                device_ip,
                device_node_id: Some(node_id),
                pin,
                created_at,
                expires_at,
            }),
            other => Err(PairingError::InvalidTransition(format!(
                "Cannot set node ID from {} state",
                other.state_name()
            ))),
        }
    }

    /// Transition: DeviceConnected → PinVerified
    /// Accepts plaintext PIN and verifies against stored hash
    pub fn verify_pin(
        self,
        provided_pin_plaintext: &str,
        session_token: SessionToken,
        now: DateTime<Utc>,
    ) -> Result<Self, PairingError> {
        match self {
            Self::DeviceConnected {
                token,
                device_ip,
                device_node_id,
                pin,
                expires_at,
                ..
            } => {
                if now > expires_at {
                    return Err(PairingError::TokenExpired);
                }

                // Verify plaintext PIN against stored hash (constant-time via bcrypt)
                if !pin.verify(provided_pin_plaintext) {
                    return Err(PairingError::InvalidPin);
                }

                // Require device_node_id to be set before PIN verification
                let device_node_id = device_node_id.ok_or(PairingError::MissingNodeId)?;

                Ok(Self::PinVerified {
                    token,
                    device_ip,
                    device_node_id,
                    session_token,
                    created_at: now,
                })
            }
            other => Err(PairingError::InvalidTransition(format!(
                "Cannot verify PIN from {} state",
                other.state_name()
            ))),
        }
    }

    /// Transition: PinVerified → DeviceRegistered
    pub fn register_device(self, peer_token: PeerToken) -> Result<Self, PairingError> {
        match self {
            Self::PinVerified {
                token,
                device_node_id,
                session_token,
                ..
            } => Ok(Self::DeviceRegistered {
                token,
                node_id: device_node_id,
                peer_token,
                session_token,
            }),
            other => Err(PairingError::InvalidTransition(format!(
                "Cannot register device from {} state",
                other.state_name()
            ))),
        }
    }

    /// Transition to Failed state
    #[allow(dead_code)]
    pub fn fail(self, reason: PairingFailure, now: DateTime<Utc>) -> Self {
        Self::Failed {
            token: self.token().clone(),
            reason,
            failed_at: now,
        }
    }

    /// Check if state is expired
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self {
            Self::TokenCreated { expires_at, .. } => now > *expires_at,
            Self::DeviceConnected { expires_at, .. } => now > *expires_at,
            Self::PinVerified { .. } => false, // No expiry after PIN verified
            Self::DeviceRegistered { .. } => false,
            Self::Failed { .. } => true,
        }
    }

    /// Check if pairing is complete
    #[allow(dead_code)]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::DeviceRegistered { .. })
    }

    /// Check if pairing failed
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Pure coordinator - Business logic decisions
pub struct PairingCoordinator;

impl PairingCoordinator {
    /// Calculate token expiration (pure)
    pub fn calculate_expiry(created_at: DateTime<Utc>, ttl_seconds: u64) -> DateTime<Utc> {
        created_at + Duration::seconds(ttl_seconds as i64)
    }

    /// Create initial pairing state
    pub fn create_pairing(token: JoinToken, now: DateTime<Utc>, ttl_seconds: u64) -> PairingState {
        let expires_at = Self::calculate_expiry(now, ttl_seconds);
        PairingState::TokenCreated {
            token,
            created_at: now,
            expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation() {
        let token1 = JoinToken::generate();
        let token2 = JoinToken::generate();

        // Tokens should be unique
        assert_ne!(token1, token2);

        // Tokens should be 32 characters
        assert_eq!(token1.as_str().len(), 32);
        assert_eq!(token2.as_str().len(), 32);
    }

    #[test]
    fn test_pin_generation() {
        let (plaintext1, _hash1) = Pin::generate();
        let (plaintext2, _hash2) = Pin::generate();

        // PINs should be 6 digits
        assert_eq!(plaintext1.len(), 6);
        assert_eq!(plaintext2.len(), 6);

        // PINs should be numeric
        assert!(plaintext1.chars().all(|c| c.is_numeric()));
        assert!(plaintext2.chars().all(|c| c.is_numeric()));
    }

    #[test]
    fn test_valid_state_transitions() {
        let token = JoinToken::new("test123");
        let now = Utc::now();

        // Start state
        let state = PairingCoordinator::create_pairing(token.clone(), now, 300);
        assert_eq!(state.state_name(), "TokenCreated");
        assert!(!state.is_expired(now));

        // Transition 1: Connect device
        let (state, pin) = state
            .connect_device(IpAddress::new("192.168.1.100"), now)
            .unwrap();
        assert_eq!(state.state_name(), "DeviceConnected");

        // Transition 1b: Set device node ID
        let state = state.set_device_node_id(NodeId::new("node123")).unwrap();

        // Transition 2: Verify PIN
        let state = state
            .verify_pin(&pin, SessionToken::new("session456"), now)
            .unwrap();
        assert_eq!(state.state_name(), "PinVerified");

        // Transition 3: Register device
        let state = state.register_device(PeerToken::new("peer789")).unwrap();
        assert_eq!(state.state_name(), "DeviceRegistered");
        assert!(state.is_complete());
        assert!(!state.is_failed());
    }

    #[test]
    fn test_invalid_transition_rejected() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token, now, 300);

        // Cannot verify PIN from TokenCreated state
        let result = state.verify_pin("123456", SessionToken::new("session"), now);

        assert!(matches!(result, Err(PairingError::InvalidTransition(..))));
    }

    #[test]
    fn test_expired_token_rejected() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let past = now - Duration::minutes(10);

        let state = PairingCoordinator::create_pairing(token, past, 300); // 5 min TTL

        // Should be expired
        assert!(state.is_expired(now));

        // Should reject connection
        let result = state.connect_device(IpAddress::new("192.168.1.100"), now);
        assert!(matches!(result, Err(PairingError::TokenExpired)));
    }

    #[test]
    fn test_wrong_pin_rejected() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token, now, 300);

        let (state, _correct_pin) = state
            .connect_device(IpAddress::new("192.168.1.100"), now)
            .unwrap();

        let state = state.set_device_node_id(NodeId::new("node123")).unwrap();

        // Try with wrong PIN
        let result = state.verify_pin("999999", SessionToken::new("session"), now);

        assert!(matches!(result, Err(PairingError::InvalidPin)));
    }

    #[test]
    fn test_pin_verification_requires_node_id() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token, now, 300);

        let (state, pin) = state
            .connect_device(IpAddress::new("192.168.1.100"), now)
            .unwrap();

        // Don't set node_id, try to verify PIN
        let result = state.verify_pin(&pin, SessionToken::new("session"), now);

        assert!(matches!(result, Err(PairingError::MissingNodeId)));
    }

    #[test]
    fn test_pin_verification() {
        // Test correct PIN verification
        let hashed_pin = Pin::new("123456");
        assert!(hashed_pin.verify("123456"));

        // Test incorrect PIN
        assert!(!hashed_pin.verify("654321"));

        // Test different length
        assert!(!hashed_pin.verify("12345"));

        // Test empty string
        let empty_pin = Pin::new("");
        assert!(empty_pin.verify(""));
        assert!(!empty_pin.verify("123456"));

        // Test that different hashes of same plaintext both verify correctly
        let pin2 = Pin::new("123456");
        assert!(pin2.verify("123456"));
        // Note: pin1 and pin2 have different hashes (different bcrypt salts) but both verify the same plaintext
    }

    #[test]
    fn test_fail_transition() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token, now, 300);

        let failed_state = state.fail(PairingFailure::InvalidPin, now);

        assert_eq!(failed_state.state_name(), "Failed");
        assert!(failed_state.is_failed());
        assert!(!failed_state.is_complete());
    }

    #[test]
    fn test_state_serialization() {
        let token = JoinToken::new("test");
        let now = Utc::now();
        let state = PairingCoordinator::create_pairing(token, now, 300);

        // Serialize to JSON
        let json = serde_json::to_string(&state).unwrap();

        // Deserialize back
        let deserialized: PairingState = serde_json::from_str(&json).unwrap();

        assert_eq!(state, deserialized);
    }
}
