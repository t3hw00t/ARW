---
title: Cluster Schemas
---

# Cluster Schemas (Planned)

Updated: 2025-09-12

Node Manifest (signed)
- spec/schemas/cluster_node_manifest.json
- Use for attestation/pinning: scheduler targets only nodes whose manifest matches the workspace spec; Home Node verifies signatures against the trust store.

Events (planned)
- `Cluster.ManifestPublished`, `Cluster.ManifestTrusted`, `Cluster.ManifestRejected`

See also: Architecture → Lightweight Mitigations; Clustering; Policy; Developer Security.

