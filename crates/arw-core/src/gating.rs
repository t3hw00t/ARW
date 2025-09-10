use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::RwLock;

#[derive(Default, Clone, Serialize)]
pub struct GateState {
    // Immutable denies set by user policy (config/env). Once set, cannot be removed at runtime.
    deny_user: HashSet<String>,
    // Immutable denies set by hierarchy/negotiation. Once set, cannot be removed at runtime.
    deny_hier: HashSet<String>,
}

static STATE: OnceCell<RwLock<GateState>> = OnceCell::new();

fn cell() -> &'static RwLock<GateState> {
    STATE.get_or_init(|| RwLock::new(GateState::default()))
}

#[derive(Debug, Deserialize)]
struct GatingCfg {
    #[serde(default)]
    deny_user: Vec<String>,
}

fn wildcard_match(pattern: &str, key: &str) -> bool {
    if let Some(pfx) = pattern.strip_suffix('*') {
        key.starts_with(pfx)
    } else {
        pattern == key
    }
}

/// Initialize immutable user policy denies from a TOML file and env var.
pub fn init_from_config(path: &str) {
    let mut denies: HashSet<String> = HashSet::new();
    // File
    if Path::new(path).exists() {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(cfg) = toml::from_str::<GatingCfg>(&s) {
                for k in cfg.deny_user { denies.insert(k); }
            }
        }
    }
    // Env: comma-separated
    if let Ok(s) = std::env::var("ARW_GATING_DENY") {
        for k in s.split(',') { let k = k.trim(); if !k.is_empty() { denies.insert(k.to_string()); } }
    }
    if !denies.is_empty() {
        let mut st = cell().write().unwrap();
        st.deny_user.extend(denies);
    }
}

/// Set immutable user policy denies at runtime; adding only.
pub fn deny_user<I: IntoIterator<Item = String>>(keys: I) {
    let mut st = cell().write().unwrap();
    for k in keys { st.deny_user.insert(k); }
}

/// Set immutable hierarchy denies at runtime; adding only.
pub fn deny_hierarchy<I: IntoIterator<Item = String>>(keys: I) {
    let mut st = cell().write().unwrap();
    for k in keys { st.deny_hier.insert(k); }
}

/// Check if a key is allowed (deny wins; patterns with trailing * are supported).
pub fn allowed(key: &str) -> bool {
    let st = cell().read().unwrap();
    for p in &st.deny_user { if wildcard_match(p, key) { return false; } }
    for p in &st.deny_hier { if wildcard_match(p, key) { return false; } }
    true
}

/// Role → default gating
#[derive(Debug, Clone, Copy)]
pub enum Role {
    Root,
    Regional,
    Edge,
    Connector,
    Observer,
}

/// Apply immutable default denies for a role. Observer is most restrictive.
pub fn apply_role_defaults(role: Role) {
    let mut denies: Vec<String> = Vec::new();
    match role {
        Role::Observer => {
            denies.extend([
                "queue:*",            // no task movement
                "task:*",             // no task kinds
                "tools:*",            // no tool execution
                "memory:*",           // no memory change
                "models:*",           // no model operations
            ].iter().map(|s| s.to_string()));
        }
        Role::Connector => {
            // Connectors may not mutate local models/memory by default
            denies.extend(["memory:*", "models:*"].iter().map(|s| s.to_string()));
        }
        _ => {}
    }
    if !denies.is_empty() { deny_hierarchy(denies); }
}

/// Snapshot for introspection.
pub fn snapshot() -> serde_json::Value {
    let st = cell().read().unwrap();
    serde_json::json!({
        "deny_user": st.deny_user,
        "deny_hierarchy": st.deny_hier,
    })
}

