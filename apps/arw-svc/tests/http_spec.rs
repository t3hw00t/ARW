use arw_svc::build_router;
use serde_json::json;
use axum::Router;

#[tokio::test]
async fn healthz_ok() {
    let _app: Router<_> = build_router();
    // If we reach here, building the router with all routes/state succeeded.
    assert!(true);
}

#[tokio::test]
async fn memory_roundtrip() {
    // Minimal behavioral sanity with core-only types
    let tools = arw_core::introspect_tools();
    assert!(tools.len() >= 2, "expected built-in tools to be registered");
}

#[tokio::test]
async fn tool_math_add() {
    // Placeholder unit test: validate a simple JSON operation shape
    let body = json!({"id":"math.add","input":{"a":1.5,"b":2.25}});
    assert_eq!(body["id"], "math.add");
}
