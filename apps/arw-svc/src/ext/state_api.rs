use arw_events::Envelope;
use axum::response::IntoResponse;
use axum::Json;
use arw_macros::arw_admin;
use once_cell::sync::OnceCell;
use serde::Serialize;
use serde_json::json;
use std::collections::VecDeque;
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

#[arw_admin(method="GET", path="/admin/state/observations", summary="Recent event observations")]
pub async fn observations_get() -> impl IntoResponse {
    let v = VERSION.load(Ordering::Relaxed);
    let s = store().read().unwrap();
    let items: Vec<Envelope> = s.recent.iter().cloned().collect();
    Json(json!({
        "version": v,
        "items": items,
    }))
}

// -------- Beliefs/Intents/Actions (versioned and rolling stores) --------

static BELIEFS_VER: OnceCell<AtomicU64> = OnceCell::new();
static BELIEFS: OnceCell<RwLock<Vec<serde_json::Value>>> = OnceCell::new();
fn beliefs_ver() -> &'static AtomicU64 { BELIEFS_VER.get_or_init(|| AtomicU64::new(0)) }
fn beliefs() -> &'static RwLock<Vec<serde_json::Value>> { BELIEFS.get_or_init(|| RwLock::new(Vec::new())) }

static INTENTS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn intents() -> &'static RwLock<VecDeque<serde_json::Value>> { INTENTS.get_or_init(|| RwLock::new(VecDeque::with_capacity(256))) }

static ACTIONS: OnceCell<RwLock<VecDeque<serde_json::Value>>> = OnceCell::new();
fn actions() -> &'static RwLock<VecDeque<serde_json::Value>> { ACTIONS.get_or_init(|| RwLock::new(VecDeque::with_capacity(256))) }

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
    // Intents: rolling list of generic Intents.* events
    if env.kind.starts_with("Intents.") {
        let mut q = intents().write().unwrap();
        if q.len() == q.capacity() { let _ = q.pop_front(); }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
    // Actions: rolling list of generic Actions.* events
    if env.kind.starts_with("Actions.") {
        let mut q = actions().write().unwrap();
        if q.len() == q.capacity() { let _ = q.pop_front(); }
        q.push_back(json!({"time": env.time, "kind": env.kind, "payload": env.payload}));
    }
}

#[arw_admin(method="GET", path="/admin/state/beliefs", summary="Current beliefs snapshot")]
pub async fn beliefs_get() -> impl IntoResponse {
    let v = beliefs_ver().load(Ordering::Relaxed);
    let s = beliefs().read().unwrap().clone();
    Json(json!({"version": v, "items": s}))
}
#[arw_admin(method="GET", path="/admin/state/intents", summary="Recent intents")]
pub async fn intents_get() -> impl IntoResponse {
    let s: Vec<_> = intents().read().unwrap().iter().cloned().collect();
    Json(json!({"items": s}))
}
#[arw_admin(method="GET", path="/admin/state/actions", summary="Recent actions")]
pub async fn actions_get() -> impl IntoResponse {
    let s: Vec<_> = actions().read().unwrap().iter().cloned().collect();
    Json(json!({"items": s}))
}
