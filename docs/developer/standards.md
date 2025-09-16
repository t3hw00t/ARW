---
title: Open Standards & Practices
---

# Open Standards & Practices

Updated: 2025-09-15
Type: Reference

This project leans on open, portable standards to improve interoperability, tooling, and longevity.

Adopted
- OpenAPI + JSON Schema: service interface and artifacts (`spec/openapi.yaml`, generated docs/tests, Spectral lint).
- AsyncAPI: event channels and payloads; SSE envelopes include CloudEvents metadata.
- CloudEvents: SSE events carry `ce.*` metadata (id, time, type, source, datacontenttype).
- SPDX: license identifiers at file+repo levels for compatibility with scanners.
- SemVer + Keep a Changelog: predictable releases, machine-friendly notes.

Design & UI
- W3C Design Tokens: single source tokens in `assets/design/tokens.w3c.json` (and CSS/JSON mirrors). Use tokens across docs and apps.
- Tailwind tokens (optional): generate `assets/design/tailwind.tokens.json` via `just tokens-tailwind` for use in Tailwind config files.
  - Example: see `assets/design/tailwind.example.config.cjs` for merging tokens into `theme.colors`.
- Style Dictionary (optional): build additional outputs with `just tokens-sd`.
  - Emits: CSS variables (`tokens.css`) + dark overrides (`tokens.dark.css`), SCSS (`tokens.scss`), Less (`tokens.less`), JS module (`tokens.mjs`), JSON (`tokens.json`, `colors.json`), Android (`colors.xml`), iOS (Swift class) under `assets/design/generated/`.
  - Requires Node/npm. Safe to skip on systems without Node (script no‑ops).
  - Also emits: `tokens.derived.css` (brand `rgb` and legacy aliases) and `tokens.theme.css` (class‑scoped `.theme-light` and `.theme-dark` variables) for class‑based theming.
- A11y signals: honor `prefers-reduced-motion`; maintain visible focus; avoid color-only signals (use icons/text).
- Dark mode: `prefers-color-scheme` for first-class dark palette.

Recommended (Next)
- Contrast preferences: consider `prefers-contrast` and `forced-colors` media queries for enhanced accessibility in high-contrast environments.
- Tokens pipeline: integrate a generator (e.g., Style Dictionary) to emit platform outputs (CSS, JSON, iOS, Android, Tailwind config) from the W3C tokens.
  - Minimal pipeline included: `scripts/build_tokens.py` (W3C → CSS/JSON) and `scripts/gen_tailwind_tokens.py` (JSON → Tailwind tokens JSON).
- ADRs (Architecture Decision Records): capture significant choices under `docs/adr/` for traceability.
- Supply-chain: expand SBOMs (CycloneDX/SPDX) in release pipelines and verify provenance with Sigstore.
- Accessibility checks: consider adding a lightweight a11y linter pass for static pages (axe CLI) in CI (optional Node toolchain).
  - Repo includes a docs a11y job that builds the site and runs Axe on key pages, uploading JSON reports as artifacts.
- Security headers: ensure CSP is minimally strict for embedded pages; launcher config already scopes `style-src` to `'self'`.

Notes
- Keep standards additive — do not change meanings of existing fields.
- Prefer widely-supported features and progressive enhancement when adopting new CSS capabilities.
