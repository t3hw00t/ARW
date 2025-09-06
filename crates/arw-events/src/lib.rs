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

/// A simple broadcast bus for JSON-serializable events.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Envelope>,
}

impl Bus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Envelope> {
        self.tx.subscribe()
    }

    pub fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let val =
            serde_json::to_value(payload).unwrap_or_else(|_| serde_json::json!({"_ser":"error"}));
        let _ = self.tx.send(Envelope {
            time: now,
            kind: kind.to_string(),
            payload: val,
        });
    }
}
