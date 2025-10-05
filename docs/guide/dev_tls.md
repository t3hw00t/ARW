---
title: Dev TLS Profiles
---

# Dev TLS Profiles
Updated: 2025-09-29

Local TLS makes it easier to test secure transports and proxy headers without waiting for a public certificate. The `dev_tls_profile.sh` helper plants mkcert certificates when available and falls back to self-signed assets so every environment can exercise HTTPS.

## Quickstart

1. Install [mkcert](https://github.com/FiloSottile/mkcert) (optional but recommended). On macOS or Linux, install via Homebrew (`brew install mkcert nss`) or your package manager; on Windows use Scoop/Chocolatey.
2. Run the helper:
   - `just tls-dev` — generates certs for `localhost`, `127.0.0.1`, and `::1`.
   - `just tls-dev dev.local 127.0.0.1` — override the host list (first host seeds the output filenames).
   - Alternatively call `scripts/dev_tls_profile.sh` directly when Just is unavailable.
3. Start Caddy with the generated config (use whichever flow fits your environment):
   ```bash
   # Just recipe (runs in the foreground)
   just proxy-caddy-start

   # or call Caddy directly
   caddy run --config configs/reverse_proxy/caddy/Caddyfile.localhost
   ```
4. Launch `arw-server` (for example `just dev`). Keep `ARW_TRUST_FORWARD_HEADERS=1` when you expect proxy headers like `X-Forwarded-For`.

The helper writes assets to `configs/reverse_proxy/caddy/`:
- `*.crt` / `*.key`: certificate and private key (ignored by git).
- `Caddyfile.<host>`: reverse proxy snippet pointing to `127.0.0.1:${ARW_DEV_TLS_BACKEND_PORT:-8091}`.

## mkcert vs self-signed

- When `mkcert` is present, the script installs the local trust store (unless `ARW_DEV_TLS_SKIP_TRUST_INSTALL=1`) and issues certificates for every host argument.
- Without mkcert, it generates a self-signed RSA certificate via `openssl` with Subject Alternative Names for all hostnames/IPs.
- Export `ARW_DEV_TLS_FORCE_OPENSSL=1` to skip mkcert even when installed (useful inside CI).
- Adjust certificate lifetime by setting `ARW_DEV_TLS_SELF_SIGNED_DAYS` (default `825`).

## Hygiene

- Certificates live under `configs/reverse_proxy/`, which ships with a `.gitignore` entry for keys and generated configs. Keep them private.
- Re-run the helper whenever you add hosts or rotate credentials; existing files are overwritten in place.
- For production, follow the hardened guidance in [Reverse Proxy](reverse_proxy.md) or integrate with your infrastructure’s certificate automation.
