use futures_lite::StreamExt;
use iroh_blobs::store::fs::FsStore;
use iroh_docs::engine::LiveEvent;
use iroh_docs::protocol::Docs;
use iroh_docs::store::Query;
use iroh_docs::{AuthorId, DocTicket, NamespaceId};
use rusqlite::params;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::db::DbPool;

/// Manages the shared iroh-docs document for mesh catalog replication.
pub struct CatalogSync {
    docs: Docs,
    blobs: FsStore,
    author: AuthorId,
    namespace: NamespaceId,
    pool: DbPool,
    node_id: String,
}

/// JSON structure for catalog entries stored in iroh-docs.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct CatalogEntryMeta {
    filename: String,
    dir: String,
    path: String,
    size: i64,
    mime: Option<String>,
    file_type: String,
    modified: Option<String>,
    thumbnail_cid: Option<String>,
    origin_node: String,
}

impl CatalogSync {
    /// Create a new CatalogSync that uses the given docs and blobs instances.
    /// Creates or opens the shared catalog document.
    pub async fn new(
        docs: Docs,
        blobs: FsStore,
        pool: DbPool,
        node_id: String,
    ) -> anyhow::Result<Self> {
        let api = docs.api();

        // Create a persistent author for this node
        let author = api.author_create().await?;

        // Try to open an existing catalog document, or create a new one.
        let namespace = Self::get_or_create_catalog_namespace(&docs).await?;

        tracing::info!(
            "Catalog sync initialized: namespace={}, author={}",
            namespace,
            author,
        );

        Ok(CatalogSync {
            docs,
            blobs,
            author,
            namespace,
            pool,
            node_id,
        })
    }

    /// Get or create the catalog namespace.
    async fn get_or_create_catalog_namespace(docs: &Docs) -> anyhow::Result<NamespaceId> {
        let api = docs.api();
        let mut doc_list = api.list().await?;

        // Use the first existing document as our catalog
        if let Some(Ok((namespace, _))) = doc_list.next().await {
            tracing::info!("Using existing catalog document: {}", namespace);
            return Ok(namespace);
        }

        // Create a new document
        let doc = api.create().await?;
        let namespace = doc.id();
        tracing::info!("Created new catalog document: {}", namespace);
        Ok(namespace)
    }

    /// Get a ticket that other nodes can use to join this catalog.
    pub async fn share_ticket(&self) -> anyhow::Result<DocTicket> {
        let api = self.docs.api();
        let doc = api
            .open(self.namespace)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Catalog document not found"))?;
        let ticket = doc
            .share(
                iroh_docs::api::protocol::ShareMode::Write,
                iroh_docs::api::protocol::AddrInfoOptions::RelayAndAddresses,
            )
            .await?;
        Ok(ticket)
    }

    /// Join an existing catalog from a remote node's ticket.
    pub async fn join_from_ticket(
        docs: Docs,
        blobs: FsStore,
        pool: DbPool,
        node_id: String,
        ticket: DocTicket,
    ) -> anyhow::Result<Self> {
        let api = docs.api();
        let author = api.author_create().await?;
        let doc = api.import(ticket).await?;
        let namespace = doc.id();

        tracing::info!("Joined remote catalog: namespace={}", namespace);

        Ok(CatalogSync {
            docs,
            blobs,
            author,
            namespace,
            pool,
            node_id,
        })
    }

    /// Publish a local catalog entry to the iroh-docs document.
    /// Called by the indexer after hashing and storing a file.
    pub async fn publish_entry(
        &self,
        cid: &str,
        filename: &str,
        dir: &str,
        path: &str,
        size: i64,
        mime: Option<&str>,
        file_type: &str,
        modified: Option<&str>,
        thumbnail_bytes: Option<&[u8]>,
    ) -> anyhow::Result<()> {
        let api = self.docs.api();
        let doc = api
            .open(self.namespace)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Catalog document not found"))?;

        // If there's a thumbnail, store it as a blob and reference its hash
        let thumbnail_cid = if let Some(thumb) = thumbnail_bytes {
            let tag_info = self.blobs.add_slice(thumb).await?;
            Some(tag_info.hash.to_hex().to_string())
        } else {
            None
        };

        let meta = CatalogEntryMeta {
            filename: filename.to_string(),
            dir: dir.to_string(),
            path: path.to_string(),
            size,
            mime: mime.map(|s| s.to_string()),
            file_type: file_type.to_string(),
            modified: modified.map(|s| s.to_string()),
            thumbnail_cid,
            origin_node: self.node_id.clone(),
        };

        let value = serde_json::to_vec(&meta)?;
        doc.set_bytes(self.author, cid.as_bytes().to_vec(), value)
            .await?;

        Ok(())
    }

    /// Subscribe to remote catalog changes and ingest them into the local DB.
    /// Runs as a background task.
    pub async fn subscribe_and_ingest(sync: Arc<Mutex<CatalogSync>>) -> anyhow::Result<()> {
        let (namespace, pool, docs, blobs) = {
            let s = sync.lock().await;
            (s.namespace, s.pool.clone(), s.docs.clone(), s.blobs.clone())
        };

        let api = docs.api();
        let doc = api
            .open(namespace)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Catalog document not found"))?;

        let mut events = doc.subscribe().await?;

        while let Some(event) = events.next().await {
            let event = match event {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Catalog subscription error: {e}");
                    continue;
                }
            };

            // Process remote insert events
            if let LiveEvent::InsertRemote { entry, content_status, .. } = event {
                // Only process if content is complete
                if !matches!(content_status, iroh_docs::sync::ContentStatus::Complete) {
                    continue;
                }

                let key = entry.key().to_vec();
                let cid = match String::from_utf8(key) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                // Read entry content from blob store
                let content_hash = entry.content_hash();
                let content = match blobs.get_bytes(content_hash).await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("Failed to read catalog entry content: {e}");
                        continue;
                    }
                };

                let meta: CatalogEntryMeta = match serde_json::from_slice(&content) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("Failed to parse catalog entry: {e}");
                        continue;
                    }
                };

                // Ingest into local database
                if let Err(e) = ingest_remote_entry(&pool, &cid, &meta, &blobs).await {
                    tracing::warn!("Failed to ingest remote catalog entry {cid}: {e}");
                }
            }
        }

        Ok(())
    }

    /// Do an initial sync by reading all existing entries from the document.
    pub async fn initial_sync(&self) -> anyhow::Result<u64> {
        let api = self.docs.api();
        let doc = api
            .open(self.namespace)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Catalog document not found"))?;

        let entries = doc.get_many(Query::all()).await?;
        let mut entries = std::pin::pin!(entries);
        let mut count = 0u64;

        while let Some(Ok(entry)) = entries.next().await {
            let key = entry.key().to_vec();
            let cid = match String::from_utf8(key) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let content_hash = entry.content_hash();
            let content = match self.blobs.get_bytes(content_hash).await {
                Ok(c) => c,
                Err(_) => continue,
            };

            let meta: CatalogEntryMeta = match serde_json::from_slice(&content) {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Skip our own entries
            if meta.origin_node == self.node_id {
                continue;
            }

            if let Err(e) = ingest_remote_entry(&self.pool, &cid, &meta, &self.blobs).await {
                tracing::debug!("Failed to ingest entry {cid}: {e}");
                continue;
            }
            count += 1;
        }

        tracing::info!("Initial catalog sync: ingested {count} remote entries");
        Ok(count)
    }
}

/// Ingest a single remote catalog entry into the local SQLite database.
async fn ingest_remote_entry(
    pool: &DbPool,
    cid: &str,
    meta: &CatalogEntryMeta,
    blobs: &FsStore,
) -> anyhow::Result<()> {
    let conn = pool.get()?;

    // Upsert into content_index
    conn.execute(
        "INSERT INTO content_index (cid, dir, path, filename, size, mime, file_type, modified, origin_node, is_local)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
         ON CONFLICT(cid) DO UPDATE SET
           dir = excluded.dir,
           path = excluded.path,
           filename = excluded.filename,
           size = excluded.size,
           mime = excluded.mime,
           file_type = excluded.file_type,
           modified = excluded.modified,
           origin_node = excluded.origin_node,
           is_local = excluded.is_local,
           indexed_at = datetime('now')",
        params![
            cid,
            meta.dir,
            meta.path,
            meta.filename,
            meta.size,
            meta.mime,
            meta.file_type,
            meta.modified,
            meta.origin_node,
        ],
    )?;

    // Fetch and store thumbnail if referenced
    if let Some(ref thumb_hex) = meta.thumbnail_cid {
        let has_thumb: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM content_thumbnails WHERE cid = ?1",
                params![cid],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if !has_thumb {
            if let Ok(hash) = thumb_hex.parse::<iroh_blobs::Hash>() {
                match blobs.get_bytes(hash).await {
                    Ok(thumb_bytes) => {
                        if let Ok(img) = image::load_from_memory(&thumb_bytes) {
                            let width = img.width() as i32;
                            let height = img.height() as i32;
                            conn.execute(
                                "INSERT OR REPLACE INTO content_thumbnails (cid, thumbnail, width, height)
                                 VALUES (?1, ?2, ?3, ?4)",
                                params![cid, thumb_bytes.as_ref(), width, height],
                            )?;
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Failed to fetch thumbnail blob {thumb_hex}: {e}");
                    }
                }
            }
        }
    }

    tracing::debug!(
        "Ingested remote entry: cid={}, file={}, from={}",
        cid,
        meta.filename,
        meta.origin_node
    );
    Ok(())
}
