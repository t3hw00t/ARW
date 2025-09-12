use arw_events::Envelope;
use arw_macros::arw_admin;
use axum::response::IntoResponse;
use chrono::{DateTime, Utc};
use once_cell::sync::OnceCell;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

#[derive(Default)]
struct ObsStore {
    recent: VecDeque<Envelope>,
}

static STORE: OnceCell<RwLock<ObsStore>> = OnceCell::new();
static VERSION: AtomicU64 = AtomicU64::new(0);

fn store() -> &'static RwLock<ObsStore> {
    STORE.get_or_init(|| RwLock::new(ObsStore::default()))
}

// Called from the main bus subscriber to fold in events to a compact read-model
pub async fn obs_on_event(env: &Envelope) {
    // Keep a small rolling window
    const CAP: usize = 256;
    let mut s = store().write().unwrap();
    if s.recent.len() == CAP {
        s.recent.pop_front();
    }
    s.recent.push_back(env.clone());
    VERSION.fetch_add(1, Ordering::Relaxed);
}

#[derive(Serialize)]
struct ObsSnapshot<'a> {
    version: u64,
    items: &'a [Envelope],
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/observations",
    summary = "Recent event observations"
)]
pub async fn observations_get() -> impl IntoResponse {
    let v = VERSION.load(Ordering::Relaxed);
    let s = store().read().unwrap();
    let items: Vec<Envelope> = s.recent.iter().cloned().collect();
    super::ok(json!({
        "version": v,
        "items": items,
    }))
}

// -------- Beliefs/Intents/Actions (versioned and rolling stores) --------

static BELIEFS_VER: OnceCell<AtomicU64> = OnceCell::new();
static BELIEFS: OnceCell<RwLock<Vec<serde_json::Value>>> = OnceCell::new();
fn beliefs_ver() -> &'static AtomicU64 {
    BELIEFS_VER.get_or_init(|| AtomicU64::new(0))
}
fn beliefs() -> &'static RwLock<Vec<serde_json::Value>> {
    BELIEFS.get_or_init(|| RwLock::new(Vec::new()))
}

static INTENTS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn intents() -> &'static RwLock<VecDeque<serde_json::Value>> {
    INTENTS.get_or_init(|| RwLock::new(VecDeque::with_capacity(256)))
}

static ACTIONS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn actions() -> &'static RwLock<VecDeque<serde_json::Value>> {
    ACTIONS.get_or_init(|| RwLock::new(VecDeque::with_capacity(256)))
}

pub async fn on_event(env: &Envelope) {
    // Always keep observations current
    obs_on_event(env).await;
    // Beliefs update from Feedback.Suggested or Beliefs.* events
    if env.kind == "Feedback.Suggested" || env.kind.starts_with("Beliefs.") {
        let mut list: Vec<serde_json::Value> = Vec::new();
        if let Some(arr) = env.payload.get("suggestions").and_then(|x| x.as_array()) {
            list = arr.clone();
        } else {
            // For generic Beliefs.* events, store payload as entry
            list.push(env.payload.clone());
        }
        {
            let mut s = beliefs().write().unwrap();
            *s = list;
        }
        beliefs_ver().fetch_add(1, Ordering::Relaxed);
    }
    // Logic Units: rolling list of unit events
    if env.kind.starts_with("LogicUnit.") {
        let mut q = logic_units().write().unwrap();
        if q.len() == q.capacity() {
            let _ = q.pop_front();
        }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
    // Experiments: rolling list of experiment events
    if env.kind.starts_with("Experiment.") {
        let mut q = experiments().write().unwrap();
        if q.len() == q.capacity() {
            let _ = q.pop_front();
        }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
    // Runtime matrix: keep last health per target/id when provided
    if env.kind == "Runtime.Health" {
        let mut m = runtime_matrix().write().unwrap();
        let key = env
            .payload
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("runtime")
            .to_string();
        m.insert(key, env.payload.clone());
    }
    // Intents: rolling list of generic Intents.* events
    if env.kind.starts_with("Intents.") {
        let mut q = intents().write().unwrap();
        if q.len() == q.capacity() {
            let _ = q.pop_front();
        }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
    // Actions: rolling list of generic Actions.* events
    if env.kind.starts_with("Actions.") {
        let mut q = actions().write().unwrap();
        if q.len() == q.capacity() {
            let _ = q.pop_front();
        }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
    // Episodes stitching by corr_id
    if let Some(cid) = env.payload.get("corr_id").and_then(|x| x.as_str()) {
        let mut map = episodes_map().write().unwrap();
        let ep = map.entry(cid.to_string()).or_insert_with(|| Episode {
            id: cid.to_string(),
            last: env.time.clone(),
            items: Vec::new(),
        });
        ep.last = env.time.clone();
        ep.items.push(env.clone());
        drop(map);
        let mut order = ep_order().write().unwrap();
        if !order.contains(&cid.to_string()) {
            if order.len() == order.capacity() {
                let _ = order.pop_front();
            }
            order.push_back(cid.to_string());
        }
    }
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/beliefs",
    summary = "Current beliefs snapshot"
)]
pub async fn beliefs_get() -> impl IntoResponse {
    let v = beliefs_ver().load(Ordering::Relaxed);
    let s = beliefs().read().unwrap().clone();
    super::ok(json!({"version": v, "items": s}))
}
#[arw_admin(
    method = "GET",
    path = "/admin/state/intents",
    summary = "Recent intents"
)]
pub async fn intents_get() -> impl IntoResponse {
    let s: Vec<_> = intents().read().unwrap().iter().cloned().collect();
    super::ok(json!({"items": s}))
}
#[arw_admin(
    method = "GET",
    path = "/admin/state/actions",
    summary = "Recent actions"
)]
pub async fn actions_get() -> impl IntoResponse {
    let s: Vec<_> = actions().read().unwrap().iter().cloned().collect();
    super::ok(json!({"items": s}))
}

// -------- Episodes (stitched by corr_id) --------

#[derive(Clone, Serialize)]
struct Episode {
    id: String,
    last: String,
    items: Vec<Envelope>,
}

static EPISODES: OnceCell<RwLock<HashMap<String, Episode>>> = OnceCell::new();
static EP_ORDER: OnceCell<RwLock<VecDeque<String>>> = OnceCell::new();
fn episodes_map() -> &'static RwLock<HashMap<String, Episode>> {
    EPISODES.get_or_init(|| RwLock::new(HashMap::new()))
}
fn ep_order() -> &'static RwLock<VecDeque<String>> {
    EP_ORDER.get_or_init(|| RwLock::new(VecDeque::with_capacity(64)))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/episodes",
    summary = "Recent episodes stitched by corr_id"
)]
pub async fn episodes_get() -> impl IntoResponse {
    let order = ep_order().read().unwrap();
    let map = episodes_map().read().unwrap();
    let mut items: Vec<serde_json::Value> = Vec::new();
    for id in order.iter() {
        if let Some(ep) = map.get(id) {
            // Compute simple rollup: count, duration_ms, errors
            let count = ep.items.len() as u64;
            let first_ts = ep.items.first().map(|e| e.time.as_str()).unwrap_or("");
            let last_ts = ep.items.last().map(|e| e.time.as_str()).unwrap_or("");
            let duration_ms = match (
                DateTime::parse_from_rfc3339(first_ts),
                DateTime::parse_from_rfc3339(last_ts),
            ) {
                (Ok(a), Ok(b)) => {
                    let am = a.with_timezone(&Utc).timestamp_millis();
                    let bm = b.with_timezone(&Utc).timestamp_millis();
                    bm.saturating_sub(am) as u64
                }
                _ => 0,
            };
            let errors = ep
                .items
                .iter()
                .filter(|e| e.payload.get("error").is_some())
                .count() as u64;
            items.push(json!({
                "id": ep.id,
                "last": ep.last,
                "count": count,
                "duration_ms": duration_ms,
                "errors": errors,
                "items": ep.items,
            }));
        }
    }
    super::ok(json!({"items": items}))
}

// -------- Logic Units / Experiments / Runtime Matrix / Policy leases --------

static LOGIC_UNITS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn logic_units() -> &'static RwLock<VecDeque<serde_json::Value>> {
    LOGIC_UNITS.get_or_init(|| RwLock::new(VecDeque::with_capacity(128)))
}

static EXPERIMENTS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn experiments() -> &'static RwLock<VecDeque<serde_json::Value>> {
    EXPERIMENTS.get_or_init(|| RwLock::new(VecDeque::with_capacity(128)))
}

static RUNTIME_MATRIX: OnceCell<RwLock<HashMap<String, serde_json::Value>>> = OnceCell::new();
fn runtime_matrix() -> &'static RwLock<HashMap<String, serde_json::Value>> {
    RUNTIME_MATRIX.get_or_init(|| RwLock::new(HashMap::new()))
}

static POLICY_LEASES: OnceCell<RwLock<Vec<serde_json::Value>>> = OnceCell::new();
fn policy_leases() -> &'static RwLock<Vec<serde_json::Value>> {
    POLICY_LEASES.get_or_init(|| RwLock::new(Vec::new()))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/logic_units",
    summary = "Recent Logic Unit events"
)]
pub async fn logic_units_get() -> impl IntoResponse {
    let s: Vec<_> = logic_units().read().unwrap().iter().cloned().collect();
    super::ok(json!({"items": s}))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/experiments",
    summary = "Recent experiment events"
)]
pub async fn experiments_get() -> impl IntoResponse {
    let s: Vec<_> = experiments().read().unwrap().iter().cloned().collect();
    super::ok(json!({"items": s}))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/runtime_matrix",
    summary = "Current runtime matrix snapshot"
)]
pub async fn runtime_matrix_get() -> impl IntoResponse {
    let s = runtime_matrix().read().unwrap().clone();
    super::ok(json!({"items": s}))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/policy",
    summary = "Active policy leases (capabilities)"
)]
pub async fn policy_state_get() -> impl IntoResponse {
    let s = policy_leases().read().unwrap().clone();
    super::ok(json!({"leases": s}))
}

// Snapshot of an episode (by corr_id)
#[arw_admin(
    method = "GET",
    path = "/admin/state/episode/:id/snapshot",
    summary = "Episode snapshot for reproducibility"
)]
pub async fn episode_snapshot_get(axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    let map = episodes_map().read().unwrap();
    if let Some(ep) = map.get(&id) {
        // Build a minimal snapshot; future: include effective config/model hashes, active units, etc.
        let first_ts = ep.items.first().map(|e| e.time.clone()).unwrap_or_default();
        let last_ts = ep.items.last().map(|e| e.time.clone()).unwrap_or_default();
        // Load current config for effective config enrichment (best-effort)
        let cfg = crate::ext::io::load_json_file_async(&crate::ext::paths::config_path())
            .await
            .unwrap_or_else(|| json!({}));
        return super::ok(json!({
            "id": ep.id,
            "started": first_ts,
            "ended": last_ts,
            "items": ep.items,
            "effective_config": {
                "logic_units": [],
                "config": cfg
            }
        }));
    }
    super::ok(json!({"error": "not_found"}))
}
