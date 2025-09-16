---
title: Cluster Schemas
---

# Cluster Schemas

Updated: 2025-09-15
Type: Reference

Status: Planned

Node Manifest (signed)
- spec/schemas/cluster_node_manifest.json
- Use for attestation/pinning: scheduler targets only nodes whose manifest matches the workspace spec; Home Node verifies signatures against the trust store.

Events (planned)
- `cluster.manifest.published`, `cluster.manifest.trusted`, `cluster.manifest.rejected`

See also: Architecture → Lightweight Mitigations; Clustering; Policy; Developer Security.
