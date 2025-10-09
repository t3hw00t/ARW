---
title: Gating Config
---

# Gating Config
Updated: 2025-10-09
Generated: 2025-10-09 13:32 UTC
Type: Reference

Immutable gating policy boots from `configs/gating.toml` or the `ARW_GATING_FILE` override. It layers with hierarchy defaults, runtime capsules, and leases so denies remain traceable and auditable. Keys support trailing `*` wildcards.

## Load order
- `ARW_GATING_FILE` (absolute or relative) if set
- `configs/gating.toml` discovered via `ARW_CONFIG_DIR`, the executable directory, or the workspace root
- `ARW_GATING_DENY` environment variable (comma-separated)

## Schema
- JSON Schema: [`gating_config.schema.json`](gating_config.schema.json)
- Validate with `jsonschema`, `ajv`, or `arw-cli gate config schema`

## Top-level keys

| Key | Type | Description |
| --- | --- | --- |
| `deny_user` | `array<string>` | Immutable deny-list applied at boot; supports trailing `*`. |
| `contracts` | `array<Contract>` | Conditional denies evaluated on every request; supports filters, TTLs, quotas, and auto-renew. |

## Contract fields

| Field | Type | Description |
| --- | --- | --- |
| `id` | `string` | Unique identifier recorded in audits and renewals. |
| `patterns` | `array<string>` | Gating key patterns (supports trailing `*`). |
| `subject_role` | `string?` | Optional caller role filter (`root`, `regional`, `edge`, `connector`, `observer`). |
| `subject_node` | `string?` | Optional node id filter (`ARW_NODE_ID`). |
| `tags_any` | `array<string>?` | Match when any tag from the caller overlaps. |
| `valid_from_ms` | `integer?` | Epoch milliseconds that activate the contract (inclusive). |
| `valid_to_ms` | `integer?` | Epoch milliseconds that expire the contract (inclusive). |
| `auto_renew_secs` | `integer?` | Seconds to extend the contract after expiry. |
| `immutable` | `bool?` | Defaults to `true`; when `false`, runtime may remove before expiry. |
| `quota_limit` | `integer?` | Maximum invocations allowed within the sliding window. |
| `quota_window_secs` | `integer?` | Sliding window size in seconds paired with `quota_limit`. |

## Field notes
- `valid_from_ms` and `valid_to_ms` use milliseconds since Unix epoch.
- Quotas require both `quota_limit` and `quota_window_secs`.
- `auto_renew_secs` updates the next expiry relative to the evaluation time.

## Examples

### Deny introspection by default
```toml
deny_user = ["introspect:*"]
```

### Nightly freeze for actions and tools
```toml
[[contracts]]
id = "night-freeze"
patterns = ["actions:*", "tools:*"]
valid_from_ms = 1735689600000  # 2024-12-01T00:00:00Z
valid_to_ms = 1735776000000    # 2024-12-02T00:00:00Z
immutable = true
```

### Quota-limited edge tools burst
```toml
[[contracts]]
id = "edge-tools-burst"
patterns = ["tools:run"]
subject_role = "edge"
tags_any = ["lab"]
quota_limit = 5
quota_window_secs = 60
auto_renew_secs = 0
```
