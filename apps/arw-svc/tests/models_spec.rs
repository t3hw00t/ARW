use axum::{body::Body, http::{Request, StatusCode}, Router};
use http_body_util::BodyExt as _;
use serde_json::{json, Value};
use tower::ServiceExt; // for Router::oneshot

#[tokio::test]
async fn models_list_and_set_default() {
    std::env::set_var("ARW_STATE_DIR", "target/test-state-models");
    let app: Router<arw_svc::AppState> = arw_svc::build_router();

    // List models
    let req = Request::builder().method("GET").uri("/models").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let arr: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(arr.is_array());

    // Set default id to a dummy
    let req = Request::builder()
        .method("POST").uri("/models/default")
        .header("content-type", "application/json")
        .body(Body::from(json!({"id":"demo"}).to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read back default
    let req = Request::builder().method("GET").uri("/models/default").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v.get("default").and_then(|s| s.as_str()), Some("demo"));
}
