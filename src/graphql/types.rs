use async_graphql::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a node in the personal mesh network
#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
#[graphql(complex)]
pub struct MeshNode {
    /// Unique node identifier (UUID)
    pub id: String,

    /// Human-readable node name (e.g., "Felix's Laptop")
    pub name: String,

    /// Hostname or IP address for this node
    pub hostname: String,

    /// HTTPS port for this node's Salita instance
    pub port: u16,

    /// Node status (online, offline, degraded)
    pub status: NodeStatus,

    /// Capabilities this node provides
    pub capabilities: Vec<String>,

    /// Last seen timestamp
    pub last_seen: DateTime<Utc>,

    /// When this node was first registered
    pub created_at: DateTime<Utc>,

    /// Additional metadata (JSON)
    #[graphql(skip)]
    pub metadata: Option<String>,
}

#[ComplexObject]
impl MeshNode {
    /// Full HTTPS URL for this node
    async fn url(&self) -> String {
        format!("https://{}:{}", self.hostname, self.port)
    }

    /// Whether this node is currently online
    async fn is_online(&self) -> bool {
        matches!(self.status, NodeStatus::Online)
    }
}

/// Node status in the mesh
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Enum, Eq, PartialEq)]
pub enum NodeStatus {
    /// Node is online and responding
    Online,

    /// Node is offline or unreachable
    Offline,

    /// Node is responding but with degraded performance
    Degraded,
}

/// Input for registering a new node
#[derive(InputObject)]
pub struct RegisterNodeInput {
    /// Node name
    pub name: String,

    /// Hostname or IP address
    pub hostname: String,

    /// HTTPS port
    pub port: u16,

    /// Node capabilities (optional)
    pub capabilities: Option<Vec<String>>,

    /// Additional metadata (JSON string, optional)
    pub metadata: Option<String>,
}

/// Input for updating node status
#[derive(InputObject)]
pub struct UpdateNodeStatusInput {
    /// Node ID
    pub node_id: String,

    /// New status
    pub status: NodeStatus,
}

/// Result of node operations
#[derive(SimpleObject)]
pub struct NodeOperationResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Message describing the result
    pub message: String,

    /// The affected node (if applicable)
    pub node: Option<MeshNode>,
}

/// Connection between two nodes
#[derive(Clone, Debug, Serialize, Deserialize, SimpleObject)]
pub struct NodeConnection {
    /// Source node ID
    pub from_node_id: String,

    /// Target node ID
    pub to_node_id: String,

    /// Connection type (webrtc, http, etc.)
    pub connection_type: ConnectionType,

    /// Connection status
    pub status: ConnectionStatus,

    /// Last successful communication
    pub last_ping: Option<DateTime<Utc>>,

    /// Round-trip time in milliseconds
    pub latency_ms: Option<i32>,
}

/// Type of connection between nodes
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Enum, Eq, PartialEq)]
pub enum ConnectionType {
    /// Direct WebRTC data channel
    WebRtc,

    /// HTTP/HTTPS connection
    Http,

    /// Unknown or fallback
    Unknown,
}

/// Status of a connection
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Enum, Eq, PartialEq)]
pub enum ConnectionStatus {
    /// Connection is active
    Active,

    /// Connection is idle but available
    Idle,

    /// Connection failed or disconnected
    Disconnected,
}
