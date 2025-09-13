use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
#[cfg(feature = "arrow-bench")]
pub mod arrow_ingest;

mod config;
pub use config::{config_schema_json, load_config, write_schema_file, Config};
pub mod gating;
pub mod gating_keys;
pub mod hierarchy;
pub mod orchestrator;
#[cfg(feature = "nats")]
pub mod orchestrator_nats;
pub mod rpu;

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

// ---------------- Administrative Endpoint Registry ----------------
/// Metadata describing an admin/ops HTTP endpoint served by arw-svc under `/admin`.
#[derive(Clone, Serialize)]
pub struct AdminEndpoint {
    pub method: &'static str,
    pub path: &'static str,
    #[serde(default)]
    pub summary: &'static str,
}

// Collect admin endpoints registered via `arw-macros::arw_admin` at compile time.
inventory::collect!(AdminEndpoint);

/// List all registered admin endpoints. Sorted by path+method and deduplicated.
pub fn list_admin_endpoints() -> Vec<AdminEndpoint> {
    let mut out: Vec<AdminEndpoint> = inventory::iter::<AdminEndpoint>
        .into_iter()
        .cloned()
        .collect();
    out.sort_by(|a, b| a.path.cmp(b.path).then(a.method.cmp(b.method)));
    out.dedup_by(|a, b| a.path == b.path && a.method == b.method);
    out
}

/// Compute effective paths and portability flags (env-based; cross‑platform).
pub fn load_effective_paths() -> serde_json::Value {
    // Load defaults from config file if present, then overlay env vars
    // Resolve config path independent of current working directory.
    let cfg_path_env = std::env::var("ARW_CONFIG").ok();
    let cfg_resolved = match cfg_path_env {
        Some(p) => Some(std::path::PathBuf::from(p)),
        None => resolve_config_path("configs/default.toml"),
    };
    let cfg = cfg_resolved
        .as_ref()
        .and_then(|p| p.to_str())
        .and_then(|p| match load_config(p) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::error!(
                    "invalid config {}: {}",
                    cfg_resolved
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "<none>".into()),
                    e
                );
                None
            }
        });

    let portable = std::env::var("ARW_PORTABLE")
        .ok()
        .and_then(|v| v.parse::<bool>().ok())
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.portable))
        .unwrap_or(false);

    // Compute sensible OS-specific defaults (Windows Known Folders, XDG, etc.)
    // Use directories::ProjectDirs to anchor per-user app paths consistently.
    // Fallback to envs if directories can't be resolved.
    let proj_dirs = directories::ProjectDirs::from("org", "arw", "arw");
    let default_state: String = proj_dirs
        .as_ref()
        .map(|p| p.data_local_dir().to_string_lossy().to_string())
        // Fallbacks preserve previous behavior: prefer LOCALAPPDATA on Windows, HOME elsewhere
        .or_else(|| std::env::var("LOCALAPPDATA").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());
    let default_cache: String = proj_dirs
        .as_ref()
        .map(|p| p.cache_dir().to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}/arw/cache", default_state.clone()));
    let default_logs: String = proj_dirs
        .as_ref()
        .map(|p| {
            p.data_local_dir()
                .join("logs")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| format!("{}/arw/logs", default_state.clone()));

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
        .unwrap_or_else(|| default_state.clone());
    let cache_dir = std::env::var("ARW_CACHE_DIR")
        .ok()
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.cache_dir.clone()))
        .unwrap_or_else(|| default_cache.clone());
    let logs_dir = std::env::var("ARW_LOGS_DIR")
        .ok()
        .or_else(|| cfg.as_ref().and_then(|c| c.runtime.logs_dir.clone()))
        .unwrap_or_else(|| default_logs.clone());

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

/// Resolve a config file path independent of the current working directory.
///
/// Search order (first existing wins):
/// - `ARW_CONFIG_DIR` environment variable if set (joined with `rel`)
/// - Directory of the current executable (joined with `rel`)
/// - Parent of the executable directory (joined with `rel`) — useful for dev layouts
/// - Workspace root during development (relative to this crate's manifest): `../../` (joined with `rel`)
/// - Current working directory (joined with `rel`)
///
/// If `rel` is absolute, it is returned if it exists.
pub fn resolve_config_path(rel: &str) -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return if rel_path.exists() {
            Some(rel_path.to_path_buf())
        } else {
            None
        };
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(cfg_dir) = std::env::var("ARW_CONFIG_DIR") {
        if !cfg_dir.trim().is_empty() {
            candidates.push(PathBuf::from(cfg_dir));
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.to_path_buf());
            if let Some(parent) = exe_dir.parent() {
                candidates.push(parent.to_path_buf());
            }
        }
    }

    // Dev convenience: from arw-core crate dir to workspace root
    let dev_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../");
    candidates.push(dev_root);

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }

    for base in candidates.into_iter() {
        let p = base.join(rel);
        if p.exists() {
            return Some(p);
        }
    }
    None
}
