use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

impl NodeIdentity {
    /// Load existing node identity or create a new one
    pub fn load_or_create(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("node_identity.json");

        if path.exists() {
            let json = fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            let identity = Self {
                id: uuid::Uuid::now_v7().to_string(),
                name: default_node_name(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            fs::write(&path, serde_json::to_string_pretty(&identity)?)?;
            tracing::info!("Created new node identity: {}", identity.id);
            Ok(identity)
        }
    }
}

fn default_node_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "Salita Node".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_create_generates_new_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = NodeIdentity::load_or_create(tmp.path()).unwrap();

        assert!(!identity.id.is_empty());
        assert!(!identity.name.is_empty());
        assert!(!identity.created_at.is_empty());
    }

    #[test]
    fn load_or_create_preserves_existing_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let id1 = NodeIdentity::load_or_create(tmp.path()).unwrap();
        let id2 = NodeIdentity::load_or_create(tmp.path()).unwrap();

        assert_eq!(id1.id, id2.id);
        assert_eq!(id1.name, id2.name);
        assert_eq!(id1.created_at, id2.created_at);
    }

    #[test]
    fn node_identity_file_format() {
        let tmp = tempfile::tempdir().unwrap();
        let identity = NodeIdentity::load_or_create(tmp.path()).unwrap();

        let path = tmp.path().join("node_identity.json");
        let json = fs::read_to_string(&path).unwrap();

        assert!(json.contains(&identity.id));
        assert!(json.contains(&identity.name));
        assert!(json.contains(&identity.created_at));
    }
}
