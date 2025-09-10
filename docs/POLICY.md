Policy, Gating, and Capsules
Updated: 2025-09-10

Single Source of Truth
- Gate keys: `arw-core::gating_keys` — constants for all actions/streams; used across endpoints for enforcement and docgen.
- Contract model: `arw-core::gating` — deny contracts with conditions (role/node/tags, time windows, auto-renew). Deny-wins and immutable within active window.
- Policy Capsule: `arw-protocol::GatingCapsule` — wire format for propagating denies and contracts.

Ingress/Egress Guards
- Enforced at the start and end of processing:
  - Ingress: `io:ingress:task.<kind>`, `io:ingress:tools.<id>`
  - Egress: `io:egress:task.<kind>`, `io:egress:tools.<id>`, `io:egress:chat`
- Purpose: personality safeguard — prevent disallowed info from entering context and prevent sensitive outputs from leaving.

Capsule Adoption
- Adopt via HTTP header `X-ARW-Gate: <json>` after passing admin rate-limit; header size limited (≤4 KiB).
- Adoption is ephemeral by default (renegotiated on restart).
- Bus/Event: Envelope can carry an optional capsule; service does not auto-adopt from events by default.

Regulatory Provenance Unit (RPU) — Planned
- Verify capsule signatures (ed25519/secp256k1; Sigstore later) against a trust store.
- ABAC (Cedar) for adoption decisions (issuer/role/tags/node, TTL, scope).
- Enforce hop TTL and `propagate` scope; relay only verified capsules.
- Ephemeral adoption ledger (append-only; optional timestamp anchoring).

Next Steps
- Budgets/quotas with persisted counters (optional allow-with-budgets) — deny precedence.
- Macro `#[arw_gate("key")]` to annotate handlers; auto-enforce and docgen.
- Generate `spec/schemas/gating.schema.json` and `docs/GATING_KEYS.md` from code.
