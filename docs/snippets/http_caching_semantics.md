---
title: HTTP Caching Semantics (Digest Blobs)
---

# HTTP Caching Semantics (Digest Blobs)
Updated: 2025-09-26
Type: Reference

For endpoints that serve immutable, digest‑addressed blobs (e.g., `/admin/models/by-hash/{sha256}`):

- Validators: strong ETag and Last‑Modified
  - `ETag: "{sha256}"` (quoted, strong)
  - `Last-Modified: <RFC 2822>`
  - `Accept-Ranges: bytes`
- Cache policy: long‑lived and immutable
  - `Cache-Control: public, max-age=31536000, immutable`
- Conditional requests
  - `If-None-Match` → `304 Not Modified` when it matches the ETag (response continues to advertise `Accept-Ranges: bytes`)
  - `If-Modified-Since` → `304 Not Modified` when not newer than Last‑Modified
  - Precedence: ETag checks (If‑None‑Match) take precedence over Last‑Modified
- Partial content
  - `Range: bytes=start-end | start- | -suffix`
  - Returns `206 Partial Content` (or `206` + empty body for `HEAD`) with `Content-Range: bytes start-end/total`
  - Invalid/unsatisfiable ranges → `416 Range Not Satisfiable` with `Content-Range: bytes */total`
  - Only single-range requests are supported; multi-range specs fall back to `416`
- HEAD behavior
  - Mirrors GET headers (validators, caching, `Accept-Ranges`); no body
  - Honors `If-None-Match` / `If-Modified-Since`

For generated reference artifacts (e.g., `/spec/openapi.yaml`, `/spec/asyncapi.yaml`, `/spec/mcp-tools.json`, `/spec/schemas/*.json`, `/catalog/index`), the server emits strong ETags derived from file contents plus optional `Last-Modified` timestamps. The cache policy defaults to `Cache-Control: public, max-age=300, must-revalidate`, balancing fresh docs with client revalidation support.

Examples:

```bash
# HEAD with ETag validator; 304 if unchanged
curl -I \
  -H 'If-None-Match: "<sha256>"' \
  http://localhost:8091/admin/models/by-hash/<sha256>

# GET a slice (first 1KB)
curl -H 'Range: bytes=0-1023' \
  http://localhost:8091/admin/models/by-hash/<sha256>
```
