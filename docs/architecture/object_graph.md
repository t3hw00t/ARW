---
title: Unified Object Graph
---

# Unified Object Graph
Updated: 2025-10-07
Type: Explanation

Core idea: treat the system as two things — a shared object graph (entities + relations) and a single event stream. Every surface (Project Hub, Chat, Training Park, Managers) is just a lens on that graph, driven by the same live events (SSE). This drastically reduces drift and keeps the experience coherent.

Global scope (inventories)
- Models, Tools, Policies, Hardware targets, Sandboxes/Containers, Plugins/Extensions, Logic Units
- Modular Cognitive Stack agents (chat, recall, compression, validation, tooling) registered through the orchestrator with typed contracts.

Project scope (bindings and use)
- Projects, Agent Profiles (definitions), Agent Instances (profile + runtime + policy), Data Sources, Memory Mounts (vector/graph/kv/doc indexes), Context Recipes (how to assemble the enhanced context), Logic Unit bindings (slots)

Design principle: managers edit inventories; Projects and Agents only reference those inventories. Duplicate data and hidden state are anti‑patterns.

Read‑models and state
- Server exposes `/state/*` read‑models (observations, beliefs, world, intents, actions, episodes, self/{agent}). These are the source of truth for UI and clients. The `world` view is a scoped, typed belief graph (Project Map) built from the event stream; the `self/{agent}` view is the agent’s metacognitive profile (capabilities, competence, calibration, resource curve, failure modes, interaction contract).
- Use correlation id (`corr_id`) to stitch events into episodes.

Relationships (examples)
- Project ↔ Files/Notes/Artifacts
- Agent Profile ↔ Tools/Policies/Runtime Preferences
- Agent Profile ↔ Logic Unit Slots (Retrieval, Reasoning, Sampling, Policy, Memory, Evaluation)
- Agent Profile ↔ Modular agent roster (assign which specialist LLM handles chat vs. recall); provenance states reference [Modular Cognitive Stack](modular_cognitive_stack.md).
- Agent Instance ↔ Project, Model, Hardware/Sandbox
- Memory Mounts ↔ Indexes and Datasets, referenced by Context Recipes

Extensibility
- Tools declare JSON Schemas and required capabilities; UI auto‑forms inputs, policy checks enforce capabilities, and results become first‑class events.
- Plugins install tools and may contribute optional UI panels/commands.

See also: Events Vocabulary, UI Architecture, Recipes, Context Recipes.
