# Agents Running Wild (ARW)

Local‑first Rust workspace for building and running personal AI agents. ARW bundles a lightweight service, CLI, and optional desktop launcher so you can experiment without cloud lock‑in.

## Try ARW in 2 Minutes

Windows
```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup.ps1
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

Linux / macOS
```bash
bash scripts/setup.sh
bash scripts/start.sh --wait-health
```

Open http://127.0.0.1:8090 and visit `/debug` (set `ARW_DEBUG=1` for local dev). The Debug UI includes an Episodes panel (stitched by `corr_id`) and live state snapshots; server read‑models are exposed under `/state/*` (observations, beliefs, intents, actions, episodes).

Docker (amd64/arm64)
```bash
docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
```

## What’s Inside

- Service: user‑mode HTTP with debug UI and SSE events
- Tools: macro‑driven registration with generated JSON Schemas
- Observability: tracing/logging/metrics and event journal (optional)
- Packaging: portable installs and per‑user state by default

## Next Steps

- Quickstart guide: `docs/guide/quickstart.md`
- Desktop Launcher: `docs/guide/launcher.md`
- Admin Endpoints: `docs/guide/admin_endpoints.md`
- Security Hardening: `docs/guide/security_hardening.md`
- Roadmap: `docs/ROADMAP.md`

## Developers

- Enter Nix dev shell: `nix develop`
- Fast loop: `just dev` (runs `arw-svc` with `ARW_DEBUG=1`)
- Docs locally: `just docs-serve` → http://127.0.0.1:8000
- More: `docs/developer/index.md`

## Containers

- Run latest image:
  ```bash
  docker run --rm -p 8090:8090 ghcr.io/t3hw00t/arw-svc:latest
  ```
- Compose/Helm examples: see `docs/DEPLOYMENT.md`

## Contributing

See `CONTRIBUTING.md`. Please open issues/PRs and discussions on GitHub.

—

ARW is dual‑licensed under MIT or Apache‑2.0.
