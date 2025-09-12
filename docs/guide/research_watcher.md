---
title: Research Watcher
---

# Research Watcher

A small, read‑only watcher that surfaces candidate Logic Units from frontier research as config‑packages you can A/B on demand.

Sources
- arXiv feeds; OpenReview/ACL/ICLR/NeurIPS proceedings; Papers‑with‑Code; curated labs/blogs.

Pipeline
- Ingest → categorize by slot → extract pattern summaries (title, gist, expected effect, compute needs) → draft a config‑only Logic Unit manifest with explainer + eval plan.

Curation
- Human review in the Library: approve → becomes Experimental; reject → archived with reasons.

Safety
- Never installs code automatically. Only config‑only drafts; code plugins require signing/sandboxing and explicit review.

MVP
- Start as a simple RSS/JSON fetcher; write suggestions to a local queue/read‑model; render under Library → Suggested.

See also: Logic Units, Evaluation Harness.

