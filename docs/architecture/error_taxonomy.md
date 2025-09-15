---
title: Error & Event Taxonomy
---

# Error & Event Taxonomy
Updated: 2025-09-14
Type: Explanation

Canonical event types
- Episode lifecycle; obs/bel/int/act; tokens.in/out; tool.invoked/ran/error; policy.prompt/allow/deny; runtime.health; artifact.created.

Error catalog
- Categories: user, tool, policy, runtime, network, model.
- Stable codes: e.g., `admission-denied`, `hard-exhausted`, `disk-insufficient`, `checksum-mismatch`, `policy_denied`, `timeout`, `rate_limited`.

Problem details
- RFC 7807 with `{ type, title, status, detail, instance, trace_id, code }`.

See also: Events Vocabulary, Metrics & Insights.
