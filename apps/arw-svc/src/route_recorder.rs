#![allow(dead_code)]
use std::sync::OnceLock;

use axum::{routing, Router};

use crate::AppState;

static ROUTES: OnceLock<std::sync::RwLock<Vec<(String, String)>>> = OnceLock::new();

fn store() -> &'static std::sync::RwLock<Vec<(String, String)>> {
    ROUTES.get_or_init(|| std::sync::RwLock::new(Vec::new()))
}

fn record(method: &str, path: &str) {
    let mut g = store().write().unwrap();
    g.push((method.to_string(), path.to_string()));
}

pub fn note(method: &str, path: &str) {
    record(method, path);
}

pub fn snapshot() -> Vec<(String, String)> {
    let mut v = store().read().unwrap().clone();
    v.sort();
    v.dedup();
    v
}

pub fn route_get<H, T>(router: Router<AppState>, path: &str, handler: H) -> Router<AppState>
where
    H: axum::handler::Handler<T, AppState>,
    T: 'static,
{
    record("GET", path);
    router.route(path, routing::get(handler))
}

pub fn route_post<H, T>(router: Router<AppState>, path: &str, handler: H) -> Router<AppState>
where
    H: axum::handler::Handler<T, AppState>,
    T: 'static,
{
    record("POST", path);
    router.route(path, routing::post(handler))
}

pub fn route_head<H, T>(router: Router<AppState>, path: &str, handler: H) -> Router<AppState>
where
    H: axum::handler::Handler<T, AppState>,
    T: 'static,
{
    record("HEAD", path);
    router.route(path, routing::head(handler))
}

// MethodRouter helpers to be used inline in `.route(path, ...)` chains
pub fn get_rec<S, H, T>(path: &str, handler: H) -> routing::MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    H: axum::handler::Handler<T, S>,
    T: 'static,
{
    record("GET", path);
    routing::get(handler)
}

pub fn post_rec<S, H, T>(path: &str, handler: H) -> routing::MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    H: axum::handler::Handler<T, S>,
    T: 'static,
{
    record("POST", path);
    routing::post(handler)
}

pub fn head_rec<S, H, T>(path: &str, handler: H) -> routing::MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
    H: axum::handler::Handler<T, S>,
    T: 'static,
{
    record("HEAD", path);
    routing::head(handler)
}
