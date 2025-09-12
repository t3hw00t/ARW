---
title: Budgets & Context Economy
---

# Budgets & Context Economy

Motivation
- Token/time/compute are scarce; retrieval can drown generation.

Budgets
- Per‑agent and per‑project budgets with hard caps and soft targets.
- Each execution carries a spend plan: tokens in/out, latency targets, and optional $ estimates for remote APIs.

Events
- `Budget.Planned`, `Budget.Spent`, `Budget.Degraded` (soft exhausted), `Budget.HardExhausted`.

Context recipes
- Cap tokens per pipe; track actual contribution vs budget.
- A/B comparison helps tune recall vs precision.

See also: Context Working Set, Context Recipes, Cost & Quotas, Runtime Matrix.
