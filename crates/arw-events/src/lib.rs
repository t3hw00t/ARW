use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Minimal event envelope (RFC3339 time).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Envelope {
    pub time: String,
    pub kind: String,
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<arw_protocol::GatingCapsule>,
}

/// Pluggable event bus API. For now subscribe returns a local channel receiver
/// to keep existing callers working; remote implementations may fan-in to a
/// local relay.
pub trait EventBus: Send + Sync + Clone + 'static {
    fn subscribe(&self) -> broadcast::Receiver<Envelope>;
    fn publish<T: Serialize>(&self, kind: &str, payload: &T);
    fn publish_with_policy<T: Serialize>(
        &self,
        kind: &str,
        payload: &T,
        policy: Option<arw_protocol::GatingCapsule>,
    );
    /// Subscribe to a filtered view of the bus that forwards only events
    /// whose kind starts with any of the provided prefixes.
    fn subscribe_filtered(
        &self,
        prefixes: Vec<String>,
        capacity: Option<usize>,
    ) -> broadcast::Receiver<Envelope>;
}

/// Local in-process bus backed by tokio broadcast channels.
#[derive(Default)]
struct Counters {
    published: AtomicU64,
    delivered: AtomicU64,
    lagged: AtomicU64,
    no_receivers: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusStats {
    pub published: u64,
    pub delivered: u64,
    pub lagged: u64,
    pub no_receivers: u64,
    pub receivers: usize,
}

#[derive(Clone)]
pub struct LocalBus {
    tx: broadcast::Sender<Envelope>,
    counters: Arc<Counters>,
    replay: Arc<Mutex<VecDeque<Envelope>>>,
    replay_cap: usize,
    journal: Option<PathBuf>,
    journal_lock: Arc<Mutex<()>>,
}

impl LocalBus {
    pub fn new(capacity: usize) -> Self {
        Self::new_with_replay(capacity, 256)
    }
    pub fn new_with_replay(capacity: usize, replay_cap: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        let journal = std::env::var("ARW_EVENTS_JOURNAL").ok().map(PathBuf::from);
        Self {
            tx,
            counters: Arc::new(Counters::default()),
            replay: Arc::new(Mutex::new(VecDeque::with_capacity(replay_cap)) ),
            replay_cap,
            journal,
            journal_lock: Arc::new(Mutex::new(())),
        }
    }
    pub fn stats(&self) -> BusStats {
        BusStats {
            published: self.counters.published.load(Ordering::Relaxed),
            delivered: self.counters.delivered.load(Ordering::Relaxed),
            lagged: self.counters.lagged.load(Ordering::Relaxed),
            no_receivers: self.counters.no_receivers.load(Ordering::Relaxed),
            receivers: self.tx.receiver_count(),
        }
    }
    pub fn note_lag(&self, n: u64) {
        self.counters.lagged.fetch_add(n, Ordering::Relaxed);
    }
    pub fn replay(&self, n: usize) -> Vec<Envelope> {
        let rb = self.replay.lock().unwrap();
        let len = rb.len();
        let take = n.min(len);
        rb.iter()
            .skip(len.saturating_sub(take))
            .cloned()
            .collect()
    }
}

impl EventBus for LocalBus {
    fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }
    fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let val = serde_json::to_value(payload)
            .unwrap_or_else(|_| serde_json::json!({ "_ser": "error" }));
        let env = Envelope {
            time: now,
            kind: kind.to_string(),
            payload: val,
            policy: None,
        };
        self.counters.published.fetch_add(1, Ordering::Relaxed);
        match self.tx.send(env.clone()) {
            Ok(n) => {
                self.counters
                    .delivered
                    .fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(_e) => {
                self.counters
                    .no_receivers
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        // Optional journal
        self.maybe_journal_env(&env);
        // Push to replay buffer
        let mut rb = self.replay.lock().unwrap();
        if rb.len() == self.replay_cap {
            rb.pop_front();
        }
        rb.push_back(env);
    }
    fn publish_with_policy<T: Serialize>(
        &self,
        kind: &str,
        payload: &T,
        policy: Option<arw_protocol::GatingCapsule>,
    ) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let val = serde_json::to_value(payload)
            .unwrap_or_else(|_| serde_json::json!({ "_ser": "error" }));
        let env = Envelope {
            time: now,
            kind: kind.to_string(),
            payload: val,
            policy,
        };
        self.counters.published.fetch_add(1, Ordering::Relaxed);
        match self.tx.send(env.clone()) {
            Ok(n) => {
                self.counters
                    .delivered
                    .fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(_e) => {
                self.counters
                    .no_receivers
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.maybe_journal_env(&env);
        let mut rb = self.replay.lock().unwrap();
        if rb.len() == self.replay_cap {
            rb.pop_front();
        }
        rb.push_back(env);
    }
    fn subscribe_filtered(
        &self,
        prefixes: Vec<String>,
        capacity: Option<usize>,
    ) -> broadcast::Receiver<Envelope> {
        let (tx, rx) = broadcast::channel(capacity.unwrap_or(128));
        let mut src = self.tx.subscribe();
        let prefs: Vec<String> = prefixes.into_iter().collect();
        let out = tx.clone();
        tokio::spawn(async move {
            while let Ok(env) = src.recv().await {
                let k = env.kind.as_str();
                if prefs.iter().any(|p| k.starts_with(p)) {
                    let _ = out.send(env);
                }
                if out.receiver_count() == 0 {
                    break;
                }
            }
        });
        rx
    }
}

impl LocalBus {
    fn maybe_journal_env(&self, env: &Envelope) {
        let path = match &self.journal { Some(p) => p.clone(), None => return };
        let lk = self.journal_lock.clone();
        let line = match serde_json::to_string(env) {
            Ok(mut s) => { s.push('\n'); s },
            Err(_) => return,
        };
        tokio::task::spawn_blocking(move || {
            let _g = lk.lock().unwrap();
            let max_mb: u64 = std::env::var("ARW_EVENTS_JOURNAL_MAX_MB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20);
            let max_bytes = max_mb.saturating_mul(1024 * 1024);
            if let Ok(md) = std::fs::metadata(&path) {
                if md.len() >= max_bytes {
                    let p1 = path.with_extension("log.1");
                    let p2 = path.with_extension("log.2");
                    let p3 = path.with_extension("log.3");
                    let _ = std::fs::remove_file(&p3);
                    if p2.exists() { let _ = std::fs::rename(&p2, &p3); }
                    if p1.exists() { let _ = std::fs::rename(&p1, &p2); }
                    if path.exists() { let _ = std::fs::rename(&path, &p1); }
                }
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                use std::io::Write as _;
                let _ = f.write_all(line.as_bytes());
            }
        });
    }
}
/// Backward compatible façade that current apps use.
#[derive(Clone)]
pub struct Bus {
    inner: LocalBus,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: LocalBus::new(capacity),
        }
    }
    pub fn new_with_replay(capacity: usize, replay_cap: usize) -> Self {
        Self {
            inner: LocalBus::new_with_replay(capacity, replay_cap),
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.inner.subscribe()
    }
    pub fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        self.inner.publish(kind, payload)
    }
    pub fn publish_with_policy<T: Serialize>(
        &self,
        kind: &str,
        payload: &T,
        policy: Option<arw_protocol::GatingCapsule>,
    ) {
        self.inner.publish_with_policy(kind, payload, policy)
    }
    pub fn note_lag(&self, n: u64) {
        self.inner.note_lag(n)
    }
    pub fn stats(&self) -> BusStats {
        self.inner.stats()
    }
    pub fn replay(&self, n: usize) -> Vec<Envelope> {
        self.inner.replay(n)
    }
    pub fn subscribe_filtered(
        &self,
        prefixes: Vec<String>,
        capacity: Option<usize>,
    ) -> broadcast::Receiver<Envelope> {
        self.inner.subscribe_filtered(prefixes, capacity)
    }
}

// Placeholder for future remote backends (NATS JetStream, Redis Streams, ZMQ relay)
// Implementations will wrap a local relay to preserve subscribe() semantics.

#[cfg(feature = "nats")]
pub async fn attach_nats_outgoing(bus: &Bus, url: &str, node_id: &str) {
    // Connect once and spawn a relay: local bus -> NATS subjects (arw.events.node.<node_id>.<Kind>)
    match async_nats::connect(url).await {
        Ok(client) => {
            // Optional outgoing filter: ARW_NATS_OUT_FILTER="prefix1,prefix2"
            let mut rx = {
                let filt = std::env::var("ARW_NATS_OUT_FILTER")
                    .ok()
                    .unwrap_or_default();
                let prefs: Vec<String> = filt
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                if prefs.is_empty() {
                    bus.subscribe()
                } else {
                    bus.subscribe_filtered(prefs, None)
                }
            };
            let node = node_id.to_string();
            tokio::spawn(async move {
                while let Ok(env) = rx.recv().await {
                    let mut env2 = env.clone();
                    if let Some(mut cap) = env2.policy.clone() {
                        if !local_capsule_allows(&cap) {
                            continue;
                        }
                        if let Some(ttl) = cap.hop_ttl.as_mut() {
                            if *ttl == 0 {
                                continue;
                            } else {
                                *ttl -= 1;
                            }
                        }
                        env2.policy = Some(cap);
                    }
                    let subj = format!("arw.events.node.{}.{}", node, env2.kind.replace(' ', "."));
                    if let Ok(bytes) = serde_json::to_vec(&env2) {
                        let _ = client.publish(subj, bytes.into()).await;
                    }
                }
            });
            tracing::info!("arw-events: relaying local events to NATS at {}", url);
        }
        Err(e) => {
            tracing::warn!("arw-events: failed to connect to NATS at {}: {}", url, e);
        }
    }
}

#[cfg(not(feature = "nats"))]
pub async fn attach_nats_outgoing(_bus: &Bus, _url: &str, _node_id: &str) {}

/// Subscribe to NATS subjects and publish into the local bus (aggregator mode).
/// Uses subject form: `arw.events.node.<node_id>.<Kind>` to avoid loops.
#[cfg(feature = "nats")]
pub async fn attach_nats_incoming(bus: &Bus, url: &str, self_node_id: &str) {
    use futures_util::StreamExt;
    let self_node = self_node_id.to_string();
    match async_nats::connect(url).await {
        Ok(client) => {
            // Subscribe to node-specific subjects
            let sub = match client.subscribe("arw.events.node.>").await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("arw-events: subscribe failed: {}", e);
                    return;
                }
            };
            let bus = bus.clone();
            tokio::spawn(async move {
                let mut sub = sub;
                while let Some(msg) = sub.next().await {
                    // Subject pattern: arw.events.node.<node>.<Kind>
                    let subj = msg.subject.clone();
                    let parts: Vec<&str> = subj.split('.').collect();
                    if parts.len() >= 5 {
                        let node = parts[3];
                        if node == self_node {
                            continue;
                        }
                    }
                    if let Ok(env) = serde_json::from_slice::<Envelope>(&msg.payload) {
                        bus.publish(&env.kind, &env.payload);
                    }
                }
            });
            tracing::info!(
                "arw-events: ingesting NATS events into local bus from {}",
                url
            );
        }
        Err(e) => {
            tracing::warn!("arw-events: failed to connect to NATS at {}: {}", url, e);
        }
    }
}

#[cfg(not(feature = "nats"))]
pub async fn attach_nats_incoming(_bus: &Bus, _url: &str, _self_node_id: &str) {}

#[cfg(feature = "nats")]
fn local_capsule_allows(cap: &arw_protocol::GatingCapsule) -> bool {
    // Minimal checks: TTL, issued_at bounds, propagate sanity
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if cap.issued_at_ms > now.saturating_add(5 * 60 * 1000) {
        return false;
    } // future-dated too far
    if let Some(p) = &cap.propagate {
        if !matches!(p.as_str(), "none" | "children" | "peers" | "all") {
            return false;
        }
    }
    true
}
