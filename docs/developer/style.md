---
title: Style & Harmony
---

# Style & Harmony

We aim for a calm, precise experience. Keep visuals understated; let high-impact interactions shine.

Updated: 2025-09-12

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
- Events: `status` is human‑friendly (started, downloading, degraded, complete, canceled). `code` is a stable machine hint (e.g., `admission_denied`, `hard_exhausted`, `disk_insufficient`, `canceled_by_user`). Add codes additively; don’t change existing meanings.
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

// Build and append a "deny" entry (request_failed)
let entry = EgressLedgerEntry::deny("request_failed")
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
