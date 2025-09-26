---
title: Autonomy Lane Charter
---

# Autonomy Lane Charter

Updated: 2025-09-26
Type: Decision record
Status: Draft

## Why

We want to let trusted teams run fully autonomous helpers that share compute, memory, and workflows without breaking the safety promises that make Agent Hub feel calm. The Autonomy Lane defines the “sandbox” rules so product owners, operators, and external partners know exactly what happens when a helper is allowed to move without constant human approvals.

## What “Autonomy Lane” Means

An Autonomy Lane is a named environment inside Agent Hub where:
- A helper can execute a pre-approved recipe end to end without pausing for every approve/deny prompt.
- Every action, message, and outbound request is still logged in real time and can be interrupted instantly.
- Budgets, schedules, and allowed destinations are locked before the run begins and cannot widen themselves.

This lane is opt-in. Teams choose when to enter it and can fall back to Guided mode anytime.

## Guardrails Checklist

1. **Budget contract**
   - Time, token, and spend caps declared up front.
   - Automatic cool-down when 90% of any budget is used.
   - Daily digest sent to the lane owner.

2. **Destination policy**
   - Allowlist of domains, APIs, and local tools.
   - Any new host triggers an interruptible “seeking permission” alert.
   - File system scope limited to the active project.

3. **Operator controls**
   - Big **Pause helper** button in the Trial Control Center.
   - “Rollback last run” action that reverts recent changes or restores snapshots.
   - Phone/email rotation for on-call humans during the trial window.

4. **Transparency overlays**
   - Live ticker showing current objective, latest action, and next planned step.
   - Timeline replay available after the run with annotations for auto/assist/human steps.
   - Metrics tile in the dashboard with heartbeat, spend, approvals bypassed, and overrides.

5. **Audit hooks**
- Capsule guard leases auto-refresh and log any denial.
- All outbound requests and file writes tagged with the lane id.
- Run summary exported to the Trial Dossier archive.
- Runtime supervisor hooks: accelerator/tier claims (`runtime.claim.*` events), voice/vision runtime enablement, and fallback rules are recorded alongside the lane audit trail.

## Rollout Plan

- **Design**: create low-fidelity mockups for the Autonomy Lane status panel, pause/rollback controls, and budget editor.
- **Implementation**: ship the scheduler kill switch, egress firewall presets, and lane-specific telemetry (tasks `trial-autonomy-governor`, `autonomy-rollback-playbook`).
- **Testing**: stage synthetic workloads (e.g., sandbox e-commerce store) and rehearse interrupts twice before inviting real users.
- **Launch**: announce Gate G4 with clear entry criteria, support rotation, and exit review template.

## Open Questions

- Should lane entry require two-person sign-off (owner + operator)?
- Do we surface a “confidence meter” to end users or keep it as an operator-only signal?
- How do we visualize long-running autonomous work without causing alert fatigue?

Please record decisions and updates in this document as the lane matures.
