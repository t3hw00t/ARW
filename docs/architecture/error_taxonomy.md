---
title: Error & Event Taxonomy
---

# Error & Event Taxonomy

Canonical event types
- Episode lifecycle; Obs/Bel/Int/Act; Tokens.In/Out; Tool.Invoked/Ran/Error; Policy.Prompt/Allow/Deny; Runtime.Health; Artifact.Created.

Error catalog
- Categories: user, tool, policy, runtime, network, model.
- Stable codes: e.g., `admission_denied`, `hard_exhausted`, `disk_insufficient`, `checksum_mismatch`, `policy_denied`, `timeout`, `rate_limited`.

Problem details
- RFC 7807 with `{ type, title, status, detail, instance, trace_id, code }`.

See also: Events Vocabulary, Metrics & Insights.

