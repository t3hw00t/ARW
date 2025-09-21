use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{admin_ok, util, AppState};
use arw_topics as topics;

#[derive(Deserialize, ToSchema)]
pub(crate) struct ConnectorManifest {
    #[serde(default)]
    pub id: Option<String>,
    pub kind: String,
    pub provider: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub meta: Value,
}

fn connectors_dir() -> std::path::PathBuf {
    util::state_dir().join("connectors")
}

/// List registered connector manifests (secrets elided).
#[utoipa::path(get, path = "/state/connectors", tag = "Connectors", responses((status = 200, body = serde_json::Value)))]
pub async fn state_connectors() -> impl IntoResponse {
    use tokio::fs as afs;
    let dir = connectors_dir();
    let mut items: Vec<Value> = Vec::new();
    if let Ok(mut rd) = afs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                if name.ends_with(".json") {
                    if let Ok(bytes) = afs::read(ent.path()).await {
                        if let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) {
                            if let Some(obj) = v.as_object_mut() {
                                if obj.contains_key("token") {
                                    obj.remove("token");
                                }
                                if obj.contains_key("refresh_token") {
                                    obj.remove("refresh_token");
                                }
                            }
                            items.push(v);
                        }
                    }
                }
            }
        }
    }
    Json(json!({"items": items}))
}

/// Register a connector manifest (admin).
#[utoipa::path(post, path = "/connectors/register", tag = "Connectors", request_body = ConnectorManifest, responses((status = 201, body = serde_json::Value), (status = 401)))]
pub async fn connector_register(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(mut manifest): Json<ConnectorManifest>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let id = manifest
        .id
        .take()
        .unwrap_or_else(|| format!("{}-{}", manifest.provider, uuid::Uuid::new_v4()));
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), json!(id));
    obj.insert("kind".into(), json!(manifest.kind));
    obj.insert("provider".into(), json!(manifest.provider));
    obj.insert("scopes".into(), json!(manifest.scopes));
    obj.insert("meta".into(), manifest.meta);
    let dir = connectors_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let path = dir.join(format!(
        "{}.json",
        &obj.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("connector")
    ));
    if let Err(e) = tokio::fs::write(
        &path,
        serde_json::to_vec(&Value::Object(obj.clone())).unwrap_or_default(),
    )
    .await
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        );
    }
    // Emit event (no secrets)
    state.bus.publish(
        topics::TOPIC_CONNECTORS_REGISTERED,
        &json!({"id": obj["id"].clone(), "provider": obj["provider"].clone()}),
    );
    (
        axum::http::StatusCode::CREATED,
        Json(json!({"id": obj["id"].clone(), "ok": true})),
    )
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct ConnectorTokenReq {
    pub id: String,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}
/// Set/update connector tokens (admin).
#[utoipa::path(post, path = "/connectors/token", tag = "Connectors", request_body = ConnectorTokenReq, responses((status = 200, body = serde_json::Value), (status = 401)))]
pub async fn connector_token_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<ConnectorTokenReq>,
) -> impl IntoResponse {
    if !admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        );
    }
    let path = connectors_dir().join(format!("{}.json", req.id));
    let mut base = serde_json::Map::new();
    if let Ok(bytes) = tokio::fs::read(&path).await {
        base = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
    }
    if let Some(tok) = req.token {
        base.insert("token".into(), json!(tok));
    }
    if let Some(rtok) = req.refresh_token {
        base.insert("refresh_token".into(), json!(rtok));
    }
    if let Some(exp) = req.expires_at {
        base.insert("expires_at".into(), json!(exp));
    }
    if !base.contains_key("id") {
        base.insert("id".into(), json!(req.id));
    }
    if let Err(e) = tokio::fs::write(
        &path,
        serde_json::to_vec(&Value::Object(base.clone())).unwrap_or_default(),
    )
    .await
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        );
    }
    state.bus.publish(
        topics::TOPIC_CONNECTORS_TOKEN_UPDATED,
        &json!({"id": req.id}),
    );
    (axum::http::StatusCode::OK, Json(json!({"ok": true})))
}
