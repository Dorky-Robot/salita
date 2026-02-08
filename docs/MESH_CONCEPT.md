# The Mesh Concept: What Does It Mean to Be "In the Mesh"?

## Current State Analysis

Right now, Salita has:
- **mesh_nodes** table - stores devices you've added (laptop, phone, raspberry pi)
- **node_connections** table - tracks direct connections between nodes
- **Pairing flow** - QR + PIN to add devices to the mesh
- **GraphQL API** - query nodes, their status, and connections

But what does it actually **mean** to be in the mesh?

---

## The Core Question

When you add your phone to your laptop's mesh, what should happen?

### Current Implementation (Implicit)
- Your laptop stores: "Phone exists at 192.168.1.50:6969"
- Your phone stores: nothing (it doesn't know it's in a mesh)
- There's no shared state, no coordination, just a **registry**

This is more like a **contact list** than a mesh.

### What Could "Mesh" Mean?

Let's explore different interpretations:

---

## Option 1: The Personal Device Registry

**Mental Model:** "My mesh is the list of devices I own/control"

**What it provides:**
- Quick access to your other Salita instances
- Dashboard shows: "These are all your nodes"
- No data sharing, no coordination
- Just convenient bookmarks with health monitoring

**What changes:**
- Nothing architecture-wise - current model works
- Maybe rename "mesh" to "devices" or "my_network"
- Focus UX on "quick links to your other instances"

**Use case:**
```
You have Salita running on:
- Desktop (main, always on)
- Laptop (sometimes)
- Phone (rarely, for quick posts)

The "mesh" just lets you jump between them easily.
```

---

## Option 2: The Shared Identity Mesh

**Mental Model:** "All my devices share the same identity and data"

**What it provides:**
- Post from phone → appears on laptop instantly
- One user, multiple devices, synchronized state
- Like Apple's Handoff/Continuity for Salita

**What changes:**
- Nodes need to sync posts, comments, reactions
- Need conflict resolution (what if offline edits conflict?)
- Need to track "which device am I using?" for attribution
- Sessions work across all nodes (or each node has its own?)

**Data flow:**
```
You post from your phone:
  → Phone's local DB gets the post
  → Phone broadcasts to mesh: "new post created"
  → Desktop receives event, fetches post, inserts into local DB
  → All devices stay in sync
```

**Schema additions:**
```sql
-- Track which node created each piece of content
ALTER TABLE posts ADD COLUMN origin_node_id TEXT REFERENCES mesh_nodes(id);

-- Track sync state
CREATE TABLE sync_log (
  id TEXT PRIMARY KEY,
  entity_type TEXT NOT NULL, -- 'post', 'comment', 'reaction'
  entity_id TEXT NOT NULL,
  operation TEXT NOT NULL,   -- 'create', 'update', 'delete'
  node_id TEXT NOT NULL,
  synced_at TEXT NOT NULL
);
```

---

## Option 3: The Capability Mesh

**Mental Model:** "Nodes specialize - phone posts, desktop stores media, pi hosts services"

**What it provides:**
- Nodes have roles based on their capabilities
- Phone: lightweight, mobile posting
- Desktop: full UI, media processing
- Pi: always-on, background jobs, storage

**What changes:**
- Capabilities become first-class (already have the field!)
- Routing: "which node should handle this request?"
- Delegation: "phone wants to upload a video → send to desktop for processing"

**Capabilities taxonomy:**
```json
[
  "posting",        // Can create posts
  "media_upload",   // Can handle media uploads
  "media_storage",  // Can store large media files
  "media_transcode",// Can process video/images
  "always_on",      // Available 24/7
  "public_endpoint" // Has public URL (ngrok, domain)
]
```

**Use case:**
```
You take a photo on your phone.
Phone has: ["posting", "media_upload"]
Desktop has: ["media_storage", "media_transcode", "always_on"]

Flow:
1. Phone creates post with "media pending"
2. Phone asks mesh: "who can store this video?"
3. Desktop responds, receives upload
4. Desktop transcodes, stores, updates post with final URL
5. All nodes see the completed post
```

---

## Option 4: The Federated Mesh (ActivityPub-style)

**Mental Model:** "Each node is independent, but they all talk to each other"

**What it provides:**
- Each node is fully autonomous
- Nodes follow each other (even your own nodes)
- Your phone follows your desktop's feed
- Like running multiple Mastodon instances that happen to be yours

**What changes:**
- Each node has its own identity (not shared)
- Cross-posting becomes "republishing to another node"
- No sync needed - it's just federation between your own nodes

**User identity:**
```
Desktop: felix@desktop.local
Phone:   felix@phone.local
Pi:      felix@pi.home

They're distinct identities that happen to be controlled by the same person.
```

---

## Option 5: The Hybrid Mesh (Primary + Replicas)

**Mental Model:** "One node is primary, others are clients/replicas"

**What it provides:**
- Desktop (or Pi) is the "source of truth"
- Phone and laptop are thin clients
- All writes go to primary
- Replicas cache for offline access

**What changes:**
- Clear hierarchy: primary vs secondary nodes
- Simpler sync model (hub-and-spoke, not peer-to-peer)
- Primary can be on a Pi (always on, stable)
- Clients can work offline, sync when reconnected

**Schema:**
```sql
ALTER TABLE mesh_nodes ADD COLUMN role TEXT CHECK(role IN ('primary', 'replica'));
ALTER TABLE mesh_nodes ADD COLUMN primary_node_id TEXT REFERENCES mesh_nodes(id);
```

---

## Questions to Answer

To decide which direction to take, consider:

1. **Offline behavior:** What happens when a node is offline?
   - Option 1: Nothing, each node is independent
   - Option 2: Sync when back online (complex)
   - Option 5: Replica works with cached data, syncs when primary reachable

2. **Data ownership:** Where does data "live"?
   - Option 1: Each node only knows about itself
   - Option 2: Data lives on all nodes (replicated)
   - Option 3: Data lives where capabilities match
   - Option 5: Data lives on primary, cached on replicas

3. **User experience:** What's the main use case?
   - "I want quick links to my other devices" → Option 1
   - "I want one seamless experience across devices" → Option 2 or 5
   - "I want my devices to have different jobs" → Option 3
   - "I want to run multiple identities" → Option 4

4. **Complexity budget:** How much are you willing to build?
   - Option 1: Almost nothing (current state works)
   - Option 2/3: Significant sync/coordination layer
   - Option 4: Full federation protocol
   - Option 5: Medium complexity, hub-and-spoke

---

## Recommendation: Start with Option 1, Path to Option 5

**Phase 1 (Current):** Device Registry
- Rename "mesh" to something clearer ("My Devices"?)
- Keep it simple: just a list of your Salita instances
- Focus on health monitoring and quick access

**Phase 2 (Future):** Primary + Replicas
- Designate one node as primary (usually desktop or pi)
- Other nodes become "sync clients"
- Start with read-only sync (posts from primary → replicas)
- Later add write sync with conflict resolution

**Why this path:**
- Clear incremental story
- Don't over-engineer early
- Each phase has clear value
- Can always pivot to Option 2 or 3 later if needed

---

## Design Principles for the Mesh

Regardless of which option you choose:

1. **Explicit is better than implicit**
   - If nodes sync, make it visible
   - If they don't, make that clear too
   - Don't pretend there's a mesh if there isn't one

2. **Local-first**
   - Each node should work independently
   - Mesh features are enhancements, not requirements
   - Never block local operations waiting for remote nodes

3. **Capability-driven**
   - Use the capabilities field!
   - Let nodes advertise what they can do
   - Route requests to capable nodes

4. **Observable**
   - Show sync status
   - Show which node data came from
   - Make the mesh debuggable

---

## Next Steps

1. **Decide:** Which option resonates with your vision?
2. **Document:** Update this based on your choice
3. **Rename:** If "mesh" doesn't fit, call it what it is
4. **Prototype:** Build the smallest version that proves the concept
5. **Iterate:** Add complexity only when needed

What do you think? Which direction feels right?
