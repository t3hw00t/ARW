use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use once_cell::sync::OnceCell;

// Global handle for dynamic HTTP timeout seconds. Shared across the crate
// so ext::governor_hints_set can update it at runtime and middleware reads it per-request.
static TIMEOUT_SECS: OnceCell<Arc<AtomicU64>> = OnceCell::new();

fn init_default_timeout_secs() -> u64 {
    std::env::var("ARW_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
}

pub fn global_timeout_handle() -> Arc<AtomicU64> {
    TIMEOUT_SECS
        .get_or_init(|| Arc::new(AtomicU64::new(init_default_timeout_secs())))
        .clone()
}

pub fn set_global_timeout_secs(v: u64) {
    global_timeout_handle().store(v.max(1), Ordering::Relaxed);
}

pub fn current_http_timeout_secs() -> u64 {
    global_timeout_handle().load(Ordering::Relaxed)
}

// Axum middleware version: apply timeout per request; returns a Response on timeout or error.
#[allow(dead_code)]
pub async fn dyn_timeout_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    let secs = current_http_timeout_secs().max(1);
    let dur = Duration::from_secs(secs);
    match tokio::time::timeout(dur, next.run(req)).await {
        Ok(resp) => resp,
        Err(_elapsed) => {
            use axum::http::StatusCode;
            (StatusCode::GATEWAY_TIMEOUT, "request timeout").into_response()
        }
    }
}
