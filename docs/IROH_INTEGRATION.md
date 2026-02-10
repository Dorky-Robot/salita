# Iroh-Based Distributed Storage

> Leverage Iroh for content-addressed, mesh-native distributed file storage

## Overview

Iroh provides the perfect foundation for Salita's distributed storage:
- **Content-addressed blobs** - files/chunks identified by hash
- **Gossip protocol** - mesh nodes discover content automatically
- **Direct transfers** - peer-to-peer blob transfers
- **Private network** - isolated from public internet
- **Rust-native** - clean integration with existing stack

## Architecture

```
┌─────────────────────────────────────────────────┐
│         Salita Storage API (GraphQL/REST)        │
│    (upload, download, list files, sharing)       │
└───────────────┬─────────────────────────────────┘
                │
┌───────────────▼─────────────────────────────────┐
│           Storage Manager (Rust)                 │
│  - File metadata (SQLite)                        │
│  - User permissions & sharing                    │
│  - Chunking strategy (optional, or whole files)  │
└───────────┬──────────────────┬──────────────────┘
            │                  │
            │         ┌────────▼──────────┐
            │         │  Iroh Node (per   │
            │         │   Salita instance)│
            │         │                   │
            │         │ - Content blobs   │
            │         │ - Gossip protocol │
            │         │ - P2P transfers   │
            │         └────────┬──────────┘
            │                  │
┌───────────▼──────────────────▼──────────────────┐
│              Mesh Network Layer                  │
│   (Existing mesh_nodes, node discovery)          │
└─────────────────────────────────────────────────┘
```

## How Iroh Works (Simplified)

1. **Content Addressing**:
   - Add blob → get BLAKE3 hash
   - Blobs immutable, identified by hash
   - Same content = same hash (deduplication)

2. **Gossip**:
   - Nodes announce what content they have
   - Queries propagate through mesh
   - "Who has blob XYZ?" → responses come back

3. **Transfers**:
   - Request blob by hash from peer
   - Direct QUIC connection
   - Verified on receipt (hash check)

4. **Private Network**:
   - Each Salita mesh = isolated Iroh network
   - Nodes only connect to mesh members
   - No exposure to public IPFS/DHT

## Integration Plan

### Phase 1: Single-Node Storage (Iroh basics)
**Goal**: Upload/download files on one Salita node using Iroh

```rust
// src/storage/mod.rs - Core storage manager
pub struct StorageManager {
    iroh_node: iroh::node::Node,
    db: DbPool,
}

impl StorageManager {
    /// Upload a file, returns content hash
    pub async fn add_file(&self, data: &[u8], metadata: FileMetadata)
        -> Result<Hash> {
        // Add to Iroh (content-addressed)
        let hash = self.iroh_node.blobs().add_bytes(data).await?;

        // Store metadata in SQLite
        self.db.execute(
            "INSERT INTO files_dss (id, hash, name, size, owner_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![metadata.id, hash.to_string(), ...]
        )?;

        Ok(hash)
    }

    /// Download a file by hash
    pub async fn get_file(&self, hash: &Hash) -> Result<Bytes> {
        self.iroh_node.blobs().read_to_bytes(hash).await
    }
}
```

**Database**:
```sql
CREATE TABLE files_dss (
    id TEXT PRIMARY KEY,              -- UUIDv7
    hash TEXT NOT NULL,                -- Iroh blob hash (BLAKE3)
    name TEXT NOT NULL,                -- Original filename
    mime_type TEXT,
    size INTEGER NOT NULL,
    owner_id TEXT NOT NULL,            -- User who uploaded
    created_at TEXT NOT NULL,
    metadata TEXT,                     -- JSON: tags, description, etc.
    FOREIGN KEY (owner_id) REFERENCES users(id)
);
```

**API**:
```graphql
type File {
  id: ID!
  name: String!
  size: Int!
  mimeType: String
  hash: String!                # Iroh content hash
  owner: User!
  createdAt: DateTime!
}

type Mutation {
  uploadFile(name: String!, data: String!): File!
}

type Query {
  file(id: ID!): File
  files: [File!]!
}
```

### Phase 2: Multi-Node Replication
**Goal**: Files automatically replicate across mesh nodes

```rust
// Storage config
pub struct StorageConfig {
    replication_factor: usize,  // How many copies (default: 3)
    auto_replicate: bool,        // Auto-replicate on upload
}

impl StorageManager {
    /// Pin blob to N nodes in the mesh
    pub async fn replicate(&self, hash: &Hash, num_copies: usize)
        -> Result<()> {
        // Get mesh nodes from DB
        let nodes = self.get_mesh_nodes().await?;

        // Select target nodes (strategy: diverse, available)
        let targets = self.select_replication_targets(&nodes, num_copies)?;

        // Ask each node to fetch and pin the blob
        for node in targets {
            self.request_pin(node, hash).await?;
        }

        Ok(())
    }
}
```

**Gossip integration**:
- Each Salita node announces its Iroh endpoint via mesh
- Nodes subscribe to `salita-storage` gossip topic
- Replication requests sent via gossip → nodes fetch blobs

### Phase 3: Smart Chunking (Optional)
**Goal**: Large files split into chunks for efficiency

- Files > 10MB split into 1MB chunks
- Each chunk is separate Iroh blob
- Merkle tree tracks chunks → file mapping
- Parallel download from multiple nodes

```rust
pub struct ChunkedFile {
    file_id: String,
    chunks: Vec<Hash>,      // Ordered chunk hashes
    chunk_size: usize,
    total_size: usize,
}
```

### Phase 4: Web UI & Sharing
**Goal**: User-friendly file browser and sharing

- Upload: drag-drop files, shows replication status
- Browse: folder-like view (virtual paths in DB)
- Share: generate share links (one-time or persistent)
- Permissions: owner-only, specific users, or public (mesh-wide)

## Configuration

```toml
# config.toml
[storage]
enabled = true
data_dir = "~/.salita/storage"  # Iroh blob storage
replication_factor = 3           # Copies per file
max_storage_gb = 100             # Storage quota per node
auto_replicate = true            # Replicate on upload

[storage.iroh]
gossip_enabled = true
max_blob_size_mb = 1000         # Reject files > 1GB
```

## Code Structure

```
src/storage/
├── mod.rs              # StorageManager, main API
├── iroh_node.rs        # Iroh node lifecycle, config
├── replication.rs      # Replication strategy, pinning
├── metadata.rs         # SQLite metadata management
├── chunking.rs         # Optional chunking logic
└── sharing.rs          # Share links, permissions

src/routes/storage.rs   # HTTP/GraphQL endpoints
src/graphql/storage.rs  # GraphQL schema for files
```

## Migration from Current State

1. **Add storage module** (`src/storage/mod.rs`)
2. **Initialize Iroh node** in `main.rs` alongside mesh setup
3. **Add storage routes** to router
4. **Extend AppState** with `StorageManager`
5. **Database migration** for `files_dss` table

## Key Decisions

### Chunking: Start Simple
- **Phase 1**: Store whole files as single blobs (simpler)
- **Phase 2+**: Add chunking for large files if needed
- Iroh handles blob splitting internally anyway (QUIC streams)

### Replication: Manual → Automatic
- **Phase 1**: Manual replication (user chooses nodes)
- **Phase 2**: Automatic based on config (N copies)
- **Phase 3**: Smart placement (consider node capacity, network topology)

### Namespacing: Virtual Paths
- Iroh blobs are flat (hash-addressed)
- Salita adds filesystem-like paths in SQLite
- User sees `/photos/2024/vacation.jpg`
- Backend stores path → hash mapping

### Permissions: Salita-Managed
- Iroh has no built-in permissions
- Salita enforces via existing user auth
- Only show files user owns or has access to
- Share links = temporary access tokens

## Performance Characteristics

**Iroh strengths**:
- Fast BLAKE3 hashing
- QUIC for efficient transfers
- Built-in deduplication
- Streaming support

**Expected performance** (gigabit network):
- Upload 100MB file: ~1-2 seconds (hash + store)
- Replicate to 3 nodes: +2-3 seconds
- Download from mesh: ~1 second (once located)
- Gossip propagation: < 1 second

## Security

- **Content integrity**: BLAKE3 hash verification
- **Mesh isolation**: Private Iroh network (no public DHT)
- **Access control**: Salita auth layer (not Iroh-level)
- **Encryption at rest**: Future - encrypt blobs before Iroh

## Next Steps

1. ✅ Add `iroh` dependency to Cargo.toml
2. ⬜ Create `src/storage/mod.rs` with StorageManager skeleton
3. ⬜ Initialize Iroh node in `main.rs`
4. ⬜ Add database migration for `files_dss` table
5. ⬜ Implement basic upload/download in GraphQL
6. ⬜ Test single-node storage
7. ⬜ Add multi-node replication logic
8. ⬜ Build web UI for file management

## Resources

- Iroh docs: https://iroh.computer/docs
- Iroh examples: https://github.com/n0-computer/iroh/tree/main/iroh/examples
- Gossip guide: https://iroh.computer/docs/layers/gossip
