---
title: HTTP Caching Semantics (Digest Blobs)
---

# HTTP Caching Semantics (Digest Blobs)
Updated: 2025-09-15
Type: Reference

For endpoints that serve immutable, digest‑addressed blobs (e.g., `/admin/models/by-hash/{sha256}`):

- Validators: strong ETag and Last‑Modified
  - `ETag: "{sha256}"` (quoted, strong)
  - `Last-Modified: <RFC 2822>`
  - `Accept-Ranges: bytes`
- Cache policy: long‑lived and immutable
  - `Cache-Control: public, max-age=31536000, immutable`
- Conditional requests
  - `If-None-Match` → `304 Not Modified` when it matches the ETag
  - `If-Modified-Since` → `304 Not Modified` when not newer than Last‑Modified
  - Precedence: ETag checks (If‑None‑Match) take precedence over Last‑Modified
- Partial content
  - `Range: bytes=start-end | start- | -suffix`
  - Returns `206 Partial Content` with `Content-Range: bytes start-end/total`
  - Invalid/unsatisfiable ranges → `416 Range Not Satisfiable` with `Content-Range: bytes */total`
- HEAD behavior
  - Mirrors GET headers (validators, caching); no body
  - Honors `If-None-Match` / `If-Modified-Since`

Examples:

```bash
# HEAD with ETag validator; 304 if unchanged
curl -I \
  -H 'If-None-Match: "<sha256>"' \
  http://localhost:8090/admin/models/by-hash/<sha256>

# GET a slice (first 1KB)
curl -H 'Range: bytes=0-1023' \
  http://localhost:8090/admin/models/by-hash/<sha256>
```
