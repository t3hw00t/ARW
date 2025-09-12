#![allow(clippy::needless_return)]
pub mod app_state;
mod dyn_timeout;
mod ext;
pub mod resources;

pub use app_state::AppState;
use axum::{routing::get, Router};

/// Build an axum Router with a simple /healthz and all extra routes from ext.rs.
/// This is for tests; your runtime binary still uses its own main.rs.
pub fn build_router() -> Router<AppState> {
    let base = Router::new().route("/healthz", get(|| async { "ok" }));
    let app = base.merge(ext::extra_routes());
    app.with_state(AppState::default())
}

/// No-op symbol to force linking this crate in tests when needed.
pub fn linkme() {}
