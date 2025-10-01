use once_cell::sync::OnceCell;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use tracing::Level;

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
    capsules: HashMap<String, CapsuleRuntime>,
}

fn runtime_cell() -> &'static RwLock<RuntimeState> {
    RUNTIME.get_or_init(|| RwLock::new(RuntimeState::default()))
}

#[derive(Clone)]
struct CapsuleRuntime {
    denies: Vec<String>,
    contracts: Vec<Contract>,
    lease_until_ms: Option<u64>,
}

impl RuntimeState {
    fn prune_expired(&mut self, now: u64) {
        let expired: Vec<String> = self
            .capsules
            .iter()
            .filter_map(|(id, cap)| match cap.lease_until_ms {
                Some(until) if now >= until => Some(id.clone()),
                _ => None,
            })
            .collect();
        for id in expired {
            self.capsules.remove(&id);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CapsuleLeaseState {
    pub lease_until_ms: Option<u64>,
    pub renew_within_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
struct GatingPolicyConfig {
    /// Immutable denies applied at startup (supports trailing `*`).
    #[serde(default)]
    deny_user: Vec<String>,
    /// Conditional contracts evaluated alongside runtime capsules.
    #[serde(default)]
    contracts: Vec<ContractCfg>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ContractCfg {
    /// Stable identifier recorded in audits and renewals.
    id: String,
    /// List of gating key patterns (supports trailing `*`).
    #[serde(default)]
    patterns: Vec<String>,
    /// Optional role filter (e.g., `edge`, `regional`).
    #[serde(default)]
    subject_role: Option<String>,
    /// Optional node identifier filter (matches `ARW_NODE_ID`).
    #[serde(default)]
    subject_node: Option<String>,
    /// Match when any tag is present on the caller (case-sensitive).
    #[serde(default)]
    tags_any: Option<Vec<String>>,
    /// Contract activates after this epoch millisecond (inclusive).
    #[serde(default)]
    valid_from_ms: Option<u64>,
    /// Contract expires at this epoch millisecond (inclusive).
    #[serde(default)]
    valid_to_ms: Option<u64>,
    /// Automatically renew by this many seconds when expired.
    #[serde(default)]
    auto_renew_secs: Option<u64>,
    /// Treat contract as immutable within its active window (default true).
    #[serde(default)]
    immutable: Option<bool>,
    /// Maximum allowed hits within the quota window (optional).
    #[serde(default)]
    quota_limit: Option<u64>,
    /// Sliding window size in seconds paired with `quota_limit`.
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

#[inline]
fn gate_denied(key: &str, source: &str, pattern: Option<&str>) -> bool {
    if tracing::enabled!(Level::INFO) {
        let pattern = pattern.unwrap_or("");
        if let Some(meta) = crate::gating_keys::find(key) {
            tracing::info!(
                target: "arw::gating",
                key,
                source,
                pattern,
                title = meta.title,
                stability = meta.stability,
                summary = meta.summary,
                "gating denied"
            );
        } else {
            tracing::info!(target: "arw::gating", key, source, pattern, "gating denied");
        }
    }
    false
}

/// Initialize immutable user policy denies from a TOML file and env var.
pub fn init_from_config(path: &str) {
    let (deny_user, contracts) = load_config_entries(path);
    if !deny_user.is_empty() {
        let mut st = cell().write().unwrap();
        st.deny_user.extend(deny_user);
    }
    if !contracts.is_empty() {
        *contracts_cell().write().unwrap() = contracts;
    }
}

/// Set immutable user policy denies at runtime; adding only.
pub fn deny_user<I: IntoIterator<Item = String>>(keys: I) {
    let mut st = cell().write().unwrap();
    for k in keys {
        st.deny_user.insert(k);
    }
}

fn contract_from_cfg(c: ContractCfg) -> Contract {
    Contract {
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
    }
}

fn load_config_entries(path: &str) -> (HashSet<String>, Vec<Contract>) {
    let mut denies: HashSet<String> = HashSet::new();
    let mut contracts: Vec<Contract> = Vec::new();

    if Path::new(path).exists() {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(cfg) = toml::from_str::<GatingPolicyConfig>(&s) {
                denies.extend(cfg.deny_user);
                if !cfg.contracts.is_empty() {
                    contracts.extend(cfg.contracts.into_iter().map(contract_from_cfg));
                }
            }
        }
    }

    if let Ok(s) = std::env::var("ARW_GATING_DENY") {
        for k in s.split(',') {
            let trimmed = k.trim();
            if !trimmed.is_empty() {
                denies.insert(trimmed.to_string());
            }
        }
    }

    (denies, contracts)
}

pub fn reload_from_config(path: &str) {
    let (deny_user, contracts) = load_config_entries(path);
    {
        let mut st = cell().write().unwrap();
        st.deny_user.clear();
        st.deny_user.extend(deny_user);
    }
    let contract_ids: HashSet<String> = contracts.iter().map(|c| c.id.clone()).collect();
    {
        let mut list = contracts_cell().write().unwrap();
        *list = contracts;
    }
    {
        let mut runtime = runtime_cell().write().unwrap();
        runtime.expires.retain(|id, _| contract_ids.contains(id));
        runtime
            .budgets
            .retain(|(id, _), _| contract_ids.contains(id));
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
                return gate_denied(key, "user_policy", Some(p.as_str()));
            }
        }
        for p in &st.deny_hier {
            if wildcard_match(p, key) {
                return gate_denied(key, "hierarchy_policy", Some(p.as_str()));
            }
        }
    }
    // Runtime capsules (lease-based denies/contracts)
    let mut runtime = runtime_cell().write().unwrap();
    runtime.prune_expired(now);
    for cap in runtime.capsules.values() {
        for p in &cap.denies {
            if wildcard_match(p, key) {
                return gate_denied(key, "runtime_capsule", Some(p.as_str()));
            }
        }
    }
    // Contracts (deny with conditions, auto-renew)
    let role = crate::hierarchy::get_state().self_node.role;
    let role_s = format!("{:?}", role).to_lowercase();
    let node_id = std::env::var("ARW_NODE_ID").unwrap_or_else(|_| "local".into());
    let tags = crate::hierarchy::get_state().self_node.tags;
    let mut list = contracts_cell().read().unwrap().clone();
    for cap in runtime.capsules.values() {
        list.extend(cap.contracts.clone());
    }
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
                    return gate_denied(key, "quota_budget", Some(c.id.as_str()));
                }
                // allow this occurrence and increment counter
                ent.1 += 1;
                continue;
            }
            return gate_denied(key, "contract_active", Some(c.id.as_str()));
        }
        // Not active (expired) — auto renew?
        if !active {
            if let Some(renew) = c.auto_renew_secs {
                let new_to = now.saturating_add(renew * 1000);
                runtime.expires.insert(c.id.clone(), new_to);
                return gate_denied(key, "contract_auto_renew", Some(c.id.as_str()));
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
pub fn adopt_capsule(cap: &arw_protocol::GatingCapsule) -> CapsuleLeaseState {
    let now = now_ms();
    let lease_until = cap.lease_duration_ms.map(|dur| now.saturating_add(dur));
    let renew_within = cap.renew_within_ms;

    let mut runtime = runtime_cell().write().unwrap();
    let contracts: Vec<Contract> = cap
        .contracts
        .iter()
        .map(|c| Contract {
            id: format!("{}::{}", cap.id, c.id),
            patterns: c.patterns.clone(),
            subject_role: c.subject_role.clone(),
            subject_node: c.subject_node.clone(),
            tags_any: c.tags_any.clone(),
            valid_from_ms: c.valid_from_ms,
            valid_to_ms: c.valid_to_ms,
            auto_renew_secs: c.auto_renew_secs,
            immutable: c.immutable.unwrap_or(true),
            quota_limit: None,
            quota_window_secs: None,
        })
        .collect();

    runtime.capsules.insert(
        cap.id.clone(),
        CapsuleRuntime {
            denies: cap.denies.clone(),
            contracts,
            lease_until_ms: lease_until,
        },
    );

    CapsuleLeaseState {
        lease_until_ms: lease_until,
        renew_within_ms: renew_within,
    }
}

/// Return the JSON Schema describing `configs/gating.toml`.
pub fn gating_config_schema_json() -> serde_json::Value {
    let schema = schemars::schema_for!(GatingPolicyConfig);
    serde_json::to_value(&schema).expect("schema json")
}

/// Render the gating config reference in Markdown.
pub fn render_config_markdown(generated_at: &str) -> String {
    let mut out = format!(
        "---\ntitle: Gating Config\n---\n\n# Gating Config\nGenerated: {}\nType: Reference\n\n",
        generated_at
    );

    out.push_str("Immutable gating policy boots from `configs/gating.toml` or the `ARW_GATING_FILE` override. It layers with hierarchy defaults, runtime capsules, and leases so denies remain traceable and auditable. Keys support trailing `*` wildcards.\n\n");

    out.push_str("## Load order\n- `ARW_GATING_FILE` (absolute or relative) if set\n- `configs/gating.toml` discovered via `ARW_CONFIG_DIR`, the executable directory, or the workspace root\n- `ARW_GATING_DENY` environment variable (comma-separated)\n\n");

    out.push_str("## Schema\n- JSON Schema: [`gating_config.schema.json`](gating_config.schema.json)\n- Validate with `jsonschema`, `ajv`, or `arw-cli gate config schema`\n\n");

    out.push_str("## Top-level keys\n\n| Key | Type | Description |\n| --- | --- | --- |\n| `deny_user` | `array<string>` | Immutable deny-list applied at boot; supports trailing `*`. |\n| `contracts` | `array<Contract>` | Conditional denies evaluated on every request; supports filters, TTLs, quotas, and auto-renew. |\n\n");

    out.push_str("## Contract fields\n\n| Field | Type | Description |\n| --- | --- | --- |\n| `id` | `string` | Unique identifier recorded in audits and renewals. |\n| `patterns` | `array<string>` | Gating key patterns (supports trailing `*`). |\n| `subject_role` | `string?` | Optional caller role filter (`root`, `regional`, `edge`, `connector`, `observer`). |\n| `subject_node` | `string?` | Optional node id filter (`ARW_NODE_ID`). |\n| `tags_any` | `array<string>?` | Match when any tag from the caller overlaps. |\n| `valid_from_ms` | `integer?` | Epoch milliseconds that activate the contract (inclusive). |\n| `valid_to_ms` | `integer?` | Epoch milliseconds that expire the contract (inclusive). |\n| `auto_renew_secs` | `integer?` | Seconds to extend the contract after expiry. |\n| `immutable` | `bool?` | Defaults to `true`; when `false`, runtime may remove before expiry. |\n| `quota_limit` | `integer?` | Maximum invocations allowed within the sliding window. |\n| `quota_window_secs` | `integer?` | Sliding window size in seconds paired with `quota_limit`. |\n\n");

    out.push_str("## Field notes\n- `valid_from_ms` and `valid_to_ms` use milliseconds since Unix epoch.\n- Quotas require both `quota_limit` and `quota_window_secs`.\n- `auto_renew_secs` updates the next expiry relative to the evaluation time.\n\n");

    out.push_str("## Examples\n\n### Deny introspection by default\n```toml\ndeny_user = [\"introspect:*\"]\n```\n\n");
    out.push_str("### Nightly freeze for actions and tools\n```toml\n[[contracts]]\nid = \"night-freeze\"\npatterns = [\"actions:*\", \"tools:*\"]\nvalid_from_ms = 1735689600000  # 2024-12-01T00:00:00Z\nvalid_to_ms = 1735776000000    # 2024-12-02T00:00:00Z\nimmutable = true\n```\n\n");
    out.push_str("### Quota-limited edge tools burst\n```toml\n[[contracts]]\nid = \"edge-tools-burst\"\npatterns = [\"tools:run\"]\nsubject_role = \"edge\"\ntags_any = [\"lab\"]\nquota_limit = 5\nquota_window_secs = 60\nauto_renew_secs = 0\n```\n");

    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
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
    use serde_json::Value;
    use serial_test::serial;
    use std::path::PathBuf;

    fn repo_path(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn normalize_markdown(input: &str) -> String {
        let mut out = Vec::new();
        for line in input.replace("\r\n", "\n").lines() {
            if line.starts_with("Generated: ") {
                out.push("Generated: <timestamp>".to_string());
            } else {
                out.push(line.to_string());
            }
        }
        out.push(String::new());
        out.join("\n")
    }

    #[test]
    fn gating_config_markdown_fixture_in_sync() {
        let path = repo_path("../../docs/reference/gating_config.md");
        let disk = std::fs::read_to_string(&path).expect("read gating_config.md");
        let generated = super::render_config_markdown("GENERATED");
        assert_eq!(
            normalize_markdown(&disk),
            normalize_markdown(&generated),
            "docs/reference/gating_config.md is out of sync with render_config_markdown()"
        );
    }

    #[test]
    fn gating_config_schema_fixture_in_sync() {
        let path = repo_path("../../docs/reference/gating_config.schema.json");
        let disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read schema"))
                .expect("parse schema json");
        let generated = super::gating_config_schema_json();
        assert_eq!(
            disk,
            generated,
            "docs/reference/gating_config.schema.json is out of sync with gating_config_schema_json()"
        );
    }

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
        assert!(!allowed("events:task.completed"));
    }
}
