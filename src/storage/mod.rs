mod eviction;
mod iroh_node;
mod metadata;
mod placement;
mod replication;

use anyhow::Result;
use bytes::Bytes;
use iroh::blobs::Hash;
use std::path::PathBuf;
use std::sync::Arc;

use crate::state::DbPool;

pub use self::eviction::{EvictionManager, EvictionStrategy};
pub use self::iroh_node::IrohNodeManager;
pub use self::metadata::{FileMetadata, FileRecord};
pub use self::placement::{NodeStorageQuota, PlacementStrategy, ReplicationReport};
pub use self::replication::ReplicationManager;

/// Configuration for distributed storage
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Directory for Iroh blob storage
    pub data_dir: PathBuf,
    /// Number of copies to maintain across mesh
    pub replication_factor: usize,
    /// Automatically replicate files on upload
    pub auto_replicate: bool,
    /// Maximum storage in GB
    pub max_storage_gb: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("salita")
                .join("storage"),
            replication_factor: 3,
            auto_replicate: true,
            max_storage_gb: 100,
        }
    }
}

/// Main storage manager - coordinates Iroh, metadata, and replication
pub struct StorageManager {
    iroh: Arc<IrohNodeManager>,
    db: DbPool,
    config: StorageConfig,
    replication: Arc<ReplicationManager>,
}

impl StorageManager {
    /// Create a new storage manager
    pub async fn new(db: DbPool, config: StorageConfig) -> Result<Self> {
        // Initialize Iroh node
        let iroh = Arc::new(IrohNodeManager::new(&config.data_dir).await?);

        // Initialize replication manager
        let replication = Arc::new(ReplicationManager::new(
            db.clone(),
            iroh.clone(),
            config.replication_factor,
        ));

        Ok(Self {
            iroh,
            db,
            config,
            replication,
        })
    }

    /// Add a file to distributed storage
    ///
    /// Returns the content hash and file ID
    pub async fn add_file(&self, data: Bytes, metadata: FileMetadata) -> Result<(String, Hash)> {
        // Add blob to Iroh (content-addressed storage)
        let hash = self.iroh.add_blob(data).await?;

        // Store metadata in SQLite
        let file_id = metadata::store_file_metadata(&self.db, &hash, metadata)?;

        // Replicate if auto-replication enabled
        if self.config.auto_replicate {
            self.replication
                .replicate_blob(&hash, self.config.replication_factor)
                .await?;
        }

        Ok((file_id, hash))
    }

    /// Get a file from distributed storage
    pub async fn get_file(&self, file_id: &str) -> Result<(Bytes, FileRecord)> {
        // Look up metadata to get hash
        let record = metadata::get_file_metadata(&self.db, file_id)?;

        // Fetch blob from Iroh (local or from mesh)
        let hash_bytes =
            hex::decode(&record.hash).map_err(|e| anyhow::anyhow!("Invalid hash: {}", e))?;
        let hash_array: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Hash must be 32 bytes"))?;
        let hash = Hash::from_bytes(hash_array);
        let data = self.iroh.get_blob(&hash).await?;

        Ok((data, record))
    }

    /// List files for a user
    pub async fn list_files(&self, user_id: &str) -> Result<Vec<FileRecord>> {
        metadata::list_user_files(&self.db, user_id)
    }

    /// Delete a file (removes metadata, optionally unpins blob)
    pub async fn delete_file(&self, file_id: &str, unpin: bool) -> Result<()> {
        let record = metadata::get_file_metadata(&self.db, file_id)?;

        // Remove metadata
        metadata::delete_file_metadata(&self.db, file_id)?;

        // Optionally unpin blob (allows GC)
        if unpin {
            let hash_bytes = hex::decode(&record.hash)?;
            let hash_array: [u8; 32] = hash_bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Hash must be 32 bytes"))?;
            let hash = Hash::from_bytes(hash_array);
            self.iroh.unpin_blob(&hash).await?;
        }

        Ok(())
    }

    /// Get storage statistics
    pub async fn stats(&self) -> Result<StorageStats> {
        let total_files = metadata::count_files(&self.db)?;
        let total_size = metadata::total_size(&self.db)?;
        let iroh_stats = self.iroh.stats().await?;

        Ok(StorageStats {
            total_files,
            total_size,
            blobs_stored: iroh_stats.num_blobs,
            storage_used_bytes: iroh_stats.size_bytes,
        })
    }

    /// Shutdown storage manager
    pub async fn shutdown(self) -> Result<()> {
        // Try to unwrap Arc to get ownership for shutdown
        match Arc::try_unwrap(self.iroh) {
            Ok(iroh) => iroh.shutdown().await,
            Err(_) => {
                tracing::warn!("Cannot shutdown Iroh node: other references exist");
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub struct StorageStats {
    pub total_files: u64,
    pub total_size: u64,
    pub blobs_stored: u64,
    pub storage_used_bytes: u64,
}
