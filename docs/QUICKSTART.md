# Distributed Storage - Quick Start

## What Was Set Up

Your new worktree is at: `/Users/felixflores/Projects/dorky_robot/salita-distributed-storage`

This branch (`feat/distributed-storage`) includes:

1. **Iroh integration** - Content-addressed blob storage
2. **Storage module** (`src/storage/`) - File management with SQLite metadata
3. **Database migration** - Tables for files, blob locations, sharing
4. **Architecture docs** - Full design in `docs/IROH_INTEGRATION.md`

## Structure

```
src/storage/
├── mod.rs              # StorageManager - main API
├── iroh_node.rs        # Iroh node wrapper (add/get blobs)
├── metadata.rs         # SQLite file metadata management
└── replication.rs      # Multi-node replication (TODO)
```

## Next Steps

### 1. Build and test the basics

```bash
cd /Users/felixflores/Projects/dorky_robot/salita-distributed-storage

# Build (will download Iroh dependency)
cargo build

# Run database migrations
cargo run -- migrate  # (or however you run migrations)
```

### 2. Initialize storage in main.rs

Add to `main.rs` after DB initialization:

```rust
// Initialize distributed storage
let storage_config = storage::StorageConfig {
    data_dir: data_dir.join("iroh"),
    replication_factor: 3,
    auto_replicate: true,
    max_storage_gb: 100,
};

let storage_manager = storage::StorageManager::new(pool.clone(), storage_config)
    .await
    .expect("Failed to initialize storage");
```

Add to `AppState`:
```rust
pub struct AppState {
    // ... existing fields
    pub storage: Arc<storage::StorageManager>,
}
```

### 3. Add GraphQL API

Create `src/graphql/storage.rs`:

```rust
use async_graphql::{Context, Object, Result};
use bytes::Bytes;

#[derive(Default)]
pub struct StorageQuery;

#[Object]
impl StorageQuery {
    async fn files(&self, ctx: &Context<'_>) -> Result<Vec<File>> {
        let state = ctx.data::<AppState>()?;
        let user = ctx.data::<User>()?;  // From auth

        let records = state.storage.list_files(&user.id).await?;
        Ok(records.into_iter().map(File::from).collect())
    }
}

#[derive(Default)]
pub struct StorageMutation;

#[Object]
impl StorageMutation {
    async fn upload_file(
        &self,
        ctx: &Context<'_>,
        name: String,
        data: String,  // Base64 encoded
    ) -> Result<File> {
        let state = ctx.data::<AppState>()?;
        let user = ctx.data::<User>()?;

        let decoded = base64::decode(&data)?;
        let metadata = FileMetadata {
            name,
            mime_type: Some("application/octet-stream".to_string()),
            owner_id: user.id.clone(),
            size: decoded.len() as u64,
        };

        let (file_id, hash) = state.storage
            .add_file(Bytes::from(decoded), metadata)
            .await?;

        Ok(File { id: file_id, hash: hash.to_string(), ... })
    }
}
```

### 4. Test it

```bash
# Start Salita
cargo run

# Upload a file via GraphQL
curl -X POST http://localhost:6969/graphql \
  -H "Content-Type: application/json" \
  -d '{
    "query": "mutation { uploadFile(name: \"test.txt\", data: \"SGVsbG8gV29ybGQ=\") { id hash name size } }"
  }'
```

### 5. Add multi-node replication (Phase 2)

Once basic storage works:

1. Implement `replication.rs` - query mesh_nodes, send replication requests
2. Add gossip integration - nodes announce what blobs they have
3. Add blob transfer endpoint - nodes can request blobs from each other
4. Track replication in `blob_locations` table

## Testing Locally (Multi-Node)

```bash
# Terminal 1: First Salita instance
cargo run -- --port 6969 --data-dir ~/.salita/node1

# Terminal 2: Second Salita instance
cargo run -- --port 6970 --data-dir ~/.salita/node2

# Upload file to node1, verify it replicates to node2
```

## Key Files to Read

- `docs/IROH_INTEGRATION.md` - Full architecture and design decisions
- `src/storage/mod.rs` - Main storage API
- `migrations/009_distributed_storage.sql` - Database schema

## Helpful Resources

- Iroh docs: https://iroh.computer/docs
- Iroh examples: https://github.com/n0-computer/iroh/tree/main/iroh/examples
- Iroh Discord: https://discord.gg/iroh (if you get stuck)

## Current Limitations (TODOs)

- ⬜ Replication is stubbed out (single-node only for now)
- ⬜ No web UI yet (GraphQL only)
- ⬜ No sharing links implemented
- ⬜ No chunking for large files (whole-file blobs only)
- ⬜ No garbage collection

These are all Phase 2+ features. Get the basics working first!
