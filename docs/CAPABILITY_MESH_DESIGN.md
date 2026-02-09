# Capability Mesh Design

**Status:** Active Design
**Chosen from:** [MESH_CONCEPT.md](./MESH_CONCEPT.md) Option 3

---

## Vision

Each node in your Salita mesh has **specializations** based on what it's good at. When you want to do something, the mesh routes the request to the right node.

**Example Flow:**
```
You're on your phone and want to upload a 4K video:
  → Phone knows: "I can capture, but I can't process this"
  → Phone asks mesh: "who can handle media transcoding?"
  → Desktop responds: "I can!"
  → Phone sends video to desktop
  → Desktop transcodes, generates thumbnails, stores
  → Desktop broadcasts: "video ready at https://desktop.local/media/video-123.mp4"
  → Phone updates post with final video URL
```

---

## Core Capabilities

### Primary Capabilities

| Capability | Description | Typical Nodes |
|------------|-------------|---------------|
| `posting` | Can create posts, comments, reactions | All nodes |
| `media.capture` | Can capture photos/videos | Phone, laptop |
| `media.upload` | Can receive media uploads | Desktop, pi |
| `media.storage` | Can store large media files (GB+) | Desktop, pi, NAS |
| `media.transcode` | Can process video/images | Desktop (GPU), pi |
| `media.serve` | Can serve media files publicly | Pi, desktop with ngrok |
| `always_on` | Available 24/7 | Pi, server |
| `compute.background` | Can run background jobs | Pi, desktop |
| `public_endpoint` | Has public URL | Node with ngrok/domain |
| `sync.primary` | Source of truth for data | Desktop or pi |

### Capability Combinations

Nodes have **profiles** based on their capabilities:

**Phone** (Mobile Client)
```json
["posting", "media.capture", "media.upload"]
```
- Create posts quickly
- Capture photos/videos
- Upload to capable nodes
- Lightweight, battery-conscious

**Laptop** (Portable Workstation)
```json
["posting", "media.capture", "media.upload", "media.transcode", "compute.background"]
```
- Full posting experience
- Can process media when plugged in
- Sometimes offline
- Syncs when available

**Desktop** (Power Node)
```json
[
  "posting",
  "media.upload",
  "media.storage",
  "media.transcode",
  "media.serve",
  "compute.background",
  "sync.primary"
]
```
- Full capabilities
- GPU for transcoding
- Large storage
- Often always-on
- Can be primary sync node

**Raspberry Pi** (Always-On Server)
```json
[
  "media.storage",
  "media.serve",
  "always_on",
  "public_endpoint",
  "compute.background",
  "sync.primary"
]
```
- 24/7 availability
- Public endpoint (ngrok or local network)
- Background job processor
- Reliable sync source
- Lower compute power (no GPU)

---

## Capability Discovery & Routing

### Registration

When a node joins the mesh, it advertises its capabilities:

```rust
// On phone
POST /mesh/register
{
  "name": "Felix's iPhone",
  "hostname": "192.168.1.50",
  "port": 6969,
  "capabilities": [
    "posting",
    "media.capture",
    "media.upload"
  ]
}
```

### Capability Query

Any node can ask: "Who can do X?"

```graphql
query {
  capableNodes(capability: "media.transcode") {
    id
    name
    hostname
    port
    status
  }
}
```

Returns:
```json
{
  "data": {
    "capableNodes": [
      {
        "id": "desktop-123",
        "name": "Felix's Desktop",
        "hostname": "192.168.1.100",
        "port": 6969,
        "status": "ONLINE"
      }
    ]
  }
}
```

### Routing Strategy

When you need a capability:

1. **Local first:** Check if current node can handle it
2. **Prefer always-on:** Choose nodes with `always_on` capability
3. **Prefer online:** Only route to online nodes
4. **Load balance:** If multiple nodes, pick least busy
5. **Fallback:** Degrade gracefully if no capable node found

---

## Use Cases

### Use Case 1: Mobile Video Upload

**Actors:** Phone (client), Desktop (processor)

**Flow:**
```
1. User records 4K video on phone (2GB)
2. Phone creates draft post: { body: "Check this out!", media_status: "pending" }
3. Phone queries mesh: capableNodes("media.transcode")
4. Desktop responds (online, GPU available)
5. Phone uploads video to desktop: POST /media/upload
6. Desktop:
   - Stores original
   - Transcodes to 1080p, 720p, 480p
   - Generates thumbnail
   - Returns URLs
7. Phone updates post: { media_status: "ready", video_url: "https://..." }
8. Desktop broadcasts to mesh: "post-123 media ready"
9. All nodes fetch updated post
```

**API:**
```http
# Phone → Desktop
POST https://desktop.local:6969/media/upload
Content-Type: multipart/form-data

file: [2GB video]
post_id: "post-123"
requested_formats: ["1080p", "720p", "480p"]

# Desktop responds
{
  "status": "processing",
  "job_id": "job-456",
  "estimated_time": "120s"
}

# Desktop → Phone (SSE or WebSocket)
event: transcode_progress
data: { "job_id": "job-456", "progress": 0.45 }

event: transcode_complete
data: {
  "job_id": "job-456",
  "urls": {
    "original": "https://desktop.local/media/abc123.mp4",
    "1080p": "https://desktop.local/media/abc123-1080p.mp4",
    "720p": "https://desktop.local/media/abc123-720p.mp4",
    "thumbnail": "https://desktop.local/media/abc123-thumb.jpg"
  }
}
```

---

### Use Case 2: Background Job Delegation

**Actors:** Laptop (trigger), Pi (worker)

**Flow:**
```
1. User schedules daily photo backup on laptop
2. Laptop queries mesh: capableNodes("always_on")
3. Pi responds
4. Laptop delegates: POST /jobs/schedule to Pi
5. Pi runs job at scheduled time
6. Pi notifies laptop when complete (if online)
```

---

### Use Case 3: Public Sharing

**Actors:** Phone (client), Pi (public server)

**Flow:**
```
1. User creates post with photo on phone
2. User clicks "Make Public"
3. Phone queries mesh: capableNodes("public_endpoint")
4. Pi responds (has ngrok tunnel)
5. Phone sends photo to Pi: POST /media/upload?public=true
6. Pi stores photo, returns public URL
7. Phone updates post with public URL
8. Anyone can access: https://felix-pi.ngrok.app/media/photo-123.jpg
```

---

## Data Model Changes

### Capabilities Schema

```sql
-- Already exists in mesh_nodes:
-- capabilities TEXT NOT NULL DEFAULT '[]'

-- Add capability metadata table
CREATE TABLE IF NOT EXISTS node_capability_meta (
  node_id TEXT NOT NULL,
  capability TEXT NOT NULL,
  metadata TEXT, -- JSON: { "max_file_size": "10GB", "formats": ["mp4", "webm"] }
  last_updated TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (node_id, capability),
  FOREIGN KEY (node_id) REFERENCES mesh_nodes(id) ON DELETE CASCADE
);

-- Track media delegation
CREATE TABLE IF NOT EXISTS media_jobs (
  id TEXT PRIMARY KEY,
  post_id TEXT,
  source_node_id TEXT NOT NULL, -- Node that initiated
  worker_node_id TEXT NOT NULL, -- Node processing
  job_type TEXT NOT NULL,       -- 'transcode', 'thumbnail', 'upload'
  status TEXT NOT NULL,          -- 'pending', 'processing', 'completed', 'failed'
  progress REAL,                 -- 0.0 to 1.0
  input_url TEXT,
  output_urls TEXT,              -- JSON array
  error_message TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  completed_at TEXT,
  FOREIGN KEY (source_node_id) REFERENCES mesh_nodes(id),
  FOREIGN KEY (worker_node_id) REFERENCES mesh_nodes(id)
);

CREATE INDEX idx_media_jobs_status ON media_jobs(status);
CREATE INDEX idx_media_jobs_worker ON media_jobs(worker_node_id);
```

---

## GraphQL API Extensions

```graphql
type Query {
  # Existing
  nodes: [MeshNode!]!
  currentNode: MeshNode!

  # New capability queries
  capableNodes(capability: String!): [MeshNode!]!
  nodeCapabilities(nodeId: String!): [NodeCapability!]!
  mediaJobs(status: JobStatus): [MediaJob!]!
}

type Mutation {
  # Existing
  registerNode(input: RegisterNodeInput!): NodeOperationResult!

  # New capability mutations
  updateCapabilities(nodeId: String!, capabilities: [String!]!): NodeOperationResult!
  delegateMediaJob(input: MediaJobInput!): MediaJob!
  cancelMediaJob(jobId: String!): NodeOperationResult!
}

type NodeCapability {
  capability: String!
  metadata: JSON
  lastUpdated: DateTime!
}

type MediaJob {
  id: String!
  postId: String
  sourceNode: MeshNode!
  workerNode: MeshNode!
  jobType: String!
  status: JobStatus!
  progress: Float
  inputUrl: String
  outputUrls: [String!]
  errorMessage: String
  createdAt: DateTime!
  completedAt: DateTime
}

enum JobStatus {
  PENDING
  PROCESSING
  COMPLETED
  FAILED
}

input MediaJobInput {
  postId: String
  workerNodeId: String
  jobType: String!
  inputUrl: String!
  options: JSON
}
```

---

## Implementation Phases

### Phase 1: Capability Registry ✅ (Mostly Done)
- [x] Capabilities field in mesh_nodes
- [x] GraphQL queries for nodes
- [ ] Add `capableNodes(capability)` query
- [ ] UI to view node capabilities

### Phase 2: Basic Delegation
- [ ] Media job table
- [ ] POST /media/upload endpoint
- [ ] Capability-based routing
- [ ] Job status tracking

### Phase 3: Smart Routing
- [ ] Load balancing across capable nodes
- [ ] Health-aware routing (skip degraded nodes)
- [ ] Fallback strategies
- [ ] Timeout handling

### Phase 4: Advanced Features
- [ ] SSE/WebSocket for job progress
- [ ] Automatic capability detection
- [ ] Capability health checks
- [ ] Job retry logic

---

## Open Questions

1. **Authentication between nodes:**
   - How does phone authenticate to desktop for uploads?
   - Mutual TLS? Shared secrets? Session tokens?

2. **Capability negotiation:**
   - Can nodes request capabilities dynamically?
   - "I need GPU for 5 minutes, can I borrow yours?"

3. **Conflict resolution:**
   - Two nodes both claim `sync.primary` - which wins?
   - Manual override or automatic election?

4. **Discovery:**
   - How do nodes find each other on LAN?
   - mDNS? Broadcast? Manual entry only?

5. **Offline operation:**
   - Phone uploads video but all capable nodes offline
   - Queue for later? Store locally? Fail gracefully?

---

## Next Steps

What aspect should we tackle first?

1. **Add `capableNodes()` query** - Easy win, unblocks UI
2. **Design media job system** - Core delegation mechanism
3. **Build phone → desktop upload flow** - Concrete use case
4. **Add capability UI to dashboard** - Make it visible

What feels most valuable to prototype?
