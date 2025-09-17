use crate::resources::Resources;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub bus: arw_events::Bus,
    pub stop_tx: Option<tokio::sync::broadcast::Sender<()>>,
    pub queue: Arc<dyn arw_core::orchestrator::Queue>,
    pub resources: Resources,
}

impl Default for AppState {
    fn default() -> Self {
        let bus = arw_events::Bus::new_with_replay(128, 128);
        let lease_ttl = std::env::var("ARW_ORCH_LEASE_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30_000);
        let queue: Arc<dyn arw_core::orchestrator::Queue> = Arc::new(
            arw_core::orchestrator::LocalQueue::with_lease_ttl(lease_ttl),
        );
        Self {
            bus,
            stop_tx: None,
            queue,
            resources: Resources::new(),
        }
    }
}

pub struct AppStateBuilder {
    bus_cap: usize,
    bus_replay: usize,
    queue: Option<Arc<dyn arw_core::orchestrator::Queue>>,
    stop_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl AppStateBuilder {
    pub fn new() -> Self {
        Self {
            bus_cap: 256,
            bus_replay: 256,
            queue: None,
            stop_tx: None,
        }
    }
    pub fn bus(mut self, cap: usize, replay: usize) -> Self {
        self.bus_cap = cap;
        self.bus_replay = replay;
        self
    }
    pub fn queue(mut self, q: Arc<dyn arw_core::orchestrator::Queue>) -> Self {
        self.queue = Some(q);
        self
    }
    pub fn stop_tx(mut self, tx: tokio::sync::broadcast::Sender<()>) -> Self {
        self.stop_tx = Some(tx);
        self
    }
    pub fn build(self) -> AppState {
        let bus = arw_events::Bus::new_with_replay(self.bus_cap, self.bus_replay);
        let queue = self.queue.unwrap_or_else(|| {
            let lease_ttl = std::env::var("ARW_ORCH_LEASE_MS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(30_000);
            Arc::new(arw_core::orchestrator::LocalQueue::with_lease_ttl(
                lease_ttl,
            ))
        });
        AppState {
            bus,
            stop_tx: self.stop_tx,
            queue,
            resources: Resources::new(),
        }
    }
}

impl Default for AppStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}
