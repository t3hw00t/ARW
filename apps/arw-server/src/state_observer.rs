use arw_events::Envelope;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::{tasks::TaskHandle, AppState};

const OBS_CAP: usize = 256;
const INTENTS_CAP: usize = 256;
const ACTIONS_CAP: usize = 256;
const OBS_TTL: Duration = Duration::from_secs(600);
const INTENTS_TTL: Duration = Duration::from_secs(600);
const ACTIONS_TTL: Duration = Duration::from_secs(600);

#[derive(Clone)]
struct Timed<T> {
    inserted_at: Instant,
    value: T,
}

impl<T> Timed<T> {
    fn new(value: T) -> Self {
        Self {
            inserted_at: Instant::now(),
            value,
        }
    }
}

fn prune_deque<T>(deque: &mut VecDeque<Timed<T>>, ttl: Duration, now: Instant) {
    while let Some(front) = deque.front() {
        if now.duration_since(front.inserted_at) > ttl {
            deque.pop_front();
        } else {
            break;
        }
    }
}

fn observations_store() -> &'static RwLock<VecDeque<Timed<Envelope>>> {
    static STORE: OnceCell<RwLock<VecDeque<Timed<Envelope>>>> = OnceCell::new();
    STORE.get_or_init(|| RwLock::new(VecDeque::with_capacity(OBS_CAP)))
}

fn observations_version() -> &'static AtomicU64 {
    static VERSION: OnceCell<AtomicU64> = OnceCell::new();
    VERSION.get_or_init(|| AtomicU64::new(0))
}

fn beliefs_store() -> &'static RwLock<Vec<Value>> {
    static BELIEFS: OnceCell<RwLock<Vec<Value>>> = OnceCell::new();
    BELIEFS.get_or_init(|| RwLock::new(Vec::new()))
}

fn beliefs_version() -> &'static AtomicU64 {
    static VERSION: OnceCell<AtomicU64> = OnceCell::new();
    VERSION.get_or_init(|| AtomicU64::new(0))
}

fn intents_store() -> &'static RwLock<VecDeque<Timed<Value>>> {
    static INTENTS: OnceCell<RwLock<VecDeque<Timed<Value>>>> = OnceCell::new();
    INTENTS.get_or_init(|| RwLock::new(VecDeque::with_capacity(INTENTS_CAP)))
}

fn intents_version() -> &'static AtomicU64 {
    static VERSION: OnceCell<AtomicU64> = OnceCell::new();
    VERSION.get_or_init(|| AtomicU64::new(0))
}

fn actions_store() -> &'static RwLock<VecDeque<Timed<Value>>> {
    static ACTIONS: OnceCell<RwLock<VecDeque<Timed<Value>>>> = OnceCell::new();
    ACTIONS.get_or_init(|| RwLock::new(VecDeque::with_capacity(ACTIONS_CAP)))
}

fn actions_version() -> &'static AtomicU64 {
    static VERSION: OnceCell<AtomicU64> = OnceCell::new();
    VERSION.get_or_init(|| AtomicU64::new(0))
}

pub(crate) fn actions_version_value() -> u64 {
    actions_version().load(Ordering::Relaxed)
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let handle = tokio::spawn(async move {
        let mut rx = state.bus().subscribe();
        while let Ok(env) = rx.recv().await {
            on_event(&env).await;
        }
    });
    vec![TaskHandle::new("state_observer.bus_listener", handle)]
}

async fn on_event(env: &Envelope) {
    push_observation(env).await;
    update_beliefs(env).await;
    update_intents(env).await;
    update_actions(env).await;
}

async fn push_observation(env: &Envelope) {
    let mut store = observations_store().write().await;
    let now = Instant::now();
    prune_deque(&mut store, OBS_TTL, now);
    if store.len() == OBS_CAP {
        store.pop_front();
    }
    store.push_back(Timed::new(env.clone()));
    observations_version().fetch_add(1, Ordering::Relaxed);
}

async fn update_beliefs(env: &Envelope) {
    if env.kind.as_str() == "feedback.suggested" || env.kind.starts_with("beliefs.") {
        let mut list: Vec<Value> = Vec::new();
        if let Some(arr) = env.payload.get("suggestions").and_then(|v| v.as_array()) {
            list = arr.to_vec();
        } else {
            list.push(env.payload.clone());
        }
        let mut guard = beliefs_store().write().await;
        *guard = list;
        beliefs_version().fetch_add(1, Ordering::Relaxed);
    }
}

async fn update_intents(env: &Envelope) {
    if env.kind.starts_with("intents.") {
        let mut store = intents_store().write().await;
        let now = Instant::now();
        prune_deque(&mut store, INTENTS_TTL, now);
        if store.len() == INTENTS_CAP {
            store.pop_front();
        }
        store.push_back(Timed::new(json!({
            "time": env.time,
            "kind": env.kind,
            "payload": env.payload
        })));
        intents_version().fetch_add(1, Ordering::Relaxed);
    }
}

async fn update_actions(env: &Envelope) {
    if env.kind.starts_with("actions.") {
        let mut store = actions_store().write().await;
        let now = Instant::now();
        prune_deque(&mut store, ACTIONS_TTL, now);
        if store.len() == ACTIONS_CAP {
            store.pop_front();
        }
        store.push_back(Timed::new(json!({
            "time": env.time,
            "kind": env.kind,
            "payload": env.payload
        })));
        actions_version().fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) async fn observations_snapshot() -> (u64, Vec<Envelope>) {
    let version = observations_version().load(Ordering::Relaxed);
    let guard = observations_store().read().await;
    let items: Vec<Envelope> = guard.iter().map(|entry| entry.value.clone()).collect();
    (version, items)
}

pub(crate) async fn beliefs_snapshot() -> (u64, Vec<Value>) {
    let version = beliefs_version().load(Ordering::Relaxed);
    let items = beliefs_store().read().await.clone();
    (version, items)
}

pub(crate) async fn intents_snapshot() -> (u64, Vec<Value>) {
    let version = intents_version().load(Ordering::Relaxed);
    let items = intents_store()
        .read()
        .await
        .iter()
        .map(|item| item.value.clone())
        .collect();
    (version, items)
}

pub(crate) async fn actions_snapshot() -> (u64, Vec<Value>) {
    let version = actions_version().load(Ordering::Relaxed);
    let items = actions_store()
        .read()
        .await
        .iter()
        .map(|item| item.value.clone())
        .collect();
    (version, items)
}

#[cfg(test)]
pub(crate) async fn reset_for_tests() {
    fn reset_version(cell: &AtomicU64) {
        cell.store(0, Ordering::Relaxed);
    }

    async fn clear_store<T>(lock: &RwLock<VecDeque<T>>) {
        let mut guard = lock.write().await;
        guard.clear();
    }

    async fn clear_vec_store(lock: &RwLock<Vec<Value>>) {
        let mut guard = lock.write().await;
        guard.clear();
    }

    clear_store(observations_store()).await;
    reset_version(observations_version());
    clear_vec_store(beliefs_store()).await;
    reset_version(beliefs_version());
    clear_store(intents_store()).await;
    reset_version(intents_version());
    clear_store(actions_store()).await;
    reset_version(actions_version());
}

#[cfg(test)]
pub(crate) async fn ingest_for_tests(env: &Envelope) {
    on_event(env).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{SecondsFormat, Utc};
    use std::sync::atomic::Ordering;

    fn intents_version_value() -> u64 {
        super::intents_version().load(Ordering::Relaxed)
    }

    fn env(kind: &str) -> Envelope {
        Envelope {
            time: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            kind: kind.to_string(),
            payload: json!({"corr_id": "test"}),
            policy: None,
            ce: None,
        }
    }

    #[tokio::test]
    async fn intents_snapshot_increments_version() {
        let _env_lock = crate::test_support::env::guard();
        reset_for_tests().await;
        assert_eq!(intents_version_value(), 0);

        update_intents(&env("intents.proposed")).await;
        let (v1, items1) = intents_snapshot().await;
        assert_eq!(v1, 1);
        assert_eq!(items1.len(), 1);
        assert_eq!(intents_version_value(), v1);

        update_intents(&env("intents.accepted")).await;
        let (v2, items2) = intents_snapshot().await;
        assert!(v2 > v1);
        assert_eq!(items2.len(), 2);
        assert_eq!(intents_version_value(), v2);
    }

    #[tokio::test]
    async fn actions_snapshot_increments_version() {
        let _env_lock = crate::test_support::env::guard();
        reset_for_tests().await;
        assert_eq!(actions_version_value(), 0);

        update_actions(&env("actions.completed")).await;
        let (v1, items1) = actions_snapshot().await;
        assert_eq!(v1, 1);
        assert_eq!(items1.len(), 1);
        assert_eq!(actions_version_value(), v1);

        update_actions(&env("actions.failed")).await;
        let (v2, items2) = actions_snapshot().await;
        assert!(v2 > v1);
        assert_eq!(items2.len(), 2);
        assert_eq!(actions_version_value(), v2);
    }
}
