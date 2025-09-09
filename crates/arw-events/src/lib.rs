use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;

/// Minimal event envelope (RFC3339 time).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Envelope {
    pub time: String,
    pub kind: String,
    pub payload: Value,
}

/// Pluggable event bus API. For now subscribe returns a local channel receiver
/// to keep existing callers working; remote implementations may fan-in to a
/// local relay.
pub trait EventBus: Send + Sync + Clone + 'static {
    fn subscribe(&self) -> broadcast::Receiver<Envelope>;
    fn publish<T: Serialize>(&self, kind: &str, payload: &T);
}

/// Local in-process bus backed by tokio broadcast channels.
#[derive(Clone)]
pub struct LocalBus {
    tx: broadcast::Sender<Envelope>,
}

impl LocalBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
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
        let _ = self.tx.send(Envelope {
            time: now,
            kind: kind.to_string(),
            payload: val,
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
    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.inner.subscribe()
    }
    pub fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        self.inner.publish(kind, payload)
    }
}

// Placeholder for future remote backends (NATS JetStream, Redis Streams, ZMQ relay)
// Implementations will wrap a local relay to preserve subscribe() semantics.

#[cfg(feature = "nats")]
pub async fn attach_nats_outgoing(bus: &Bus, url: &str) {
    // Connect once and spawn a relay: local bus -> NATS subjects (arw.events.<Kind>)
    match async_nats::connect(url).await {
        Ok(client) => {
            let mut rx = bus.subscribe();
            tokio::spawn(async move {
                while let Ok(env) = rx.recv().await {
                    let subj = format!("arw.events.{}", env.kind.replace(' ', "."));
                    if let Ok(bytes) = serde_json::to_vec(&env) {
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
pub async fn attach_nats_outgoing(_bus: &Bus, _url: &str) {
    // no-op when not compiled with nats feature
}

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
                        if node == self_node { continue; }
                    }
                    if let Ok(env) = serde_json::from_slice::<Envelope>(&msg.payload) {
                        bus.publish(&env.kind, &env.payload);
                    }
                }
            });
            tracing::info!("arw-events: ingesting NATS events into local bus from {}", url);
        }
        Err(e) => {
            tracing::warn!("arw-events: failed to connect to NATS at {}: {}", url, e);
        }
    }
}

#[cfg(not(feature = "nats"))]
pub async fn attach_nats_incoming(_bus: &Bus, _url: &str, _self_node_id: &str) {}
