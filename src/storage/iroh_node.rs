use anyhow::Result;
use bytes::Bytes;
use iroh::blobs::Hash;
use std::path::Path;
use tracing::info;

/// Wrapper around Iroh node for blob storage and transfer
pub struct IrohNodeManager {
    node: iroh::node::Node<iroh::blobs::store::fs::Store>,
}

impl IrohNodeManager {
    /// Create and start a new Iroh node
    pub async fn new(data_dir: &Path) -> Result<Self> {
        info!("Initializing Iroh node at {:?}", data_dir);

        // Create data directory if needed
        std::fs::create_dir_all(data_dir)?;

        // Build Iroh node
        let node = iroh::node::Node::persistent(data_dir)
            .await?
            .spawn()
            .await?;

        info!("Iroh node started: {}", node.node_id());

        Ok(Self { node })
    }

    /// Add a blob to Iroh storage
    ///
    /// Returns the content hash (BLAKE3)
    pub async fn add_blob(&self, data: Bytes) -> Result<Hash> {
        let client = self.node.client();
        let outcome = client.blobs().add_bytes(data).await?;

        info!("Added blob: {} ({} bytes)", outcome.hash, outcome.size);

        Ok(outcome.hash)
    }

    /// Get a blob from Iroh storage (local or fetch from mesh)
    pub async fn get_blob(&self, hash: &Hash) -> Result<Bytes> {
        let client = self.node.client();
        let data = client.blobs().read_to_bytes(*hash).await?;

        info!("Retrieved blob: {} ({} bytes)", hash, data.len());

        Ok(data)
    }

    /// Pin a blob (prevent garbage collection)
    pub async fn pin_blob(&self, hash: &Hash) -> Result<()> {
        // In Iroh, blobs are automatically pinned when added
        // This is a placeholder for future explicit pinning logic
        info!("Pinned blob: {}", hash);
        Ok(())
    }

    /// Unpin a blob (allow garbage collection)
    pub async fn unpin_blob(&self, hash: &Hash) -> Result<()> {
        // TODO: Implement unpinning when Iroh API supports it
        info!("Unpinned blob: {}", hash);
        Ok(())
    }

    /// Get node statistics
    pub async fn stats(&self) -> Result<NodeStats> {
        // TODO: Query actual Iroh stats
        // For now, return placeholder values
        Ok(NodeStats {
            num_blobs: 0,
            size_bytes: 0,
        })
    }

    /// Get the Iroh node ID
    pub fn node_id(&self) -> String {
        self.node.node_id().to_string()
    }

    /// Shutdown the Iroh node
    pub async fn shutdown(self) -> Result<()> {
        info!("Shutting down Iroh node");
        self.node.shutdown().await?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct NodeStats {
    pub num_blobs: u64,
    pub size_bytes: u64,
}
