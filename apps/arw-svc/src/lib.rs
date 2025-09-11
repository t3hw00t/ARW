#![allow(clippy::needless_return)]
mod ext;

use axum::{routing::get, Router};
use std::sync::Arc;

/// Minimal no-op event bus for tests (replaces real bus in binary target)
#[derive(Clone, Default)]
pub struct BusStub;

#[derive(serde::Serialize)]
pub struct BusStats {
    pub published: u64,
    pub delivered: u64,
    pub lagged: u64,
    pub no_receivers: u64,
    pub receivers: usize,
}

impl BusStub {
    pub fn publish<T: serde::Serialize>(&self, _kind: &str, _payload: &T) {}
    pub fn stats(&self) -> BusStats {
        BusStats {
            published: 0,
            delivered: 0,
            lagged: 0,
            no_receivers: 0,
            receivers: 0,
        }
    }
}
// Safe: BusStub holds no data and performs no synchronization.
unsafe impl Send for BusStub {}
unsafe impl Sync for BusStub {}

/// Public state type used by ext.rs (for the library target / tests)
#[derive(Clone)]
pub struct AppState {
    pub bus: BusStub,
    pub queue: Arc<dyn arw_core::orchestrator::Queue>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            bus: BusStub,
            queue: Arc::new(arw_core::orchestrator::LocalQueue::new()),
        }
    }
}

/// Build an axum Router with a simple /healthz and all extra routes from ext.rs.
/// This is for tests; your runtime binary still uses its own main.rs.
pub fn build_router() -> Router<AppState> {
    let base = Router::new().route("/healthz", get(|| async { "ok" }));
    let app = base.merge(ext::extra_routes());
    app.with_state(AppState::default())
}
