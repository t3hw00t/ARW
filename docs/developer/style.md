---
title: Style & Harmony
---

# Style & Harmony

We aim for a calm, precise experience. Keep visuals understated; let high-impact interactions shine.

Updated: 2025-09-14
Type: Reference

Guidelines
- Clarity first: short sentences, mild technical terms.
- Clean lines: avoid visual noise, favor whitespace.
- Gentle emphasis: use callouts and bold sparingly.
- Predictable rhythm: consistent headings, short sections, stable nav.

## Documentation Tone
- User docs are friendly and practical.
- Developer docs are precise, with code pointers and commands.
- Both avoid jargon unless it adds real value.

## Tact & Semantics

- Language: US English (American). Examples: canceled, color, disk.
- Tone: calm and helpful; avoid blame. Offer a next step or link.
- Errors: one‑line summary + cause + suggestion. Prefer “couldn’t connect (timeout)” over vague “failed”.
- Events: `status` is human‑friendly (started, downloading, degraded, complete, canceled). `code` is a stable machine hint (e.g., `admission-denied`, `hard-exhausted`, `disk-insufficient`, `canceled-by-user`). Add codes additively; don’t change existing meanings.
- Consistency: reuse nouns/verbs across UI, events, and APIs (download, resume, cancel).
- Brevity: keep messages short; include context only when it changes a decision.

## Code Style (High Level)
- Prefer explicit names over cleverness.
- Keep modules small and responsibilities clear.
- Instrument with tracing at boundaries and errors.

### Egress Ledger Helper (Builder)

When appending to the egress ledger from Rust code, prefer the typed helper and builder:

```rust
// Build and append an "allow" entry (models.download completion)
let entry = EgressLedgerEntry::allow("models.download")
    .dest(dest_host.clone(), dest_port, dest_proto.clone())
    .corr_id(corr_id.clone())
    .bytes_in(bytes)
    .duration_ms(elapsed_ms)
    .build();
Self::append_egress_ledger(&bus, entry).await;

// Build and append a "deny" entry (request-failed)
let entry = EgressLedgerEntry::deny("request-failed")
    .dest(dest_host.clone(), dest_port, dest_proto.clone())
    .corr_id(corr_id.clone())
    .duration_ms(elapsed())
    .extra(serde_json::json!({"error": e.to_string()}))
    .build();
Self::append_egress_ledger(&bus, entry).await;
```

Benefits
- Consistent shape across allow/deny cases (decision, reason_code, dest, corr_id, bytes/duration).
- Less duplication and fewer mistakes vs ad‑hoc JSON assembly.
- Extensible: add fields in one place without touching call sites.

## Documentation Conventions

- Title and H1: front‑matter `title:` and a single `#` H1 matching it.
- Updated line: add `Updated: YYYY-MM-DD` under the H1 when meaningful.
- Headings: Title Case for H2/H3.
- Bullets: sentence case; keep style consistent within a list; punctuation optional.
- Cross‑links: add a short “See also:” block near the top for adjacent pages.
- Tabs: use OS tabs for commands (`pymdownx.tabbed`) with labels “Windows” and “Linux / macOS”.
- Admonitions: use `!!! warning` for security/foot‑guns; `!!! note`/`tip` for guidance.
- Commands/paths: fenced blocks with language hints; wrap env vars and identifiers in backticks.
- Links: relative paths within `docs/` so MkDocs resolves them; avoid external URLs when an internal page exists.
- Avoid duplication: link to canonical pages (Quickstart, Deployment, Configuration, Admin Endpoints).
- Security: surface a “Minimum Secure Setup” box on Quickstart/Admin pages.
- Terms: prefer glossary terms; add new ones to `docs/GLOSSARY.md`.
- Formatting: keep sections short; prefer 80–100 char lines but don’t force awkward wraps.

## API Style (OpenAPI/AsyncAPI)

- OperationId: snake_case ending with `_doc` (e.g., `state_world_get_doc`). Enforced by Spectral in CI and pre‑push; code‑generated OpenAPI is linted too.
- Tags: every operation has a non‑empty `tags` array. Prefer existing groups (Public, Public/Specs, Admin/*).
- Descriptions: brief, one‑line imperative description for each operation. The pre‑commit hook can auto‑fill placeholders.
- Deprecations: set `deprecated: true` and include `x-sunset: 'YYYY-MM-DDTHH:MM:SSZ'`. Runtime emits `Deprecation`, optional `Sunset`, and `Link: rel="deprecation"`.
- Events: SSE envelopes include CloudEvents metadata under `ce` (`specversion`, `type`, `source`, `id`, `time`, `datacontenttype`). Document channels in AsyncAPI.
- Consistency: ProblemDetails for 4xx/5xx; keep response shapes and pagination/query params consistent across similar endpoints.

### Helpers & Scripts

- Normalize/auto‑fill: `python3 scripts/ensure_openapi_descriptions.py` (also runs in pre‑commit & CI)
- New endpoint scaffold:
  - Preview: `just endpoint-new METHOD /path tag="Admin/Core"`
  - Apply to spec: `just endpoint-add METHOD /path tag="Admin/Core" summary="..." desc="..."`
- Release notes: `just docs-release-notes base=origin/main`
- Interfaces local checks: `just interfaces-index`, `just interfaces-lint`, `just interfaces-diff`

### Pre‑commit & Pre‑push

- Pre‑commit: fmt/clippy/tests; generates interface index + deprecations and ensures descriptions/tags; lints specs when staged.
- Pre‑push: OpenAPI sync check (codegen vs spec); Spectral lint on spec + generated OpenAPI; AsyncAPI diff (best‑effort); generates interface release notes (warn on drift).

## UI Conventions (Apps & Debug)

- Tokens: use spacing vars (`--sp2/3/4/5`) and shared radii; avoid one‑off paddings.
- Buttons: `.primary` for primary CTAs; `.ghost` for low‑emphasis actions; default for normal actions.
- Collapsibles: dashboard sections can collapse; persist state per heading; add “Expand/Collapse all” if there are many panels.
- Density: optional “Density” toggle; compact reduces gaps and radii and is device‑friendly.
- Iconography: simple states → single icon; complex states → subtle icon set (e.g., `hdd+warn`).
- Status tones: ok (green), warn (amber), bad (red), accent (teal), info (muted).
- Dark mode: honor `prefers-color-scheme`; keep shadows light and overlays subtle.

## Docs Conventions (Visual)

- Topic markers: use `.topic-trio` with strengths and mirrored `data-*` labels to convey scope.
- Admonitions: prefer `tip`/`note` for guidance; `warning` for security/irreversible actions.
- Screenshots/figures: strive for same density and spacing rhythm as the app; avoid artificial zoom.
---
title: Developer Style Guide
---

# Developer Style Guide

This guide aligns code and UI conventions across ARW for a clean, fast, and consistent experience. Prefer clarity and performance over cleverness; keep the interactive performance ethos visible in both UX and code.

## UX & UI
- Layout: two‑column by default (content + right‑sidecar). The sidecar is sticky and collapsible per lane; avoid nested scroll regions when possible.
- Density: default to compact but readable. Use 12–14px for mono blocks, 14–16px for body text depending on view.
- Rhythm: use the shared spacing scale (`--sp2/3/4/5`) and avoid ad‑hoc margins/paddings.
- Color: use design tokens (`--color-ink`, `--color-muted`, `--color-line`, `--surface`, `--surface-muted`, status/brand tokens). Avoid hardcoded hex; prefer tokens.
- Motion: default to subtle. Respect `prefers-reduced-motion: reduce` — no critical info conveyed by animation.
- Components: reuse patterns — cards (bordered, soft gradient), pills (inline status), badges (count/ok/warn). Avoid bespoke one‑offs.
- Readability: long text uses `white-space: pre-wrap`; code/JSON uses `pre` with small mono font and optional pretty toggle.
- Accessibility: ensure focus outlines on interactive elements; use semantic headings and labels; keyboard path for all actions (palette, buttons).

## Events & Sidecar
- One SSE connection per window. Subscribe with `prefix` filters where possible; no parallel streams.
- Read‑models publish RFC‑6902 patch deltas (`state.read.model.patch`). Apply locally and render snapshots incrementally.
- Sidecar lanes (default order): Timeline, Context, Policy, Metrics, Models, Activity. Keep each lane scannable, avoid walls of text.
- Metrics: display route P95 with a small sparkline; color P95 green when under the current SLO. Keep numeric noise minimal.
- Policy: show leases as compact “pills” with scope, TTL, and principal; defer actions to explicit prompts.

## HTML/CSS
- Structure: minimal, semantic HTML; prefer utility classes defined in `common.css`.
- Dark mode: scope via `@media (prefers-color-scheme: dark)` with the same variable keys.
- Don’t inline styles beyond small, dynamic overrides (e.g., `white-space` toggles).
- Avoid layout shifts: give fixed heights or max‑heights to dynamic panels (logs, diff outputs).

## JavaScript
- Keep pages small and self‑contained. Put shared logic in `common.js` (SSE, read‑models, sidecar, palette, toasts, templates).
- Avoid frameworks for launcher pages (Web Components or React are overkill here). Vanilla + small helpers.
- SSE: one `EventSource` per page; replace on options change; handle `open`/`error` and expose a simple subscribe API.
- JSON patches: apply with a tight function; do not pull heavy diff libs into the launcher UI.
- Command palette: Ctrl/Cmd‑K. Include essential actions (open windows, refresh, toggle focus, SSE replay).
- Persistence: use `get_prefs/set_prefs` via Tauri; keep keys namespaced (e.g., `ui:hub`).
- Error handling: swallow non‑critical errors in UI (network blips); surface critical ones as concise toasts.

## Rust (Service)
- Endpoints: prefer public `/state/*` for UI read‑models; keep admin routes under `/admin/*` and document tokens when necessary.
- Events: publish dot.case kinds only. Avoid legacy CamelCase.
- Read‑models: publish via `read_model::emit_patch(bus, TOPIC_READMODEL_PATCH, id, &value)`; keep IDs stable and small.
- Performance: keep per‑route timing and P95 calculation cheap; emit deltas at a reasonable frequency (coalesce bursts).
- Security: gate actions (`/actions/*` when introduced) and ingress/egress per the policy module; never rely on UI for enforcement.

## Copy & Tone
- Calm, factual, and actionable. Prefer brevity in labels and helper text; avoid jargon.
- Buttons: verbs (“Refresh models”, “Run A/B”), not nouns.
- Tooltips: short, specific hints; no marketing.

## Interactive Performance Principles (UX visible)
- Show SLO targets and current performance (e.g., green P95).
- Avoid long loading states; render incrementally.
- Offer “Only changes” toggles for noisy views (diffs, logs).

## Review Checklist
- Fonts/sizes match shared tokens; spacing consistent.
- One SSE per page; filters configured; pause/clear/copy controls where relevant.
- Keyboard access: palette active; focus outlines visible.
- Dark mode legible; contrast good in both modes.
- No console errors; no obvious layout shifts.
