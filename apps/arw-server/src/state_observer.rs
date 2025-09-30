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

fn actions_store() -> &'static RwLock<VecDeque<Timed<Value>>> {
    static ACTIONS: OnceCell<RwLock<VecDeque<Timed<Value>>>> = OnceCell::new();
    ACTIONS.get_or_init(|| RwLock::new(VecDeque::with_capacity(ACTIONS_CAP)))
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

pub(crate) async fn intents_snapshot() -> Vec<Value> {
    intents_store()
        .read()
        .await
        .iter()
        .map(|item| item.value.clone())
        .collect()
}

pub(crate) async fn actions_snapshot() -> Vec<Value> {
    actions_store()
        .read()
        .await
        .iter()
        .map(|item| item.value.clone())
        .collect()
}
