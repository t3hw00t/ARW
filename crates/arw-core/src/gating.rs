use once_cell::sync::OnceCell;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
static CONTRACTS: OnceCell<RwLock<Vec<Contract>>> = OnceCell::new();
static RUNTIME: OnceCell<RwLock<RuntimeState>> = OnceCell::new();
const MAX_CONTRACTS: usize = 2048;

fn cell() -> &'static RwLock<GateState> {
    STATE.get_or_init(|| RwLock::new(GateState::default()))
}

fn contracts_cell() -> &'static RwLock<Vec<Contract>> {
    CONTRACTS.get_or_init(|| RwLock::new(Vec::new()))
}

#[derive(Default)]
struct RuntimeState {
    // Auto-renew override
    expires: HashMap<String, u64>, // contract_id -> valid_to_ms (runtime override)
    // Budget counters: (contract_id, key) -> (window_start_ms, count)
    budgets: HashMap<(String, String), (u64, u64)>,
}

fn runtime_cell() -> &'static RwLock<RuntimeState> {
    RUNTIME.get_or_init(|| RwLock::new(RuntimeState::default()))
}

#[derive(Debug, Deserialize)]
struct GatingCfg {
    #[serde(default)]
    deny_user: Vec<String>,
    #[serde(default)]
    contracts: Vec<ContractCfg>,
}

#[derive(Debug, Deserialize, Clone, JsonSchema)]
pub struct ContractCfg {
    id: String,
    #[serde(default)]
    patterns: Vec<String>, // e.g., ["events:*", "task:math.*"]
    #[serde(default)]
    subject_role: Option<String>,
    #[serde(default)]
    subject_node: Option<String>,
    #[serde(default)]
    tags_any: Option<Vec<String>>, // match any of these tags
    #[serde(default)]
    valid_from_ms: Option<u64>,
    #[serde(default)]
    valid_to_ms: Option<u64>,
    #[serde(default)]
    auto_renew_secs: Option<u64>,
    #[serde(default)]
    immutable: Option<bool>,
    #[serde(default)]
    quota_limit: Option<u64>,
    #[serde(default)]
    quota_window_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Contract {
    id: String,
    patterns: Vec<String>,
    subject_role: Option<String>,
    subject_node: Option<String>,
    tags_any: Option<Vec<String>>,
    valid_from_ms: Option<u64>,
    valid_to_ms: Option<u64>,
    auto_renew_secs: Option<u64>,
    #[allow(dead_code)]
    immutable: bool,
    quota_limit: Option<u64>,
    quota_window_secs: Option<u64>,
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
                for k in cfg.deny_user {
                    denies.insert(k);
                }
                if !cfg.contracts.is_empty() {
                    let mut out: Vec<Contract> = Vec::new();
                    for c in cfg.contracts.into_iter() {
                        out.push(Contract {
                            id: c.id,
                            patterns: c.patterns,
                            subject_role: c.subject_role,
                            subject_node: c.subject_node,
                            tags_any: c.tags_any,
                            valid_from_ms: c.valid_from_ms,
                            valid_to_ms: c.valid_to_ms,
                            auto_renew_secs: c.auto_renew_secs,
                            immutable: c.immutable.unwrap_or(true),
                            quota_limit: c.quota_limit,
                            quota_window_secs: c.quota_window_secs,
                        });
                    }
                    *contracts_cell().write().unwrap() = out;
                }
            }
        }
    }
    // Env: comma-separated
    if let Ok(s) = std::env::var("ARW_GATING_DENY") {
        for k in s.split(',') {
            let k = k.trim();
            if !k.is_empty() {
                denies.insert(k.to_string());
            }
        }
    }
    if !denies.is_empty() {
        let mut st = cell().write().unwrap();
        st.deny_user.extend(denies);
    }
}

/// Set immutable user policy denies at runtime; adding only.
pub fn deny_user<I: IntoIterator<Item = String>>(keys: I) {
    let mut st = cell().write().unwrap();
    for k in keys {
        st.deny_user.insert(k);
    }
}

/// Set immutable hierarchy denies at runtime; adding only.
pub fn deny_hierarchy<I: IntoIterator<Item = String>>(keys: I) {
    let mut st = cell().write().unwrap();
    for k in keys {
        st.deny_hier.insert(k);
    }
}

/// Check if a key is allowed (deny wins; patterns with trailing * are supported).
pub fn allowed(key: &str) -> bool {
    let now = now_ms();
    // User/hierarchy immutable sets first
    {
        let st = cell().read().unwrap();
        for p in &st.deny_user {
            if wildcard_match(p, key) {
                return false;
            }
        }
        for p in &st.deny_hier {
            if wildcard_match(p, key) {
                return false;
            }
        }
    }
    // Contracts (deny with conditions, auto-renew)
    let role = crate::hierarchy::get_state().self_node.role;
    let role_s = format!("{:?}", role).to_lowercase();
    let node_id = std::env::var("ARW_NODE_ID").unwrap_or_else(|_| "local".into());
    let tags = crate::hierarchy::get_state().self_node.tags;
    let mut runtime = runtime_cell().write().unwrap();
    let list = contracts_cell().read().unwrap().clone();
    for c in list.iter() {
        // Match pattern
        if !c.patterns.iter().any(|p| wildcard_match(p, key)) {
            continue;
        }
        // Subject filters
        if let Some(r) = &c.subject_role {
            if r.to_lowercase() != role_s {
                continue;
            }
        }
        if let Some(n) = &c.subject_node {
            if n != &node_id {
                continue;
            }
        }
        if let Some(any) = &c.tags_any {
            if !any.iter().any(|t| tags.contains(t)) {
                continue;
            }
        }
        // Window check and renewal
        let from = c.valid_from_ms.unwrap_or(0);
        let to = runtime.expires.get(&c.id).cloned().or(c.valid_to_ms);
        let active = if let Some(t) = to {
            now >= from && now <= t
        } else {
            now >= from
        };
        if active {
            // Budget enforcement if configured (deny once exhausted)
            if let (Some(limit), Some(win)) = (c.quota_limit, c.quota_window_secs) {
                let ent = runtime
                    .budgets
                    .entry((c.id.clone(), key.to_string()))
                    .or_insert((now, 0));
                if now.saturating_sub(ent.0) > win * 1000 {
                    ent.0 = now;
                    ent.1 = 0;
                }
                if ent.1 >= limit {
                    return false;
                }
                // allow this occurrence and increment counter
                ent.1 += 1;
                continue;
            }
            return false; // plain deny while active
        }
        // Not active (expired) — auto renew?
        if !active {
            if let Some(renew) = c.auto_renew_secs {
                let new_to = now.saturating_add(renew * 1000);
                runtime.expires.insert(c.id.clone(), new_to);
                return false; // becomes active immediately on first check
            }
        }
    }
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
            denies.extend(
                [
                    "queue:*",  // no task movement
                    "task:*",   // no task kinds
                    "tools:*",  // no tool execution
                    "memory:*", // no memory change
                    "models:*", // no model operations
                ]
                .iter()
                .map(|s| s.to_string()),
            );
        }
        Role::Connector => {
            // Connectors may not mutate local models/memory by default
            denies.extend(["memory:*", "models:*"].iter().map(|s| s.to_string()));
        }
        _ => {}
    }
    if !denies.is_empty() {
        deny_hierarchy(denies);
    }
}

/// Snapshot for introspection.
pub fn snapshot() -> serde_json::Value {
    let st = cell().read().unwrap();
    let contracts = contracts_cell().read().unwrap();
    serde_json::json!({
        "deny_user": st.deny_user,
        "deny_hierarchy": st.deny_hier,
        "contracts": contracts.iter().map(|c| {
            let mut m = serde_json::Map::new();
            m.insert("id".into(), c.id.clone().into());
            m.insert("patterns".into(), serde_json::to_value(&c.patterns).unwrap_or(serde_json::json!([])));
            m.insert("subject_role".into(), c.subject_role.clone().unwrap_or_default().into());
            m.insert("subject_node".into(), c.subject_node.clone().unwrap_or_default().into());
            m.insert("auto_renew_secs".into(), c.auto_renew_secs.unwrap_or(0).into());
            serde_json::Value::Object(m)
        }).collect::<Vec<_>>()
    })
}

#[inline]
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Add a contract at runtime (immutable within its own window). Id must be unique per source.
pub fn add_contract_cfg(c: ContractCfg) {
    let mut list = contracts_cell().write().unwrap();
    // Replace if same id exists; otherwise append (bounded by MAX_CONTRACTS)
    let newc = Contract {
        id: c.id,
        patterns: c.patterns,
        subject_role: c.subject_role,
        subject_node: c.subject_node,
        tags_any: c.tags_any,
        valid_from_ms: c.valid_from_ms,
        valid_to_ms: c.valid_to_ms,
        auto_renew_secs: c.auto_renew_secs,
        immutable: c.immutable.unwrap_or(true),
        quota_limit: c.quota_limit,
        quota_window_secs: c.quota_window_secs,
    };
    if let Some(pos) = list.iter().position(|x| x.id == newc.id) {
        list[pos] = newc;
    } else {
        if list.len() >= MAX_CONTRACTS {
            // Drop oldest to keep bounded
            list.remove(0);
        }
        list.push(newc);
    }
}

/// Adopt a GatingCapsule from the wire (policy propagation). Trust policy is caller's responsibility.
pub fn adopt_capsule(cap: &arw_protocol::GatingCapsule) {
    if !cap.denies.is_empty() {
        deny_hierarchy(cap.denies.clone());
    }
    for c in &cap.contracts {
        add_contract_cfg(ContractCfg {
            id: format!("{}::{}", cap.id, c.id),
            patterns: c.patterns.clone(),
            subject_role: c.subject_role.clone(),
            subject_node: c.subject_node.clone(),
            tags_any: c.tags_any.clone(),
            valid_from_ms: c.valid_from_ms,
            valid_to_ms: c.valid_to_ms,
            auto_renew_secs: c.auto_renew_secs,
            immutable: c.immutable,
            quota_limit: None,
            quota_window_secs: None,
        });
    }
}

#[cfg(test)]
/// Test-only: reset gating state to a clean slate for isolated tests.
pub fn __test_reset() {
    {
        let mut st = cell().write().unwrap();
        st.deny_user.clear();
        st.deny_hier.clear();
    }
    {
        let mut list = contracts_cell().write().unwrap();
        list.clear();
    }
    {
        let mut rt = runtime_cell().write().unwrap();
        rt.expires.clear();
        rt.budgets.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn deny_user_and_wildcard() {
        __test_reset();
        // Exact deny
        deny_user(vec!["tools:list".to_string()]);
        assert!(!allowed("tools:list"));
        assert!(allowed("tools:run"));

        // Wildcard deny
        deny_user(vec!["task:math.*".to_string()]);
        assert!(!allowed("task:math.add"));
        assert!(!allowed("task:math.sub"));
        assert!(allowed("task:other"));
    }

    #[test]
    #[serial]
    fn contracts_window_and_auto_renew() {
        __test_reset();
        let now = super::now_ms();
        add_contract_cfg(ContractCfg {
            id: "test-contract".into(),
            patterns: vec!["events:*".into()],
            subject_role: None,
            subject_node: None,
            tags_any: None,
            valid_from_ms: Some(now.saturating_sub(1000)),
            valid_to_ms: Some(now.saturating_add(1000)),
            auto_renew_secs: Some(1),
            immutable: Some(true),
            quota_limit: None,
            quota_window_secs: None,
        });
        assert!(!allowed("events:Task.Completed"));
    }
}
