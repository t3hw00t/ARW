---
title: Web Browsing
---

# Web Browsing
Updated: 2025-10-12
Type: How‑to

Give assistants and operators a turnkey way to browse the web while keeping ARW’s egress controls in place. This guide covers the leases to grant, the built-in `http.fetch` tool, and a starter recipe that ships ready to install.

## 1. Grant a temporary net.http lease
- Browsing requires `net:http` (or the broader `io:egress`) capability. Issue a 15‑minute lease:
  ```bash
  curl -s -X POST http://127.0.0.1:8091/leases \
    -H 'content-type: application/json' \
    -d '{"capability":"net:http","ttl_secs":900}' | jq
  ```
- Prefer short TTLs for day-to-day agents; combine with posture presets or capsules for longer-lived automations.

## 2. Fetch a page from the CLI
- Use `arw-cli http fetch` to submit a `net.http.get` action and print a decoded preview:
  ```bash
  arw-cli http fetch https://example.com \
    --wait-timeout-secs 45 \
    --header 'User-Agent: ARW-Browser/0.1'
  ```
- POST support: `--method post --data '{"q":"llama"}' --content-type application/json`
- Capture the head locally: `--output tmp/page.html` writes the preview bytes (full body when small). Add `--raw-preview` to keep the base64 instead of auto-decoding UTF-8. Use `--preview-kb 256` (1–1024 KB) to request a larger streamed head when you need more context for summarization.
- Requests automatically include `X-ARW-Corr` (action id) and respect egress logging, DNS guard, IP-literal blocking, and connector allowlists.

## 3. Install the Web Browsing recipe
- Drop `examples/recipes/web-browsing.yaml` into `${ARW_STATE_DIR}/recipes/web-browsing/manifest.yaml` or install via CLI:
  ```bash
  arw-cli recipes install examples/recipes/web-browsing.yaml
  ```
- The manifest:
  - grants a 15-minute `net.http` lease
  - wires `http.fetch` for retrieval
  - follows up with `summarize_text` so the agent returns concise summaries with source citations
- Tailor the prompts or add guardrails before sharing with higher-trust agents.
- Optional: register the [SearXNG connector](connectors.md#optional-searxng-metasearch-connector) and set `connector_id: search-searxng` in the recipe when you want browsing runs to route through the local metasearch aggregator.

## 4. Monitor activity
- `arw-cli state actions --kind net.http.get --json --pretty` lists browsing runs.
- The egress ledger (`/state/egress/ledger`) records allow/deny decisions, destinations, and byte counts for auditing.
- Apply capsules or the forthcoming egress firewall scopes to constrain domains (`net:http:api.github.com`, etc.).

## Next steps
- Combine with the Research recipe to fetch sources before clustering.
- Wire the same tool into Logic Units so planner agents can schedule HTTP fetches under review.
- Keep `ARW_HTTP_BODY_HEAD_KB` tuned (default 64 KB) if you routinely consume larger previews.
