use std::sync::Arc;
use crate::resources::Resources;

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
        let queue: Arc<dyn arw_core::orchestrator::Queue> =
            Arc::new(arw_core::orchestrator::LocalQueue::new());
        Self { bus, stop_tx: None, queue, resources: Resources::new() }
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
        Self { bus_cap: 256, bus_replay: 256, queue: None, stop_tx: None }
    }
    pub fn bus(mut self, cap: usize, replay: usize) -> Self { self.bus_cap = cap; self.bus_replay = replay; self }
    pub fn queue(mut self, q: Arc<dyn arw_core::orchestrator::Queue>) -> Self { self.queue = Some(q); self }
    pub fn stop_tx(mut self, tx: tokio::sync::broadcast::Sender<()>) -> Self { self.stop_tx = Some(tx); self }
    pub fn build(self) -> AppState {
        let bus = arw_events::Bus::new_with_replay(self.bus_cap, self.bus_replay);
        let queue = self.queue.unwrap_or_else(|| Arc::new(arw_core::orchestrator::LocalQueue::new()));
        AppState { bus, stop_tx: self.stop_tx, queue, resources: Resources::new() }
    }
}
