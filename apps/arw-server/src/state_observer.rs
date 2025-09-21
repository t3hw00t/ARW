use arw_events::Envelope;
use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::{tasks::TaskHandle, AppState};

const OBS_CAP: usize = 256;
const INTENTS_CAP: usize = 256;
const ACTIONS_CAP: usize = 256;

fn observations_store() -> &'static RwLock<VecDeque<Envelope>> {
    static STORE: OnceCell<RwLock<VecDeque<Envelope>>> = OnceCell::new();
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

fn intents_store() -> &'static RwLock<VecDeque<Value>> {
    static INTENTS: OnceCell<RwLock<VecDeque<Value>>> = OnceCell::new();
    INTENTS.get_or_init(|| RwLock::new(VecDeque::with_capacity(INTENTS_CAP)))
}

fn actions_store() -> &'static RwLock<VecDeque<Value>> {
    static ACTIONS: OnceCell<RwLock<VecDeque<Value>>> = OnceCell::new();
    ACTIONS.get_or_init(|| RwLock::new(VecDeque::with_capacity(ACTIONS_CAP)))
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let handle = tokio::spawn(async move {
        let mut rx = state.bus().subscribe();
        while let Ok(env) = rx.recv().await {
            on_event(&env);
        }
    });
    vec![TaskHandle::new("state_observer.bus_listener", handle)]
}

fn on_event(env: &Envelope) {
    push_observation(env);
    update_beliefs(env);
    update_intents(env);
    update_actions(env);
}

fn push_observation(env: &Envelope) {
    let mut store = observations_store().write().unwrap();
    if store.len() == OBS_CAP {
        store.pop_front();
    }
    store.push_back(env.clone());
    observations_version().fetch_add(1, Ordering::Relaxed);
}

fn update_beliefs(env: &Envelope) {
    if env.kind.as_str() == "feedback.suggested" || env.kind.starts_with("beliefs.") {
        let mut list: Vec<Value> = Vec::new();
        if let Some(arr) = env.payload.get("suggestions").and_then(|v| v.as_array()) {
            list = arr.to_vec();
        } else {
            list.push(env.payload.clone());
        }
        {
            let mut guard = beliefs_store().write().unwrap();
            *guard = list;
        }
        beliefs_version().fetch_add(1, Ordering::Relaxed);
    }
}

fn update_intents(env: &Envelope) {
    if env.kind.starts_with("intents.") {
        let mut store = intents_store().write().unwrap();
        if store.len() == INTENTS_CAP {
            store.pop_front();
        }
        store.push_back(json!({
            "time": env.time,
            "kind": env.kind,
            "payload": env.payload
        }));
    }
}

fn update_actions(env: &Envelope) {
    if env.kind.starts_with("actions.") {
        let mut store = actions_store().write().unwrap();
        if store.len() == ACTIONS_CAP {
            store.pop_front();
        }
        store.push_back(json!({
            "time": env.time,
            "kind": env.kind,
            "payload": env.payload
        }));
    }
}

pub(crate) fn observations_snapshot() -> (u64, Vec<Envelope>) {
    let version = observations_version().load(Ordering::Relaxed);
    let items: Vec<Envelope> = observations_store()
        .read()
        .unwrap()
        .iter()
        .cloned()
        .collect();
    (version, items)
}

pub(crate) fn beliefs_snapshot() -> (u64, Vec<Value>) {
    let version = beliefs_version().load(Ordering::Relaxed);
    let items = beliefs_store().read().unwrap().clone();
    (version, items)
}

pub(crate) fn intents_snapshot() -> Vec<Value> {
    intents_store().read().unwrap().iter().cloned().collect()
}

pub(crate) fn actions_snapshot() -> Vec<Value> {
    actions_store().read().unwrap().iter().cloned().collect()
}
