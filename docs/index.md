---
title: Agent Hub (ARW)
---

# Agent Hub (ARW)

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while you stay in charge. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

Updated: 2025-09-16
Type: Explanation

## Why It’s Different
- You decide access: files, web, mic, and camera are off until you grant time‑limited permission.
- You can see and replay everything: sources, steps, tools used, and cost; snapshot any run and compare later.
- It grows with you: start on one laptop; invite other machines to help or co‑drive in real time.
- Configurable, not brittle: “Logic Units” are safe strategy packs you can A/B test, apply, and roll back.

## What You Can Do
- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run focused research sprints with citations and comparisons.
- Watch sites or docs for changes and get actionable updates.
- Turn vague goals into concrete plans and next steps.
- Chat to explore data and export both answers and evidence.

## Scaling & Sharing (Opt‑In)
- Pool compute to your GPU box or a trusted collaborator’s machine; offload heavy jobs under your rules and budget.
- Live co‑drive sessions: others can watch, suggest, or take the wheel with your approval; risky actions wait for your sign‑off.
- Clear boundaries: preview what would leave your machine, to whom, and the estimated cost; an egress ledger records it.
- Fair splits: meter GPU time, tokens, and tasks for transparent revenue sharing later.

## Safety & Control
- Permission leases with timers and scopes; no silent escalation.
- A project world view tracks key facts, open questions, and constraints so agents act on evidence, not guesses.
- Budgets for time, tokens, and spend; stay within plan with a visible meter.
- Signed plugins and sandboxed tools by default.

## Get Started
- Quickstart: run ARW locally in minutes.
- Features: see what’s included and what’s planned.
- Architecture: understand the object graph and single event stream (SSE).
- Verify: check `GET /healthz` and `GET /about` for a quick sanity check.

Tip: ARW treats context as a just‑in‑time working set assembled from layered memories. See Architecture → Context Working Set for how we keep prompts small, relevant, and explainable.

## Who It’s For
- People who want real help on real work without giving away their data.
- Independent builders who prefer practical, local tools that can scale when needed.
- Teams who want transparent collaboration, clear costs, and reproducible results.

## Non‑Goals
- Not a hosted cloud platform; no hidden network egress by default.
- Not a monolithic “one‑true‑agent” — compose via recipes and tools.

## Choose Your Path
- Run Locally: Quickstart
- Operate & Secure: Security Hardening, Deployment, Admin Endpoints
- Contribute: Developer/Overview

Related:
- [Experiments (A/B) & Goldens](guide/experiments_ab.md)
- [Debug UI Overview](guide/debug_ui.md)
- [CLI Guide](guide/cli.md)
- [Spec Endpoints](reference/specs.md)
