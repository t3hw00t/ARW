---
title: Connector Catalog
---

# Connector Catalog
Updated: 2025-10-13
Type: Reference

Canonical catalog of connector blueprints supported (or incubating) in ARW. Each entry includes the recommended `provider`, `scopes`, security notes, and links to manifest examples.

## Cloud Services

| Provider | Kind  | Scopes (examples)             | Notes                                                                 | Example Manifest |
|----------|-------|-------------------------------|-----------------------------------------------------------------------|------------------|
| GitHub   | cloud | `cloud:github:repo:rw`        | Personal Access Token (PAT) or GitHub App token; prefer fine-grained scopes. Configure `meta.allowed_hosts` to `["api.github.com"]`. | [snippet](../guide/connectors.md#register-a-github-connector-and-set-a-token) |
| Slack    | cloud | `cloud:slack:bot`             | Bot token with limited channel access. Set `meta.allowed_hosts` to Slack API domains.                        | [examples/connectors/slack.json](https://github.com/t3hw00t/ARW/blob/main/examples/connectors/slack.json) |
| Notion   | cloud | `cloud:notion:workspace:rw`   | Integration token; restrict database/page scope via Notion UI.                                          | [examples/connectors/notion.json](https://github.com/t3hw00t/ARW/blob/main/examples/connectors/notion.json) |
| Google Workspace | cloud | `cloud:google:drive:ro`, `cloud:google:gmail:send` | Requires service account or OAuth; plan to supply connector helper.                                       | _future_ |
| Microsoft Graph | cloud | `cloud:microsoft:graph:mail.send` | Use application registration with least privilege; obey tenant consents.                                   | _future_ |
| SearXNG | service | `cloud:searxng:search`       | Local metasearch proxy. Keeps upstream search traffic behind a single connector.                           | [examples/connectors/searxng.json](https://github.com/t3hw00t/ARW/blob/main/examples/connectors/searxng.json) |

> _pending_: blueprint approved and documented but manifest sample not yet merged.  
> _future_: roadmap item; schemas/workflows still in design. Track progress in `docs/ROADMAP.md`.

## Local / Desktop Apps

| Provider | Kind | Scopes                          | Notes |
|----------|------|---------------------------------|-------|
| VS Code  | local| `io:app:vscode`                 | Ships with `app.vscode.open` tool. Requires trusted workstation and lease grant. |
| Office / Word | local | `io:app:word` (planned)      | Prototype; not yet released. |
| Email compose | local | `io:app:mail.compose` (planned) | Prototype; will merge with accessibility work. |

## Security & Operations

- **Lease alignment:** Each connector scope must map to an ARW capability lease. Missing leases return `connector lease required` and emit a policy decision.
- **Allowed hosts:** Use `meta.allowed_hosts` to pin domain allowlists. This feeds into the egress guard so only approved endpoints are reachable.
- **Token storage:** Secrets live under `${ARW_STATE_DIR}/connectors/<id>.json`. `/state/connectors` redacts `token` and `refresh_token`.
- **Auditing:** Connector registrations emit `connectors.registered`; token updates emit `connectors.token.updated`. Subscribe to these events for inventory tracking.

## Adding a Connector

1. Draft a manifest (`id`, `kind`, `provider`, `scopes`, `meta`).
2. Register via `POST /connectors/register`.
3. Set tokens via `POST /connectors/token`.
4. Grant capability leases (`POST /leases`).
5. Invoke tools or actions using `connector_id`.

Read the [Connectors guide](../guide/connectors.md) for step-by-step instructions, SearXNG setup, and security guidelines.
