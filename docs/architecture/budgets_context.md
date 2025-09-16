---
title: Budgets & Context Economy
---

# Budgets & Context Economy
Updated: 2025-09-15
Type: Explanation

Motivation
- Token/time/compute are scarce; retrieval can drown generation.

Budgets
- Per‑agent and per‑project budgets with hard caps and soft targets.
- Each execution carries a spend plan: tokens in/out, latency targets, and optional $ estimates for remote APIs.

Events
- `budget.planned`, `budget.spent`, `budget.degraded` (soft exhausted), `budget.hard_exhausted`.

Context recipes
- Cap tokens per pipe; track actual contribution vs budget.
- A/B comparison helps tune recall vs precision.

See also: Context Working Set, Context Recipes, Cost & Quotas, Runtime Matrix.
