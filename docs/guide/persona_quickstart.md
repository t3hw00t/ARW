---
title: Persona Preview Quickstart
---

# Persona Preview Quickstart
Updated: 2025-10-24
Type: Tutorial  
Status: Preview

The persona stack is **preview-only** and ships disabled by default. Follow this guide when you want to experiment with empathetic personas on a local hub. Expect rough edges: the API may change, migrations are manual, and Launcher surfaces hide persona panels until the flag is flipped.

## Before You Start
- Running `arw-server` from this repository (0.2.0-dev or later).
- Admin token available via `ARW_ADMIN_TOKEN` (or basic auth; adjust the examples accordingly).
- SQLite tooling (`sqlite3`) on your PATH for a one-time seed of the initial persona record.
- Optional: `jq` for parsing JSON responses.

> Preview safety: keep personas in test workspaces until the empathy research sprint lands. Production workspaces should stay on the default (disabled) posture.

## 1. Enable the Preview Flag
Set the feature flag on the server process and restart.

```bash
# Linux / macOS shell
export ARW_PERSONA_ENABLE=1
scripts/dev.sh start
```

```powershell
# PowerShell
$Env:ARW_PERSONA_ENABLE = '1'
scripts\dev.ps1 start
```

Prefer configuration files? Add the flag under `[env]` in your active config:

```toml
[env]
ARW_PERSONA_ENABLE = "1"
```

Restart the server after changing either environment or config so the kernel loads the persona service.

## 2. Discover Your Workspace ID
Persona records live inside the workspace scope. Capture the workspace identifier for the seeding step:

```bash
WORKSPACE_ID=$(curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/identity | jq -r '.workspace.id')
echo "Workspace: $WORKSPACE_ID"
```

Keep the value handy for the seeding step (for example, `local-hub`).
No `jq`? Inspect the raw JSON and copy the `workspace.id` field manually.

## 3. Seed an Initial Persona Entry
Use the CLI helper to create (or update) a persona entry. The command automatically resolves the workspace id via `/state/identity` when `--owner-ref` is not provided.

```bash
arw-cli admin persona seed \
  --id persona.alpha \
  --name Companion \
  --archetype ally \
  --traits '{"tone":"warm","style":"supportive"}' \
  --worldview '{"mission":"Assist local projects with empathy"}' \
  --preferences '{
    "context": {
      "lane_weights": {"episodic": 0.3, "procedural": 0.1},
      "slot_budgets": {"evidence": 2}
    }
  }' \
  --base http://127.0.0.1:8091
```

- `--preferences`, `--vibe-profile`, and `--calibration` accept inline JSON or `@path/to/file.json`.
- Add `--enable-telemetry` (and optionally `--telemetry-scope workspace`) when you want vibe feedback enabled immediately.
- `--state-dir` lets you target an offline workspace or a non-default state path.
- Prefer a wrapper? `just persona-seed id=persona.alpha name=Companion telemetry=true scope=workspace` runs the same preview helper via the Justfile.

> Fallback: if the CLI is unavailable, you can seed manually with `sqlite3` using the SQL snippet in the repository history.

Verify that the entry exists:

```bash
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/persona/persona.alpha | jq
```

## 4. Propose and Approve Persona Traits
Persona edits flow through proposals. Submit a diff and then approve it with a `persona:manage` lease (granted automatically when `allow_all` is true; otherwise use `arw-cli admin persona grant` first).

```bash
PROPOSAL=$(curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "submitted_by": "operator",
    "diff": {
      "name": "Companion",
      "traits": { "tone": "warm", "domain": ["research","writing"] },
      "worldview": { "values": ["curiosity","care"] },
      "preferences": {
        "telemetry": { "vibe": { "enabled": false, "scope": "workspace" } },
        "cite_sources": true
      }
    },
    "rationale": "Seed baseline traits for the preview persona"
  }' \
  http://127.0.0.1:8091/admin/persona/persona.alpha/proposals \
  | jq -r '.proposal_id')

curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{ "applied_by": "operator" }' \
  http://127.0.0.1:8091/admin/persona/proposals/"$PROPOSAL"/approve | jq
```

Check the read-model again to confirm the applied diff.

## 5. (Optional) Enable Vibe Telemetry
Keep vibe feedback opt-in. If you skipped `--enable-telemetry` during seeding, toggle it later with the admin API:

```bash
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{ "enabled": true, "scope": "workspace" }' \
  http://127.0.0.1:8091/admin/persona/persona.alpha/telemetry | jq
```

Without telemetry consent, `/persona/{id}/feedback` returns `412 Precondition Required`, protecting the persona from silent feedback ingestion.

## 6. Use the Persona in Workflows
- Set `ARW_PERSONA_ID=persona.alpha` (or pass `--persona-id persona.alpha`) before running `arw-cli orchestrator start`, smoke tests, or Launcher preview panels to tag jobs with the persona.
- Observe persona insights via `/state/persona/persona.alpha`, `/state/persona/persona.alpha/history`, and the Launcher Persona card (Preview) once the UI is enabled. The persona detail view now surfaces `context_bias_preview` (lane/slot weights + min score delta) and `vibe_metrics_preview` (aggregated counts and average signal strengths) so you can audit how telemetry is steering retrieval.
- With telemetry enabled, vibe feedback now compounds across runs: the context loop blends stored `lane_weights` with the aggregated signal averages (`signal_counts` + `signal_strength`) so repeated nudges steadily tilt retrieval and slot budgets until new feedback arrives.

## Preview Verification Checklist
Run these optional sanity checks after seeding:

```bash
# Confirm the helper works end-to-end (no output means success)
just persona-seed id=persona.alpha telemetry=true scope=workspace json=true pretty=true > persona-preview.json

# Inspect the read-model snapshot
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/persona/persona.alpha | jq '.id,.preferences.telemetry.vibe'
```

Both commands should report the preview persona id and show `enabled: true` with the configured scope when telemetry is on.

## Launcher Spot Check

- Start the launcher (`just launcher` or your platform shortcut) with `ARW_PERSONA_ENABLE=1` and an admin token configured.
- Open **Persona → Telemetry**, pick the seeded persona, and confirm the **Context bias preview** lists lane priorities, slot overrides, and min-score deltas.
- Verify the **Signal metrics** card shows total feedback, average strength, and per-signal rows with both `count` and `strength/weight` values; these are sourced from `/state/persona/{id}`.
- Use the legend next to each signal to confirm the strongest recent feedback matches what `/metrics` publishes via `arw_persona_signal_strength_avg`; lane and slot charts should mirror `arw_persona_lane_priority` / `arw_persona_slot_override` for the selected persona.

## Preview Caveats
- No migrations: deleting `state/kernel.sqlite` removes personas; keep backups if you iterate.
- Signals decay automatically when telemetry is enabled. Tune `ARW_PERSONA_SIGNAL_HALFLIFE_SECS` (default 86,400s) or set it to `off`/`0` if you prefer no decay.
- APIs may change: expect renames or schema tweaks while the empathy research concludes.
- UI surfacing: Launcher hides persona panels until `ARW_PERSONA_ENABLE` is set and at least one persona exists.

Report feedback in the Persona & Empathy backlog or via the empathy research issue tracker so we can graduate the feature safely.

## See also
- [Empathy Feedback Loop Research Plan](../ai/empathy_feedback_research_plan.md) — preview guardrails, consent copy, and telemetry rollout checklist.

