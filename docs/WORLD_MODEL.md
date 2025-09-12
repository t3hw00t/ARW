# Scoped World Model (Project Map)

Make it a scoped world model: a live, evidence‑backed “project‑world” the agents can consult and update. Done this way, it helps a lot (grounding, safety, planning, reproducibility) without adding a new heavyweight subsystem or research burden.

What “world model” should mean here

- Scope: per‑project, not “the world.” It covers the task environment the agent can actually touch: sources, tools, people/actors, constraints, timelines, budgets.
- Structure: a typed belief graph, not a monolithic neural model. Nodes: entities (files, sites, APIs, stakeholders), claims/facts, tasks/flows, policies, budgets. Edges: supports, contradicts, depends‑on, derived‑from, observed‑at, verified‑by.
- State + dynamics: current beliefs with uncertainty and provenance; simple dynamics like “if X then re‑crawl Y,” “if budget < threshold then prefer local,” “if confidence low then abstain/ask.”
- Predictors (small and local): cheap estimators for latency/cost, retrieval coverage/diversity, risk, and “likelihood this source answers the question.”

Why it helps your intended scope

- Grounding and recall: the chat/context recipe can pull from a stable belief graph instead of re‑discovering the same facts every turn.
- Safer actions: risky tool calls are evaluated against explicit beliefs, gaps, and policies; abstention/escalation becomes principled.
- Better planning: flows can reason over a state of the world (what’s known/unknown, who/what is available, what’s blocked) rather than just text.
- Reproducibility: every artifact and decision points back to beliefs, sources, and policies in force at the time.
- Performance/offloading: the scheduler can choose local vs. remote based on predicted latency/cost vs. SLOs and data‑egress rules.
- UX clarity: users can see the current picture (facts, open questions, contradictions, staleness) and steer quickly.

How to realize it with your current architecture (no new core monolith)

- Treat it as a read‑model: `/state/world` per project that is built from the same event stream and object graph you already standardized (episodes, observations, actions, artifacts, policies).
- Use events you already have: `Beliefs.Updated`, `Feedback.Suggested`, `Projects.FileWritten`, `Actions.HintApplied`, `Runtime.Health`, `Models.DownloadProgress`. The world view is just these events materialized into a belief graph with provenance and confidence.
- Feed the context recipe: let recipes pull top‑K beliefs (by relevance, freshness, and diversity) the same way they pull files or vector hits; show a trace of why each belief is included.
- Show it in the UI: a compact “Project Map” panel (entities, key claims with confidence, open questions, contradictions, stale items). Training Park can surface its health: coverage, staleness, contradiction rate, retrieval diversity.
- Keep it bounded: only ingest what the project touches; purge or summarize over time (retention and dedup rules already fit your Memory Workbench).

What not to do

- Don’t aim for a universal, learned world model. Overkill and unmaintainable.
- Don’t hide it. A world model that isn’t visible and explainable won’t build trust or reduce workload.
- Don’t let it silently widen permissions; any “world update” that implies new access must go through your consent ledger.

Optional upgrades when you have time

- Domain “mini‑worlds” as Logic Units: e.g., a web‑automation world (DOM/task state), a repo‑maintenance world (issues/PRs/deps), a research world (claims/evidence/contradictions). Each ships as a config‑first pack with its own small metrics.
- Simple simulation: “what if I change policy X / swap model Y / lift budget Z?” Run shadow episodes against the belief graph to estimate impact before you flip a switch.

Bottom line

Calling it a scoped world model is accurate and useful: it’s a live, explainable belief graph plus a few lightweight predictors, scoped to each project. It plugs into your existing object graph and event stream, strengthens context assembly, planning, safety, and remote‑offload choices, and stays maintainable by one person.

—

Implementation notes in ARW

- Endpoint: `GET /state/world[?proj=name]` returns a compact Project Map view.
- Selector: `GET /state/world/select?q=...&k=8[&proj=name]` returns top‑K beliefs (claims) with a simple trace (hits, confidence, recency).
- Context assembly: `/admin/context/assemble?q=...&k=8[&proj=name]` returns `{ beliefs, recent, policy, model, project }` where `recent` includes intents/actions/files (size‑bounded) suitable for context recipes.
- Persistence: JSON snapshot at `state/world/world.json` with versioned copies under `state/world/versions/`.
- Sources: materialized from existing events; no new event kinds are required.
- Extensibility: the graph is typed; Logic Units can add domain‑specific nodes/edges without changing the core service.
