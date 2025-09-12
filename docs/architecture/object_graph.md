---
title: Unified Object Graph
---

# Unified Object Graph

Core idea: treat the system as two things — a shared object graph (entities + relations) and a single event stream. Every surface (Project Hub, Chat, Training Park, Managers) is just a lens on that graph, driven by the same live events (SSE). This drastically reduces drift and keeps the experience coherent.

Global scope (inventories)
- Models, Tools, Policies, Hardware targets, Sandboxes/Containers, Plugins/Extensions

Project scope (bindings and use)
- Projects, Agent Profiles (definitions), Agent Instances (profile + runtime + policy), Data Sources, Memory Mounts (vector/graph/kv/doc indexes), Context Recipes (how to assemble the enhanced context)

Design principle: managers edit inventories; Projects and Agents only reference those inventories. Duplicate data and hidden state are anti‑patterns.

Read‑models and state
- Server exposes `/state/*` read‑models (observations, beliefs, intents, actions, episodes). These are the source of truth for UI and clients.
- Use correlation id (`corr_id`) to stitch events into episodes.

Relationships (examples)
- Project ↔ Files/Notes/Artifacts
- Agent Profile ↔ Tools/Policies/Runtime Preferences
- Agent Instance ↔ Project, Model, Hardware/Sandbox
- Memory Mounts ↔ Indexes and Datasets, referenced by Context Recipes

Extensibility
- Tools declare JSON Schemas and required capabilities; UI auto‑forms inputs, policy checks enforce capabilities, and results become first‑class events.
- Plugins install tools and may contribute optional UI panels/commands.

See also: Events Vocabulary, UI Architecture, Recipes, Context Recipes.

