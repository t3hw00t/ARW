use axum::{body::Body, http::{Request, StatusCode}, Router};
use http_body_util::BodyExt as _;
use serde_json::{json, Value};
use tower::ServiceExt; // for Router::oneshot

#[tokio::test]
async fn memory_apply_respects_limit() {
    // Isolate state dir to a temp-ish folder in repo
    std::env::set_var("ARW_STATE_DIR", "target/test-state-mem");

    let app: Router<arw_svc::AppState> = arw_svc::build_router();

    // Set limit = 2
    let req = Request::builder()
        .method("POST")
        .uri("/memory/limit")
        .header("content-type", "application/json")
        .body(Body::from(json!({"limit":2}).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Apply 3 entries into episodic lane
    for i in 0..3 {
        let req = Request::builder()
            .method("POST")
            .uri("/memory/apply")
            .header("content-type", "application/json")
            .body(Body::from(json!({"kind":"episodic","value": {"n": i}}).to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    // Get memory snapshot and assert lane length == 2
    let req = Request::builder().method("GET").uri("/memory").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let episodic_len = v.get("episodic").and_then(|a| a.as_array()).map(|a| a.len()).unwrap_or(0);
    assert_eq!(episodic_len, 2, "ring buffer should cap to limit");
}
