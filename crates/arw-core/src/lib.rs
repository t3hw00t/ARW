use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
#[cfg(feature = "arrow-bench")]
pub mod arrow_ingest;

mod config;
pub use config::{config_schema_json, load_config, write_schema_file, Config};
pub mod hierarchy;
pub mod orchestrator;
#[cfg(feature = "nats")]
pub mod orchestrator_nats;
pub mod gating;
pub mod gating_keys;

#[cfg(feature = "wasm")]
pub mod wasm;

/// Public metadata describing a tool that can be registered into the runtime.
#[derive(Clone, Serialize)]
pub struct ToolInfo {
    pub id: &'static str,
    pub version: &'static str,
    pub summary: &'static str,
    pub stability: &'static str,
    pub capabilities: &'static [&'static str],
}

// Enable global registration/iteration of ToolInfo with the `inventory` crate.
inventory::collect!(ToolInfo);

/// Return all known tools (those submitted via `inventory::submit!`) plus a small
/// set of built‑in defaults to guarantee baseline functionality.
pub fn introspect_tools() -> Vec<ToolInfo> {
    let mut out: Vec<ToolInfo> = Vec::new();
    let mut seen: HashSet<&'static str> = HashSet::new();

    // Registered tools (from arw-macros #[arw_tool(...)] in other crates)
    for ti in inventory::iter::<ToolInfo> {
        if seen.insert(ti.id) {
            out.push(ti.clone());
        }
    }

    // Fallback defaults
    const DEFAULTS: &[ToolInfo] = &[
        ToolInfo {
            id: "memory.probe",
            version: "1.0.0",
            summary: "Read-only memory probe (shows applied memories and paths)",
            stability: "experimental",
            capabilities: &["read-only"],
        },
        ToolInfo {
            id: "introspect.tools",
            version: "1.0.0",
            summary: "List available tools with metadata",
            stability: "experimental",
            capabilities: &["read-only"],
        },
    ];

    for d in DEFAULTS {
        if seen.insert(d.id) {
            out.push(d.clone());
        }
    }
    out
}

/// Return a JSON Schema for a known tool id (small hand-authored schemas for now).
pub fn tool_schema(id: &str) -> Value {
    match id {
        // Schema for the /probe output you showed earlier
        "memory.probe" => json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "ProbeOut",
            "type": "object",
            "properties": {
                "portable":  { "type": "boolean" },
                "state_dir": { "type": "string"  },
                "cache_dir": { "type": "string"  },
                "logs_dir":  { "type": "string"  },
                "memory": {
                    "type": "object",
                    "required": ["ephemeral","episodic","semantic","procedural"],
                    "properties": {
                        "ephemeral":  { "type": "array" },
                        "episodic":   { "type": "array" },
                        "semantic":   { "type": "array" },
                        "procedural": { "type": "array" }
                    }
                }
            },
            "required": ["portable","state_dir","cache_dir","logs_dir","memory"]
        }),

        // Schema describing the list that /introspect/tools returns
        "introspect.tools" => json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "ToolInfoList",
            "type": "array",
            "items": {
                "type": "object",
                "required": ["id","version","summary","stability","capabilities"],
                "properties": {
                    "id":           { "type": "string" },
                    "version":      { "type": "string" },
                    "summary":      { "type": "string" },
                    "stability":    { "type": "string" },
                    "capabilities": { "type": "array", "items": { "type": "string" } }
                }
            }
        }),

        // Unknown tool id — return a minimal placeholder schema
        _ => json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "Unknown",
            "type": "object"
        }),
    }
}

/// Simple sanity function for arw-cli.
pub fn hello_core() -> &'static str {
    "arw-core ok"
}

/// Compute effective paths and portability flags (env-based; cross‑platform).
pub fn load_effective_paths() -> serde_json::Value {
    // Load defaults from config file if present, then overlay env vars
    let cfg_path = std::env::var("ARW_CONFIG")
        .ok()
        .unwrap_or_else(|| "configs/default.toml".to_string());
    let cfg = load_config(&cfg_path)
        .map_err(|e| {
            tracing::error!("invalid config {}: {}", cfg_path, e);
            e
        })
        .ok();

    let portable = std::env::var("ARW_PORTABLE")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.portable))
        .unwrap_or(false);

    let home_like = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    let norm = |s: String| s.replace('\\', "/");
    let expand = |mut s: String| {
        // Very small %VAR% and $VAR expansion for portability
        for (k, v) in std::env::vars() {
            let p1 = format!("%{}%", k);
            let p2 = format!("${}", k);
            if s.contains(&p1) {
                s = s.replace(&p1, &v);
            }
            if s.contains(&p2) {
                s = s.replace(&p2, &v);
            }
        }
        norm(s)
    };

    let state_dir = std::env::var("ARW_STATE_DIR")
        .ok()
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.state_dir.clone()))
        .unwrap_or_else(|| format!("{}/arw", home_like.clone()));
    let cache_dir = std::env::var("ARW_CACHE_DIR")
        .ok()
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.cache_dir.clone()))
        .unwrap_or_else(|| format!("{}/arw/cache", home_like.clone()));
    let logs_dir = std::env::var("ARW_LOGS_DIR")
        .ok()
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.logs_dir.clone()))
        .unwrap_or_else(|| format!("{}/arw/logs", home_like));

    serde_json::json!({
        "portable": portable,
        "state_dir": expand(state_dir),
        "cache_dir": expand(cache_dir),
        "logs_dir": expand(logs_dir),
        "memory": {
            "ephemeral": [],
            "episodic": [],
            "semantic": [],
            "procedural": []
        }
    })
}

/// Simple sanity function for arw-cli.
///
/// Compute effective paths and portability flags (env-based; cross‑platform).
///
/// Print effective paths to stderr (used by CLI).
pub fn print_effective_paths() {
    eprintln!("{}", load_effective_paths());
}

/// Option wrapper around tool_schema() for the service endpoint.
pub fn introspect_schema(id: &str) -> Option<serde_json::Value> {
    match id {
        "memory.probe" | "introspect.tools" => Some(tool_schema(id)),
        _ => None,
    }
}
