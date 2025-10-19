---
title: Automation Ops Handbook
---

# Automation Ops Handbook

Updated: 2025-10-19
Type: Handbook

This handbook equips operators to run autonomous or semi-autonomous revenue agents safely. It assumes you already run the unified server (`arw-server`), opt into federation packs, and guard access with leases and policy scopes. Use it alongside the technical runbooks under `docs/ops/`.

## Readiness Checklist
- [ ] Enable `ARW_PERSONA_ENABLE=1` only after personas and empathy telemetry are reviewed with stakeholders and consent notices are in place.
- [ ] Confirm policy baselines: `policy.json` (or Cedar bundle) grants only explicit leases (`persona:manage`, `telemetry:persona:*`, `runtime:manage`, `egress:proxy`).
- [ ] Provision a dedicated automation workspace with scoped credentials and budget caps (`ARW_AUTONOMY_MAX_RUNTIME`, spend/time budgets).
- [ ] Validate guardrails: run `just smoke-safe` followed by `just runtime-smoke` with `RUNTIME_SMOKE_ALLOW_CPU=1` (and GPU if applicable) before unattended execution.
- [ ] Stage automation recipes in a non-production state dir; verify signatures with `arw-cli recipes install --verify` and dry-run using the preview execution mode.
- [ ] Confirm logging/telemetry destinations (SSE subscribers, syslog, SIEM) capture `persona.*`, `cluster.*`, `automation.*`, and egress ledger events.
- [ ] Ensure operators and on-call rotation have access to escalation channels (chat, paging, or phone tree) with documented response targets.

## Operational Guardrails
- **Consent gates**: automation must not ingest user data unless consent bundles (`configs/runtime/bundles*.json`) list matching `metadata.consent` scopes; re-run `scripts/validate_runtime_consent.py` after edits.
- **Lease expirations**: keep persona/telemetry leases under 24 hours. Use the CLI `arw-cli admin persona grant` helper to renew with justification.
- **Runtime isolation**: pin automations to the eco preset unless workloads justify higher tiers. `ARW_PERF_PRESET=eco` throttles concurrency, reduces cache pressure, and keeps CPU-only laptops within safe thermals.
- **Egress policy**: enable the Guardrail Gateway (`ARW_EGRESS_PROXY_ENABLE=1`, `ARW_EGRESS_LEDGER_ENABLE=1`) to inspect outbound automation traffic and capture audit trails.
- **Kill switch**: maintain a pre-approved `automation_shutdown` action (policy or runbook) that revokes leases, stops runtimes, and halts scheduled jobs.

## Alerting Defaults
- **Heartbeat gaps**: alert if automation personas stop producing `persona.feedback` or `automation.run.*` events for more than 2x the expected cadence.
- **Policy denials**: page when policy simulator logs repeated `deny` results for automation actions (likely signal of drift or missing leases).
- **Budget breaches**: warn at 70% of token/runtime budgets, escalate at 90%, hard-stop at 100%.
- **Egress anomalies**: trigger alerts on new domains or protocol changes in the egress ledger; create allowlists per automation persona to avoid alert fatigue.
- **Runtime degradation**: watch `runtime.supervisor` metrics (restart count, health reasons). Automations should idle gracefully; repeated restarts indicate runaway workloads.

## Escalation Flow
1. **Triage** (0-5 min): on-call reviews the alert context, correlates recent persona proposals, and checks `/state/automation/status` (or runbook dashboards) for impacted jobs.
2. **Mitigate** (5-15 min): apply the kill switch if harm is ongoing, revoke automation leases, and pause affected recipes via `arw-cli automation disable <id>`.
3. **Communicate** (<=15 min): notify stakeholders on the agreed channel; document user impact and mitigation steps.
4. **Investigate** (within 2 hours): gather persona history (`GET /state/persona/{id}/history`), recipe manifests, and ledger exports. File an incident report in the shared tracker.
5. **Remediate** (within 1 business day): patch faulty recipes or persona traits, add policy tests, and update guardrails. Schedule a follow-up review if legal/compliance obligations were affected.

## Compliance Notes
- **Data handling**: map every automation input/output to consent scopes; anonymize exports by default and retain raw telemetry only when policy explicitly permits.
- **Jurisdiction**: confirm local labor and privacy regulations before monetizing autonomous agents (GDPR, CCPA, and local employment laws may treat automations as subcontracted work).
- **Audit trail**: retain persona proposal approvals, automation ledger exports, and consent acknowledgements for at least 90 days (or local statutory minimum).
- **Third-party services**: verify Terms of Service compatibility for external APIs (no prohibited automated scraping, respect rate limits, include attribution when required).
- **Revenue reporting**: coordinate with accounting/finance teams; export contribution ledger CSVs and reconcile payouts each cycle.

## Example Runbooks
- [Runtime Bundle Runbook](runtime_bundle_runbook.md) - ensuring managed runtimes stay patched and signed before automations execute GPU workloads.
- [Cluster Runbook](cluster_runbook.md) - onboarding remote workers/co-drivers that support revenue automations.
- [Monitoring Playbook](monitoring.md) - wiring Prometheus/SSE collectors and alert routing for automation metrics.
- [Trial Readiness Checklist](trial_readiness.md) - adapting trial facilitation steps for automation pilots.
- [Systemd Overrides](systemd_overrides.md) - configuring watchdog timers, restart throttles, and sandboxing for unattended services.

## After-Action Checklist
- [ ] Incident logged with timestamps, impact summary, and remediation owner.
- [ ] Persona proposals updated (diff applied and history annotated).
- [ ] Recipes patched, signed, and redistributed; retired versions archived.
- [ ] Policy/lease rules adjusted and validated via simulator tests.
- [ ] Documentation and operator training refreshed to cover new safeguards.

## References
- Persona empathy telemetry RFC (`docs/architecture/persona_self_model_rfc.md`)
- Revenue recipe backlog (`docs/BACKLOG.md`)
- Consent validation script (`scripts/validate_runtime_consent.py`)
- Guardrail Gateway architecture (`docs/architecture/egress_firewall.md`)
