use axum::http::{header, StatusCode};
use axum::Json;
use axum::{extract::Path, response::IntoResponse};
use serde_json::json;
use utoipa::OpenApi;

fn spec_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ARW_SPEC_DIR").unwrap_or_else(|_| "spec".into()))
}

/// Static OpenAPI (curated) file.
#[utoipa::path(get, path = "/spec/openapi.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_openapi() -> impl IntoResponse {
    let path = spec_dir().join("openapi.yaml");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/yaml")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

/// Static AsyncAPI file.
#[utoipa::path(get, path = "/spec/asyncapi.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_asyncapi() -> impl IntoResponse {
    let path = spec_dir().join("asyncapi.yaml");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/yaml")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

/// Static MCP tools manifest.
#[utoipa::path(get, path = "/spec/mcp-tools.json", tag = "Specs", responses((status = 200, content_type = "application/json")))]
pub async fn spec_mcp() -> impl IntoResponse {
    let path = spec_dir().join("mcp-tools.json");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

/// Generated OpenAPI from annotations (experimental).
#[utoipa::path(get, path = "/spec/openapi.gen.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_openapi_gen() -> impl IntoResponse {
    let yaml = crate::openapi::ApiDoc::openapi()
        .to_yaml()
        .unwrap_or_else(|_| "openapi: 3.0.3".into())
        .into_bytes();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/yaml")],
        yaml,
    )
        .into_response()
}

/// JSON Schemas referenced by the API.
#[utoipa::path(get, path = "/spec/schemas/{file}", tag = "Specs", params(("file" = String, Path)), responses((status = 200, content_type = "application/json")))]
pub async fn spec_schema(Path(file): Path<String>) -> impl IntoResponse {
    // Basic guard: only allow .json under spec/schemas
    if !file.ends_with(".json") || file.contains("..") || file.contains('/') || file.contains('\\')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"type":"about:blank","title":"Bad Request","status":400})),
        )
            .into_response();
    }
    let path = spec_dir().join("schemas").join(&file);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

/// Index of published specs and schemas.
#[utoipa::path(get, path = "/spec/index.json", tag = "Specs", responses((status = 200, body = serde_json::Value)))]
pub async fn spec_index() -> impl IntoResponse {
    let mut entries = vec![];
    let base = spec_dir();
    let check = |p: &str| base.join(p).exists();
    if check("openapi.yaml") {
        entries.push(json!({"path":"/spec/openapi.yaml","content_type":"application/yaml"}));
    }
    if check("asyncapi.yaml") {
        entries.push(json!({"path":"/spec/asyncapi.yaml","content_type":"application/yaml"}));
    }
    if check("mcp-tools.json") {
        entries.push(json!({"path":"/spec/mcp-tools.json","content_type":"application/json"}));
    }
    // Schemas listing
    let schemas_dir = base.join("schemas");
    let mut schemas: Vec<String> = vec![];
    if schemas_dir.exists() {
        if let Ok(rd) = std::fs::read_dir(&schemas_dir) {
            for ent in rd.flatten() {
                if let Some(name) = ent.file_name().to_str() {
                    if name.ends_with(".json") {
                        schemas.push(name.to_string());
                    }
                }
            }
        }
    }
    schemas.sort();
    (
        StatusCode::OK,
        Json(json!({"entries": entries, "schemas": schemas })),
    )
        .into_response()
}

fn interfaces_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("ARW_INTERFACES_DIR").unwrap_or_else(|_| "interfaces".into()),
    )
}

/// Interface catalog YAML (generated).
#[utoipa::path(get, path = "/catalog/index", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn catalog_index() -> impl IntoResponse {
    let path = interfaces_dir().join("index.yaml");
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/yaml")],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

/// Catalog/spec artifacts presence/size report.
#[utoipa::path(get, path = "/catalog/health", tag = "Specs", responses((status = 200, body = serde_json::Value)))]
pub async fn catalog_health() -> impl IntoResponse {
    // Report presence/size of spec artifacts
    let base = spec_dir();
    let mut items = vec![];
    let entries = [
        ("openapi.yaml", "application/yaml"),
        ("asyncapi.yaml", "application/yaml"),
        ("mcp-tools.json", "application/json"),
    ];
    for (name, ct) in entries {
        let p = base.join(name);
        let (exists, size) = match tokio::fs::metadata(&p).await {
            Ok(m) => (true, m.len()),
            Err(_) => (false, 0),
        };
        items.push(json!({
            "name": name,
            "content_type": ct,
            "path": format!("spec/{}", name),
            "exists": exists,
            "size": size
        }));
    }
    (StatusCode::OK, Json(json!({"items": items}))).into_response()
}
