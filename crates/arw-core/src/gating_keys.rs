//! Central registry of gating keys for SoT and docs.
//!
//! The registry keeps operational metadata alongside each key so runtime
//! diagnostics, documentation, and policy tooling stay aligned.

use serde_json::{json, Value};

/// Human oriented description of a gating key.
#[derive(Debug, Clone, Copy)]
pub struct GatingKey {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub stability: &'static str,
}

impl GatingKey {
    pub const fn new(
        id: &'static str,
        title: &'static str,
        summary: &'static str,
        stability: &'static str,
    ) -> Self {
        Self {
            id,
            title,
            summary,
            stability,
        }
    }
}

/// Collection of related gating keys.
#[derive(Debug, Clone, Copy)]
pub struct GatingKeyGroup {
    pub name: &'static str,
    pub summary: &'static str,
    pub keys: &'static [GatingKey],
}

impl GatingKeyGroup {
    pub const fn new(
        name: &'static str,
        summary: &'static str,
        keys: &'static [GatingKey],
    ) -> Self {
        Self {
            name,
            summary,
            keys,
        }
    }
}

macro_rules! gating_catalog {
    (
        $(
            group {
                name: $group_name:expr,
                summary: $group_summary:expr,
                keys: [
                    $(
                        $key_const:ident => {
                            id: $key_id:expr,
                            title: $key_title:expr,
                            summary: $key_summary:expr,
                            stability: $key_stability:expr
                        }
                    ),* $(,)?
                ]
            }
        ),* $(,)?
    ) => {
        $(
            $(
                pub const $key_const: &str = $key_id;
            )*
        )*

        const GROUPS: &[GatingKeyGroup] = &[
            $(GatingKeyGroup::new(
                $group_name,
                $group_summary,
                &[
                    $(GatingKey::new(
                        $key_const,
                        $key_title,
                        $key_summary,
                        $key_stability,
                    )),*
                ],
            )),*
        ];

        const ALL_KEYS: &[&'static str] = &[
            $(
                $( $key_const ),*
            ),*
        ];
    };
}

gating_catalog! {
    group {
        name: "Orchestration",
        summary: "Task queueing and lifecycle events.",
        keys: [
            QUEUE_ENQUEUE => {
                id: "queue:enqueue",
                title: "Queue Enqueue",
                summary: "Schedule work onto the orchestrator queue.",
                stability: "stable"
            },
            EVENTS_TASK_COMPLETED => {
                id: "events:task.completed",
                title: "Task Completed Event",
                summary: "Emit task lifecycle completion events to subscribers.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Memory",
        summary: "Retrieval-augmented memory operations and quota management.",
        keys: [
            MEMORY_GET => {
                id: "memory:get",
                title: "Memory Fetch",
                summary: "Read stored memory capsules for retrieval augmented operations.",
                stability: "stable"
            },
            MEMORY_SAVE => {
                id: "memory:save",
                title: "Memory Save",
                summary: "Persist a new memory capsule to the shared store.",
                stability: "stable"
            },
            MEMORY_LOAD => {
                id: "memory:load",
                title: "Memory Load",
                summary: "Load memory collections into the active runtime context.",
                stability: "stable"
            },
            MEMORY_APPLY => {
                id: "memory:apply",
                title: "Memory Apply",
                summary: "Apply memory updates or patches to existing capsules.",
                stability: "stable"
            },
            MEMORY_LIMIT_GET => {
                id: "memory:limit:get",
                title: "Memory Limit Inspect",
                summary: "Inspect the configured memory quota for a scope.",
                stability: "stable"
            },
            MEMORY_LIMIT_SET => {
                id: "memory:limit:set",
                title: "Memory Limit Update",
                summary: "Adjust the allowed memory quota for a scope.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Runtime Supervisor",
        summary: "Managed runtime orchestration and lifecycle controls.",
        keys: [
            RUNTIME_MANAGE => {
                id: "runtime:manage",
                title: "Runtime Manage",
                summary: "Start, restore, or stop managed runtimes via the supervisor.",
                stability: "beta"
            }
        ]
    },
    group {
        name: "Models",
        summary: "Model registry, lifecycle, and distribution controls.",
        keys: [
            MODELS_LIST => {
                id: "models:list",
                title: "Models List",
                summary: "Enumerate registered model adapters and aliases.",
                stability: "stable"
            },
            MODELS_REFRESH => {
                id: "models:refresh",
                title: "Models Refresh",
                summary: "Refresh model registry metadata from upstream sources.",
                stability: "stable"
            },
            MODELS_SAVE => {
                id: "models:save",
                title: "Models Save",
                summary: "Persist model artifacts or configuration snapshots.",
                stability: "stable"
            },
            MODELS_LOAD => {
                id: "models:load",
                title: "Models Load",
                summary: "Load a stored model artifact for execution.",
                stability: "stable"
            },
            MODELS_ADD => {
                id: "models:add",
                title: "Models Add",
                summary: "Register a new model alias or adapter.",
                stability: "stable"
            },
            MODELS_DELETE => {
                id: "models:delete",
                title: "Models Delete",
                summary: "Remove an existing model alias or adapter.",
                stability: "stable"
            },
            MODELS_DEFAULT_GET => {
                id: "models:default:get",
                title: "Default Model Get",
                summary: "Read the global default model selection.",
                stability: "stable"
            },
            MODELS_DEFAULT_SET => {
                id: "models:default:set",
                title: "Default Model Set",
                summary: "Update the global default model selection.",
                stability: "stable"
            },
            MODELS_DOWNLOAD => {
                id: "models:download",
                title: "Models Download",
                summary: "Download remote model artifacts for local use.",
                stability: "beta"
            }
        ]
    },
    group {
        name: "Feedback",
        summary: "Feedback collection, automation, and application.",
        keys: [
            FEEDBACK_STATE => {
                id: "feedback:state",
                title: "Feedback State",
                summary: "Inspect the active feedback controller state.",
                stability: "stable"
            },
            FEEDBACK_SIGNAL => {
                id: "feedback:signal",
                title: "Feedback Signal",
                summary: "Emit a feedback signal event.",
                stability: "stable"
            },
            FEEDBACK_ANALYZE => {
                id: "feedback:analyze",
                title: "Feedback Analyze",
                summary: "Run feedback analyzers on collected signals.",
                stability: "beta"
            },
            FEEDBACK_APPLY => {
                id: "feedback:apply",
                title: "Feedback Apply",
                summary: "Apply feedback-driven adjustments to the system.",
                stability: "stable"
            },
            FEEDBACK_AUTO => {
                id: "feedback:auto",
                title: "Feedback Auto",
                summary: "Toggle automated feedback processing flows.",
                stability: "experimental"
            },
            FEEDBACK_RESET => {
                id: "feedback:reset",
                title: "Feedback Reset",
                summary: "Reset accumulated feedback state.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Tools",
        summary: "Tool discovery and invocation.",
        keys: [
            TOOLS_LIST => {
                id: "tools:list",
                title: "Tools List",
                summary: "List available tool integrations.",
                stability: "stable"
            },
            TOOLS_RUN => {
                id: "tools:run",
                title: "Tools Run",
                summary: "Invoke a tool on behalf of an agent.",
                stability: "beta"
            }
        ]
    },
    group {
        name: "Chat",
        summary: "Interactive chat lifecycle and assurance checks.",
        keys: [
            CHAT_SEND => {
                id: "chat:send",
                title: "Chat Send",
                summary: "Send a message to an active chat session.",
                stability: "stable"
            },
            CHAT_CLEAR => {
                id: "chat:clear",
                title: "Chat Clear",
                summary: "Clear a conversation transcript.",
                stability: "stable"
            },
            CHAT_SELF_CONSISTENCY => {
                id: "chat:self_consistency",
                title: "Chat Self-Consistency",
                summary: "Trigger self-consistency evaluation across chat responses.",
                stability: "experimental"
            },
            CHAT_VERIFY => {
                id: "chat:verify",
                title: "Chat Verify",
                summary: "Run verification routines on chat outputs.",
                stability: "beta"
            }
        ]
    },
    group {
        name: "Governor",
        summary: "Safety and steering policies applied by the governor.",
        keys: [
            GOVERNOR_SET => {
                id: "governor:set",
                title: "Governor Set",
                summary: "Update governor policies for orchestration and safety.",
                stability: "stable"
            },
            GOVERNOR_HINTS_SET => {
                id: "governor:hints:set",
                title: "Governor Hints Set",
                summary: "Configure hint prompts for the governor.",
                stability: "beta"
            }
        ]
    },
    group {
        name: "Hierarchy",
        summary: "Hierarchical coordination handshakes and state access.",
        keys: [
            HIERARCHY_HELLO => {
                id: "hierarchy:hello",
                title: "Hierarchy Hello",
                summary: "Introduce an agent to the coordination hierarchy.",
                stability: "stable"
            },
            HIERARCHY_OFFER => {
                id: "hierarchy:offer",
                title: "Hierarchy Offer",
                summary: "Offer capabilities to the hierarchy controller.",
                stability: "beta"
            },
            HIERARCHY_ACCEPT => {
                id: "hierarchy:accept",
                title: "Hierarchy Accept",
                summary: "Accept assignments from the hierarchy controller.",
                stability: "beta"
            },
            HIERARCHY_STATE_GET => {
                id: "hierarchy:state:get",
                title: "Hierarchy State Get",
                summary: "Inspect the current state of the hierarchy.",
                stability: "stable"
            },
            HIERARCHY_ROLE_SET => {
                id: "hierarchy:role:set",
                title: "Hierarchy Role Set",
                summary: "Assign or update roles within the hierarchy.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Introspection",
        summary: "Internal observability APIs for diagnostics.",
        keys: [
            INTROSPECT_TOOLS => {
                id: "introspect:tools",
                title: "Introspect Tools",
                summary: "Discover the available introspection tools.",
                stability: "stable"
            },
            INTROSPECT_SCHEMA => {
                id: "introspect:schema",
                title: "Introspect Schema",
                summary: "Fetch the introspection schema definitions.",
                stability: "stable"
            },
            INTROSPECT_STATS => {
                id: "introspect:stats",
                title: "Introspect Stats",
                summary: "Read system health and runtime statistics.",
                stability: "stable"
            },
            INTROSPECT_PROBE => {
                id: "introspect:probe",
                title: "Introspect Probe",
                summary: "Execute deep health probes on internal subsystems.",
                stability: "experimental"
            }
        ]
    },
    group {
        name: "Administration",
        summary: "Administrative controls and lifecycle hooks.",
        keys: [
            ADMIN_SHUTDOWN => {
                id: "admin:shutdown",
                title: "Admin Shutdown",
                summary: "Trigger a controlled shutdown of the system.",
                stability: "stable"
            },
            ADMIN_EMIT => {
                id: "admin:emit",
                title: "Admin Emit",
                summary: "Emit administrative diagnostics or events.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Regulatory Provenance",
        summary: "Trust and provenance management via the RPU.",
        keys: [
            RPU_TRUST_GET => {
                id: "rpu:trust:get",
                title: "RPU Trust Get",
                summary: "Inspect the trust ledger maintained by the RPU.",
                stability: "stable"
            },
            RPU_TRUST_RELOAD => {
                id: "rpu:trust:reload",
                title: "RPU Trust Reload",
                summary: "Reload trust policies and provenance rules.",
                stability: "stable"
            }
        ]
    },
    group {
        name: "Projects",
        summary: "Project workspace file management.",
        keys: [
            PROJECTS_FILE_GET => {
                id: "projects:file:get",
                title: "Project File Get",
                summary: "Read the contents of a tracked project file.",
                stability: "stable"
            },
            PROJECTS_FILE_SET => {
                id: "projects:file:set",
                title: "Project File Set",
                summary: "Write or replace the contents of a project file.",
                stability: "stable"
            },
            PROJECTS_FILE_PATCH => {
                id: "projects:file:patch",
                title: "Project File Patch",
                summary: "Apply a patch to a project file.",
                stability: "beta"
            }
        ]
    }
}

/// Return all known static keys (dynamic keys like task:<id> are omitted).
pub fn list() -> &'static [&'static str] {
    ALL_KEYS
}

/// Iterate over the gating key metadata grouped by domain.
pub fn groups() -> &'static [GatingKeyGroup] {
    GROUPS
}

/// Locate a gating key definition by identifier.
pub fn find(id: &str) -> Option<&'static GatingKey> {
    groups()
        .iter()
        .flat_map(|group| group.keys.iter())
        .find(|key| key.id == id)
}

/// Render a Markdown reference for documentation.
pub fn render_markdown(generated_at: &str) -> String {
    let groups = groups();
    let total_keys: usize = groups.iter().map(|group| group.keys.len()).sum();
    let mut out = format!(
        "---\ntitle: Gating Keys\n---\n\n# Gating Keys\nGenerated: {}\nType: Reference\n\nGenerated from code.\n\n## Overview\n\n- Groups: {}\n- Keys: {}\n\n",
        generated_at,
        groups.len(),
        total_keys
    );

    for group in groups {
        out.push_str(&format!("## {}\n\n{}\n\n", group.name, group.summary));
        out.push_str("| Key | Title | Stability | Purpose |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for key in group.keys {
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                key.id, key.title, key.stability, key.summary
            ));
        }
        out.push('\n');
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Render the gating catalog as JSON for tooling.
pub fn render_json(generated_at: Option<&str>) -> Value {
    let groups = groups();
    let total_keys: usize = groups.iter().map(|group| group.keys.len()).sum();

    let mut root = json!({
        "type": "reference",
        "total_groups": groups.len(),
        "total_keys": total_keys,
        "groups": groups
            .iter()
            .map(|group| {
                json!({
                    "name": group.name,
                    "summary": group.summary,
                    "keys": group
                        .keys
                        .iter()
                        .map(|key| {
                            json!({
                                "id": key.id,
                                "title": key.title,
                                "summary": key.summary,
                                "stability": key.stability,
                            })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>(),
    });

    if let Some(ts) = generated_at {
        root["generated"] = json!(ts);
    }

    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn normalize_markdown(input: &str) -> String {
        let mut out = Vec::new();
        for line in input.replace("\r\n", "\n").lines() {
            if line.starts_with("Updated: ") {
                continue;
            } else if line.starts_with("Generated: ") {
                out.push("Generated: <timestamp>".to_string());
            } else {
                out.push(line.to_string());
            }
        }
        out.join("\n") + "\n"
    }

    fn normalize_json(mut value: Value) -> Value {
        if let Value::Object(ref mut map) = value {
            map.insert("generated".into(), Value::String("<timestamp>".into()));
        }
        value
    }

    #[test]
    fn markdown_contains_all_keys() {
        let md = render_markdown("TEST");
        for key in list() {
            assert!(md.contains(key), "missing key {} in markdown", key);
        }
    }

    #[test]
    fn json_contains_all_keys() {
        let json = render_json(None);
        let mut seen = std::collections::HashSet::new();
        for group in json["groups"].as_array().expect("groups array") {
            for key in group["keys"].as_array().expect("keys array") {
                let id = key["id"].as_str().expect("id str");
                seen.insert(id.to_string());
            }
        }
        assert_eq!(seen.len(), list().len());
        for key in list() {
            assert!(seen.contains(*key));
        }
    }

    #[test]
    fn markdown_fixture_in_sync() {
        let path = repo_path("../../docs/GATING_KEYS.md");
        let disk = std::fs::read_to_string(&path).expect("read GATING_KEYS.md");
        let generated = render_markdown("GENERATED");
        assert_eq!(
            normalize_markdown(&disk),
            normalize_markdown(&generated),
            "docs/GATING_KEYS.md is out of sync with render_markdown()"
        );
    }

    #[test]
    fn json_fixture_in_sync() {
        let path = repo_path("../../docs/GATING_KEYS.json");
        let disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read GATING_KEYS.json"))
                .expect("parse json");
        let generated = render_json(Some("GENERATED"));
        assert_eq!(
            normalize_json(disk),
            normalize_json(generated),
            "docs/GATING_KEYS.json is out of sync with render_json()"
        );
    }
}
