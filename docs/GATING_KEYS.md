---
title: Gating Keys
---

# Gating Keys
Updated: 2025-10-20
Generated: 2025-10-20 00:55 UTC
Type: Reference

Generated from code.

## Overview

- Groups: 13
- Keys: 48

## Orchestration

Task queueing and lifecycle events.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `queue:enqueue` | Queue Enqueue | stable | Schedule work onto the orchestrator queue. |
| `events:task.completed` | Task Completed Event | stable | Emit task lifecycle completion events to subscribers. |

## Memory

Retrieval-augmented memory operations and quota management.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `memory:get` | Memory Fetch | stable | Read stored memory capsules for retrieval augmented operations. |
| `memory:save` | Memory Save | stable | Persist a new memory capsule to the shared store. |
| `memory:load` | Memory Load | stable | Load memory collections into the active runtime context. |
| `memory:apply` | Memory Apply | stable | Apply memory updates or patches to existing capsules. |
| `memory:limit:get` | Memory Limit Inspect | stable | Inspect the configured memory quota for a scope. |
| `memory:limit:set` | Memory Limit Update | stable | Adjust the allowed memory quota for a scope. |

## Runtime Supervisor

Managed runtime orchestration and lifecycle controls.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `runtime:manage` | Runtime Manage | beta | Start, restore, or stop managed runtimes via the supervisor. |

## Models

Model registry, lifecycle, and distribution controls.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `models:list` | Models List | stable | Enumerate registered model adapters and aliases. |
| `models:refresh` | Models Refresh | stable | Refresh model registry metadata from upstream sources. |
| `models:save` | Models Save | stable | Persist model artifacts or configuration snapshots. |
| `models:load` | Models Load | stable | Load a stored model artifact for execution. |
| `models:add` | Models Add | stable | Register a new model alias or adapter. |
| `models:delete` | Models Delete | stable | Remove an existing model alias or adapter. |
| `models:default:get` | Default Model Get | stable | Read the global default model selection. |
| `models:default:set` | Default Model Set | stable | Update the global default model selection. |
| `models:download` | Models Download | beta | Download remote model artifacts for local use. |

## Feedback

Feedback collection, automation, and application.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `feedback:state` | Feedback State | stable | Inspect the active feedback controller state. |
| `feedback:signal` | Feedback Signal | stable | Emit a feedback signal event. |
| `feedback:analyze` | Feedback Analyze | beta | Run feedback analyzers on collected signals. |
| `feedback:apply` | Feedback Apply | stable | Apply feedback-driven adjustments to the system. |
| `feedback:auto` | Feedback Auto | experimental | Toggle automated feedback processing flows. |
| `feedback:reset` | Feedback Reset | stable | Reset accumulated feedback state. |

## Tools

Tool discovery and invocation.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `tools:list` | Tools List | stable | List available tool integrations. |
| `tools:run` | Tools Run | beta | Invoke a tool on behalf of an agent. |

## Chat

Interactive chat lifecycle and assurance checks.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `chat:send` | Chat Send | stable | Send a message to an active chat session. |
| `chat:clear` | Chat Clear | stable | Clear a conversation transcript. |
| `chat:self_consistency` | Chat Self-Consistency | experimental | Trigger self-consistency evaluation across chat responses. |
| `chat:verify` | Chat Verify | beta | Run verification routines on chat outputs. |

## Governor

Safety and steering policies applied by the governor.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `governor:set` | Governor Set | stable | Update governor policies for orchestration and safety. |
| `governor:hints:set` | Governor Hints Set | beta | Configure hint prompts for the governor. |

## Hierarchy

Hierarchical coordination handshakes and state access.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `hierarchy:hello` | Hierarchy Hello | stable | Introduce an agent to the coordination hierarchy. |
| `hierarchy:offer` | Hierarchy Offer | beta | Offer capabilities to the hierarchy controller. |
| `hierarchy:accept` | Hierarchy Accept | beta | Accept assignments from the hierarchy controller. |
| `hierarchy:state:get` | Hierarchy State Get | stable | Inspect the current state of the hierarchy. |
| `hierarchy:role:set` | Hierarchy Role Set | stable | Assign or update roles within the hierarchy. |

## Introspection

Internal observability APIs for diagnostics.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `introspect:tools` | Introspect Tools | stable | Discover the available introspection tools. |
| `introspect:schema` | Introspect Schema | stable | Fetch the introspection schema definitions. |
| `introspect:stats` | Introspect Stats | stable | Read system health and runtime statistics. |
| `introspect:probe` | Introspect Probe | experimental | Execute deep health probes on internal subsystems. |

## Administration

Administrative controls and lifecycle hooks.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `admin:shutdown` | Admin Shutdown | stable | Trigger a controlled shutdown of the system. |
| `admin:emit` | Admin Emit | stable | Emit administrative diagnostics or events. |

## Regulatory Provenance

Trust and provenance management via the RPU.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `rpu:trust:get` | RPU Trust Get | stable | Inspect the trust ledger maintained by the RPU. |
| `rpu:trust:reload` | RPU Trust Reload | stable | Reload trust policies and provenance rules. |

## Projects

Project workspace file management.

| Key | Title | Stability | Purpose |
| --- | --- | --- | --- |
| `projects:file:get` | Project File Get | stable | Read the contents of a tracked project file. |
| `projects:file:set` | Project File Set | stable | Write or replace the contents of a project file. |
| `projects:file:patch` | Project File Patch | beta | Apply a patch to a project file. |

