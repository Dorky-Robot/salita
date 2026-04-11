use std::path::Path;

use iroh::protocol::Router;
use iroh::Endpoint;
use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{BlobsProtocol, ALPN as BLOBS_ALPN};
use iroh_docs::protocol::Docs;
use iroh_docs::ALPN as DOCS_ALPN;
use iroh_gossip::net::Gossip;
use iroh_gossip::ALPN as GOSSIP_ALPN;

/// Holds the iroh node components needed by the rest of salita.
pub struct IrohNode {
    pub endpoint: Endpoint,
    pub blobs: FsStore,
    pub docs: Docs,
    pub gossip: Gossip,
    pub router: Router,
}

impl IrohNode {
    /// Start the iroh node with persistent storage under `data_dir`.
    ///
    /// Creates:
    /// - `data_dir/iroh-blobs/` for blob storage
    /// - `data_dir/iroh-docs/` for document storage
    pub async fn start(data_dir: &Path) -> anyhow::Result<Self> {
        let blobs_dir = data_dir.join("iroh-blobs");
        let docs_dir = data_dir.join("iroh-docs");
        std::fs::create_dir_all(&blobs_dir)?;
        std::fs::create_dir_all(&docs_dir)?;

        // Build the iroh endpoint with default discovery (mDNS + DNS)
        let endpoint = Endpoint::builder().bind().await?;

        tracing::info!("iroh node started: {}", endpoint.id());

        // Blob storage on filesystem
        let blobs = FsStore::load(&blobs_dir).await?;

        // Gossip protocol for doc sync
        let gossip: Gossip = Gossip::builder().spawn(endpoint.clone());

        // Docs protocol with persistent storage
        let docs = Docs::persistent(docs_dir)
            .spawn(endpoint.clone(), (*blobs).clone(), gossip.clone())
            .await?;

        // Wire up all protocols on the iroh router
        let router = Router::builder(endpoint.clone())
            .accept(BLOBS_ALPN, BlobsProtocol::new(&blobs, None))
            .accept(GOSSIP_ALPN, gossip.clone())
            .accept(DOCS_ALPN, docs.clone())
            .spawn();

        Ok(IrohNode {
            endpoint,
            blobs,
            docs,
            gossip,
            router,
        })
    }

    /// Shutdown the iroh node gracefully.
    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.router.shutdown().await?;
        Ok(())
    }
}
