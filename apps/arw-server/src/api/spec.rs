use super::http_utils;
use crate::responses;
use axum::body::Body;
use axum::http::{
    header::{self, HeaderMap},
    Method, StatusCode,
};
use axum::response::Response;
use axum::{extract::Path, response::IntoResponse, Json};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::SystemTime;

#[cfg(not(test))]
use utoipa::OpenApi;

fn spec_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("ARW_SPEC_DIR").unwrap_or_else(|_| "spec".into()))
}

/// OpenAPI document generated from in-code annotations.
#[utoipa::path(get, path = "/spec/openapi.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_openapi(method: Method, headers: HeaderMap) -> Response {
    let yaml = crate::openapi::ApiDoc::openapi()
        .to_yaml()
        .unwrap_or_else(|_| "openapi: 3.0.3".into())
        .into_bytes();
    respond_bytes(
        method,
        headers,
        yaml,
        None,
        "application/yaml",
        CACHE_CONTROL_SPEC,
    )
}

/// Static AsyncAPI file.
#[utoipa::path(get, path = "/spec/asyncapi.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_asyncapi(method: Method, headers: HeaderMap) -> Response {
    let path = spec_dir().join("asyncapi.yaml");
    serve_spec_file(method, headers, path, "application/yaml").await
}

/// Static MCP tools manifest.
#[utoipa::path(get, path = "/spec/mcp-tools.json", tag = "Specs", responses((status = 200, content_type = "application/json")))]
pub async fn spec_mcp(method: Method, headers: HeaderMap) -> Response {
    let path = spec_dir().join("mcp-tools.json");
    serve_spec_file(method, headers, path, "application/json").await
}

/// Generated OpenAPI from annotations (alias to /spec/openapi.yaml).
#[utoipa::path(get, path = "/spec/openapi.gen.yaml", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn spec_openapi_gen(method: Method, headers: HeaderMap) -> Response {
    spec_openapi(method, headers).await
}

/// JSON Schemas referenced by the API.
#[utoipa::path(get, path = "/spec/schemas/{file}", tag = "Specs", params(("file" = String, Path)), responses((status = 200, content_type = "application/json")))]
pub async fn spec_schema(method: Method, headers: HeaderMap, Path(file): Path<String>) -> Response {
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
    serve_spec_file(method, headers, path, "application/json").await
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
    responses::json_raw_status(
        StatusCode::OK,
        json!({"entries": entries, "schemas": schemas }),
    )
}

/// Health summary for spec artifacts (presence/size).
#[utoipa::path(get, path = "/spec/health", tag = "Specs", responses((status = 200, body = serde_json::Value)))]
pub async fn spec_health() -> impl IntoResponse {
    let base = spec_dir();
    let entries = [
        ("openapi.yaml", "application/yaml"),
        ("asyncapi.yaml", "application/yaml"),
        ("mcp-tools.json", "application/json"),
    ];
    let mut items = Vec::with_capacity(entries.len());
    for (name, content_type) in entries {
        let path = base.join(name);
        let (exists, size, modified_ms) = match tokio::fs::metadata(&path).await {
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64);
                (true, meta.len(), modified)
            }
            Err(_) => (false, 0, None),
        };
        items.push(json!({
            "name": name,
            "content_type": content_type,
            "path": format!("spec/{}", name),
            "exists": exists,
            "size": size,
            "modified_ms": modified_ms,
        }));
    }
    let schemas_dir = base.join("schemas");
    let (schemas_exists, schema_files) = if schemas_dir.exists() {
        let mut names = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&schemas_dir) {
            for entry in rd.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".json") {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names.sort();
        (true, names)
    } else {
        (false, Vec::new())
    };
    responses::json_raw_status(
        StatusCode::OK,
        json!({
            "items": items,
            "schemas": {
                "exists": schemas_exists,
                "count": schema_files.len(),
                "files": schema_files,
            }
        }),
    )
}

fn interfaces_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("ARW_INTERFACES_DIR").unwrap_or_else(|_| "interfaces".into()),
    )
}

/// Interface catalog YAML (generated).
#[utoipa::path(get, path = "/catalog/index", tag = "Specs", responses((status = 200, content_type = "application/yaml")))]
pub async fn catalog_index(method: Method, headers: HeaderMap) -> Response {
    let path = interfaces_dir().join("index.yaml");
    serve_spec_file(method, headers, path, "application/yaml").await
}

const CACHE_CONTROL_SPEC: &str = "public, max-age=300, must-revalidate";

async fn serve_spec_file(
    method: Method,
    headers: HeaderMap,
    path: std::path::PathBuf,
    content_type: &'static str,
) -> Response {
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let modified = tokio::fs::metadata(&path)
                .await
                .ok()
                .and_then(|m| m.modified().ok());
            respond_bytes(
                method,
                headers,
                bytes,
                modified,
                content_type,
                CACHE_CONTROL_SPEC,
            )
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"type":"about:blank","title":"Not Found","status":404})),
        )
            .into_response(),
    }
}

fn respond_bytes(
    method: Method,
    headers: HeaderMap,
    bytes: Vec<u8>,
    last_modified: Option<SystemTime>,
    content_type: &'static str,
    cache_control: &'static str,
) -> Response {
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let etag_hex = format!("{:x}", hasher.finalize());
    let etag = http_utils::etag_value(&etag_hex);
    let last_modified_header = last_modified.and_then(http_utils::http_date_value);

    if http_utils::if_none_match_matches(&headers, &etag_hex) {
        return http_utils::not_modified_response(
            &etag,
            last_modified_header.as_ref(),
            cache_control,
        );
    }
    if let Some(modified) = last_modified {
        if http_utils::not_modified_since(&headers, modified) {
            return http_utils::not_modified_response(
                &etag,
                last_modified_header.as_ref(),
                cache_control,
            );
        }
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::ETAG, etag.clone())
        .header(header::CACHE_CONTROL, cache_control)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, bytes.len().to_string());
    if let Some(value) = last_modified_header {
        builder = builder.header(header::LAST_MODIFIED, value);
    }

    if method == Method::HEAD {
        let mut response = builder
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty()));
        if content_type.starts_with("application/json") {
            responses::mark_envelope_bypass(&mut response);
        }
        return response;
    }

    let mut response = builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()));
    if content_type.starts_with("application/json") {
        responses::mark_envelope_bypass(&mut response);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env;
    use axum::http::{header::IF_NONE_MATCH, HeaderMap};
    use http_body_util::BodyExt;
    use tempfile::tempdir;

    #[tokio::test]
    async fn spec_asyncapi_applies_caching_headers() {
        let temp = tempdir().expect("tempdir");
        tokio::fs::create_dir_all(temp.path()).await.expect("mkdir");
        tokio::fs::write(temp.path().join("asyncapi.yaml"), b"async-spec")
            .await
            .expect("write asyncapi");

        let mut env_guard = env::guard();
        env_guard.set("ARW_SPEC_DIR", temp.path().display().to_string());

        let response = spec_asyncapi(Method::GET, HeaderMap::new()).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let etag = parts.headers.get(header::ETAG).expect("etag").clone();
        assert_eq!(
            parts
                .headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some(CACHE_CONTROL_SPEC)
        );
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/yaml")
        );
        let bytes = BodyExt::collect(body).await.expect("body").to_bytes();
        assert_eq!(&bytes[..], b"async-spec");

        // HEAD reuses metadata without body
        let head_response = spec_asyncapi(Method::HEAD, HeaderMap::new()).await;
        let (head_parts, head_body) = head_response.into_parts();
        assert_eq!(head_parts.status, StatusCode::OK);
        assert!(BodyExt::collect(head_body)
            .await
            .expect("head body")
            .to_bytes()
            .is_empty());
        assert_eq!(head_parts.headers.get(header::ETAG), Some(&etag));

        // Conditional request hits 304
        let mut headers = HeaderMap::new();
        headers.insert(IF_NONE_MATCH, etag.clone());
        let not_modified = spec_asyncapi(Method::GET, headers).await;
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn spec_openapi_respects_if_none_match() {
        let response = spec_openapi(Method::GET, HeaderMap::new()).await;
        let (parts, body) = response.into_parts();
        assert_eq!(parts.status, StatusCode::OK);
        let etag = parts.headers.get(header::ETAG).expect("etag").clone();
        assert_eq!(
            parts
                .headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some(CACHE_CONTROL_SPEC)
        );
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/yaml")
        );
        // Body is non-empty YAML
        assert!(!BodyExt::collect(body)
            .await
            .expect("body")
            .to_bytes()
            .is_empty());

        let mut headers = HeaderMap::new();
        headers.insert(IF_NONE_MATCH, etag);
        let resume = spec_openapi(Method::GET, headers).await;
        assert_eq!(resume.status(), StatusCode::NOT_MODIFIED);
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
