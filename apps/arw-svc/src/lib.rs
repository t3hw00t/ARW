#![allow(clippy::needless_return)]
mod ext;

use axum::{routing::get, Router};

/// Minimal no-op event bus for tests (replaces real bus in binary target)
#[derive(Clone, Default)]
pub struct BusStub;
impl BusStub {
    pub fn publish<T: serde::Serialize>(&self, _kind: &str, _payload: &T) {}
}
// Safe: BusStub holds no data and performs no synchronization.
unsafe impl Send for BusStub {}
unsafe impl Sync for BusStub {}

/// Public state type used by ext.rs (for the library target / tests)
#[derive(Clone, Default)]
pub struct AppState {
    pub bus: BusStub,
}

/// Build an axum Router with a simple /healthz and all extra routes from ext.rs.
/// This is for tests; your runtime binary still uses its own main.rs.
pub fn build_router() -> Router<AppState> {
    let base = Router::new().route("/healthz", get(|| async { "ok" }));
    let app  = base.merge(ext::extra_routes());
    app.with_state(AppState::default())
}

