use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{extract::State, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as JsonValue};
use utoipa::ToSchema;

use crate::{egress_policy, egress_proxy, AppState};
use arw_topics as topics;
use jsonschema::{Draft, JSONSchema};

#[derive(Serialize, ToSchema)]
pub(crate) struct EgressSettings {
    pub posture: Option<String>,
    pub allowlist: Vec<String>,
    pub block_ip_literals: bool,
    pub dns_guard_enable: bool,
    pub proxy_enable: bool,
    pub proxy_port: u16,
    pub ledger_enable: bool,
    pub multi_label_suffixes: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<NetworkScopeView>,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct EgressSettingsPatch {
    #[serde(default)]
    pub posture: Option<String>,
    #[serde(default)]
    pub allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub block_ip_literals: Option<bool>,
    #[serde(default)]
    pub dns_guard_enable: Option<bool>,
    #[serde(default)]
    pub proxy_enable: Option<bool>,
    #[serde(default)]
    pub proxy_port: Option<u16>,
    #[serde(default)]
    pub ledger_enable: Option<bool>,
    #[serde(default)]
    pub multi_label_suffixes: Option<Vec<String>>,
    #[serde(default)]
    pub scopes: Option<Vec<NetworkScopePatch>>,
}

#[derive(Serialize, ToSchema, Clone, Debug, Default)]
pub(crate) struct NetworkScopeView {
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub lease_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub expired: bool,
}

#[derive(Deserialize, ToSchema, Clone, Debug, Default)]
pub(crate) struct NetworkScopePatch {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub cidrs: Vec<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub lease_capabilities: Vec<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

fn bool_flag(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn scope_configs_to_view(scopes: &[egress_policy::ScopeConfig]) -> Vec<NetworkScopeView> {
    let now = Utc::now();
    scopes
        .iter()
        .map(|scope| NetworkScopeView {
            id: scope.id.clone(),
            description: scope.description.clone(),
            hosts: scope.hosts.clone(),
            cidrs: scope.cidrs.clone(),
            ports: scope.ports.clone(),
            protocols: scope.protocols.clone(),
            lease_capabilities: scope.lease_capabilities.clone(),
            expires_at: scope.expires_at.clone(),
            expired: scope
                .expires_at_ts
                .as_ref()
                .map(|ts| ts.clone() <= now)
                .unwrap_or(false),
        })
        .collect()
}

fn scope_config_to_json(scope: &egress_policy::ScopeConfig) -> JsonValue {
    let mut map = Map::new();
    if let Some(id) = scope.id.as_ref() {
        if !id.is_empty() {
            map.insert("id".into(), json!(id));
        }
    }
    if let Some(desc) = scope.description.as_ref() {
        if !desc.is_empty() {
            map.insert("description".into(), json!(desc));
        }
    }
    if !scope.hosts.is_empty() {
        map.insert("hosts".into(), json!(scope.hosts.clone()));
    }
    if !scope.cidrs.is_empty() {
        map.insert("cidrs".into(), json!(scope.cidrs.clone()));
    }
    if !scope.ports.is_empty() {
        map.insert("ports".into(), json!(scope.ports.clone()));
    }
    if !scope.protocols.is_empty() {
        map.insert("protocols".into(), json!(scope.protocols.clone()));
    }
    if !scope.lease_capabilities.is_empty() {
        map.insert(
            "lease_capabilities".into(),
            json!(scope.lease_capabilities.clone()),
        );
    }
    if let Some(expires) = scope.expires_at.as_ref() {
        if !expires.is_empty() {
            map.insert("expires_at".into(), json!(expires));
        }
    }
    JsonValue::Object(map)
}

pub(crate) async fn current_settings(state: &AppState) -> EgressSettings {
    let policy = egress_policy::resolve_policy(state).await;
    let posture = Some(policy.posture.as_str().to_string());
    let cfg = state.config_state().lock().await.clone();
    let mut allowlist = egress_policy::config_allowlist(&cfg);
    allowlist.extend(egress_policy::env_allowlist());
    allowlist.sort();
    allowlist.dedup();
    let mut suffixes: Vec<String> = egress_policy::config_multi_label_suffixes(&cfg)
        .into_iter()
        .map(|parts| parts.join("."))
        .collect();
    suffixes.extend(
        egress_policy::env_multi_label_suffixes()
            .into_iter()
            .map(|parts| parts.join(".")),
    );
    suffixes.sort();
    suffixes.dedup();
    let scope_configs = egress_policy::config_scopes(&cfg);
    EgressSettings {
        posture,
        allowlist,
        block_ip_literals: policy.block_ip_literals,
        dns_guard_enable: policy.dns_guard_enabled,
        proxy_enable: policy.proxy_enabled,
        proxy_port: std::env::var("ARW_EGRESS_PROXY_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(9080),
        ledger_enable: policy.ledger_enabled,
        multi_label_suffixes: suffixes,
        scopes: scope_configs_to_view(&scope_configs),
    }
}

async fn compose_egress_payload(state: &AppState) -> serde_json::Value {
    let egress = current_settings(state).await;
    let posture_value = egress.posture.clone().unwrap_or_else(|| "standard".into());
    let posture_enum = egress_policy::Posture::from_str(&posture_value);
    let effective_posture = posture_enum.effective();
    let defaults = egress_policy::posture_defaults(effective_posture);
    let recommended = json!({
        "block_ip_literals": defaults.block_ip_literals,
        "dns_guard_enable": defaults.dns_guard_enabled,
        "proxy_enable": defaults.proxy_enabled,
        "ledger_enable": defaults.ledger_enabled,
    });
    let capsules_snapshot = state.capsules().snapshot().await;
    let capsule_count = capsules_snapshot
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let leases_summary = if state.kernel_enabled() {
        match state.kernel().list_leases_async(200).await {
            Ok(items) => {
                let total = items.len();
                let net = items
                    .iter()
                    .filter(|lease| {
                        lease
                            .as_object()
                            .and_then(|obj| obj.get("capability"))
                            .and_then(|v| v.as_str())
                            .map(|cap| cap.starts_with("net"))
                            .unwrap_or(false)
                    })
                    .count();
                json!({"total": total, "net": net, "items": items})
            }
            Err(err) => json!({"error": err.to_string()}),
        }
    } else {
        json!({"enabled": false})
    };

    json!({
        "egress": egress,
        "recommended": recommended,
        "capsules": {
            "active": capsule_count,
            "snapshot": capsules_snapshot,
        },
        "leases": leases_summary,
    })
}

/// Effective egress settings snapshot.
#[utoipa::path(
    get,
    path = "/state/egress/settings",
    tag = "Egress",
    responses((status = 200, description = "Egress settings", body = serde_json::Value))
)]
pub async fn state_egress_settings(State(state): State<AppState>) -> impl IntoResponse {
    Json(compose_egress_payload(&state).await)
}

/// Update egress settings (admin token required).
#[utoipa::path(
    post,
    path = "/egress/settings",
    tag = "Egress",
    request_body = EgressSettingsPatch,
    responses(
        (status = 200, description = "Updated settings", body = serde_json::Value),
        (status = 401, description = "Unauthorized"),
        (status = 400, description = "Validation error")
    )
)]
pub async fn egress_settings_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(patch): Json<EgressSettingsPatch>,
) -> impl IntoResponse {
    if !crate::admin_ok(&headers) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
        )
            .into_response();
    }
    // persist to config_state under "egress" with schema validation
    let schema_path = "spec/schemas/egress_settings.json";
    let schema_json = match std::fs::read(schema_path).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        Some(v) => v,
        None => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail":"missing egress schema"}))).into_response(),
    };
    let compiled = match JSONSchema::options().with_draft(Draft::Draft7).compile(&schema_json) {
        Ok(c) => c,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"type":"about:blank","title":"Error","status":500, "detail": format!("invalid schema: {}", e)}))).into_response(),
    };
    // Merge patch into config_state.egress
    let mut cfg = state.config_state().lock().await.clone();
    if !cfg.is_object() {
        cfg = json!({});
    }
    let mut egress = cfg.get("egress").cloned().unwrap_or_else(|| json!({}));
    if !egress.is_object() {
        egress = json!({});
    }
    let mut posture_value = if let Some(posture) = patch.posture.clone() {
        std::env::set_var("ARW_NET_POSTURE", &posture);
        posture
    } else {
        std::env::var("ARW_NET_POSTURE")
            .ok()
            .or_else(|| std::env::var("ARW_SECURITY_POSTURE").ok())
            .unwrap_or_else(|| "standard".into())
    };

    let posture_changed = patch.posture.is_some();
    if let Some(posture) = patch.posture.as_ref() {
        egress["posture"] = json!(posture);
        posture_value = posture.clone();
    }

    if let Some(list) = patch.allowlist.as_ref() {
        let entries: Vec<String> = list
            .iter()
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|s| s.to_string())
            .collect();
        std::env::set_var("ARW_NET_ALLOWLIST", entries.join(","));
        egress["allowlist"] = json!(entries);
    }

    if let Some(suffixes) = patch.multi_label_suffixes.as_ref() {
        let mut normalized: Vec<String> = Vec::new();
        let mut parsed_parts: Vec<Vec<String>> = Vec::new();
        let mut invalid: Vec<String> = Vec::new();
        for entry in suffixes {
            if let Some(parts) = egress_policy::parse_multi_label_suffix(entry) {
                let joined = parts.join(".");
                normalized.push(joined);
                parsed_parts.push(parts);
            } else {
                invalid.push(entry.clone());
            }
        }
        if !invalid.is_empty() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "type": "about:blank",
                    "title": "Bad Request",
                    "status": 400,
                    "detail": "invalid multi_label_suffixes entries",
                    "invalid": invalid
                })),
            )
                .into_response();
        }
        egress["multi_label_suffixes"] = json!(normalized);
        egress_policy::set_configured_multi_label_suffixes(parsed_parts);
    }

    if let Some(scopes_patch) = patch.scopes.as_ref() {
        let mut raw_scopes: Vec<JsonValue> = Vec::new();
        for scope in scopes_patch {
            let mut map = Map::new();
            if let Some(id) = scope.id.as_ref().and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }) {
                map.insert("id".into(), json!(id));
            }
            if let Some(desc) = scope.description.as_ref().and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }) {
                map.insert("description".into(), json!(desc));
            }
            let hosts: Vec<String> = scope
                .hosts
                .iter()
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty())
                .collect();
            if !hosts.is_empty() {
                map.insert("hosts".into(), json!(hosts));
            }
            let cidrs: Vec<String> = scope
                .cidrs
                .iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            if !cidrs.is_empty() {
                map.insert("cidrs".into(), json!(cidrs));
            }
            let mut ports: Vec<u16> = scope
                .ports
                .iter()
                .copied()
                .filter(|p| (1..=65535).contains(p))
                .collect();
            ports.sort_unstable();
            ports.dedup();
            if !ports.is_empty() {
                map.insert("ports".into(), json!(ports));
            }
            let mut protocols: Vec<String> = scope
                .protocols
                .iter()
                .map(|p| p.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect();
            protocols.sort();
            protocols.dedup();
            if !protocols.is_empty() {
                map.insert("protocols".into(), json!(protocols));
            }
            let mut lease_caps: Vec<String> = scope
                .lease_capabilities
                .iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            lease_caps.sort();
            lease_caps.dedup();
            if !lease_caps.is_empty() {
                map.insert("lease_capabilities".into(), json!(lease_caps));
            }
            if let Some(raw) = scope.expires_at.as_ref().and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }) {
                map.insert("expires_at".into(), json!(raw));
            }
            raw_scopes.push(JsonValue::Object(map));
        }

        let sanitized_configs = egress_policy::config_scopes(&json!({
            "egress": {
                "scopes": raw_scopes,
            }
        }));
        let sanitized_values: Vec<JsonValue> =
            sanitized_configs.iter().map(scope_config_to_json).collect();
        egress["scopes"] = JsonValue::Array(sanitized_values);
    }

    if let Some(port) = patch.proxy_port {
        std::env::set_var("ARW_EGRESS_PROXY_PORT", format!("{}", port));
        egress["proxy_port"] = json!(port);
    }

    let posture_enum = egress_policy::Posture::from_str(&posture_value);
    let effective_posture = posture_enum.effective();
    let defaults = egress_policy::posture_defaults(effective_posture);

    let mut block_final = patch.block_ip_literals;
    if block_final.is_none() && posture_changed {
        block_final = Some(defaults.block_ip_literals);
    }
    if let Some(value) = block_final {
        std::env::set_var("ARW_EGRESS_BLOCK_IP_LITERALS", bool_flag(value));
        egress["block_ip_literals"] = json!(value);
    }

    let mut dns_final = patch.dns_guard_enable;
    if dns_final.is_none() && posture_changed {
        dns_final = Some(defaults.dns_guard_enabled);
    }
    if let Some(value) = dns_final {
        std::env::set_var("ARW_DNS_GUARD_ENABLE", bool_flag(value));
        egress["dns_guard_enable"] = json!(value);
    }

    let mut proxy_final = patch.proxy_enable;
    if proxy_final.is_none() && posture_changed {
        proxy_final = Some(defaults.proxy_enabled);
    }
    if let Some(value) = proxy_final {
        std::env::set_var("ARW_EGRESS_PROXY_ENABLE", bool_flag(value));
        egress["proxy_enable"] = json!(value);
    }

    let mut ledger_final = patch.ledger_enable;
    if ledger_final.is_none() && posture_changed {
        ledger_final = Some(defaults.ledger_enabled);
    }
    if let Some(value) = ledger_final {
        std::env::set_var("ARW_EGRESS_LEDGER_ENABLE", bool_flag(value));
        egress["ledger_enable"] = json!(value);
    }

    // Validate the sub-tree against the schema
    if let Err(errors) = compiled.validate(&egress) {
        let errs: Vec<serde_json::Value> = errors
            .map(|e| json!({"path": e.instance_path.to_string(), "error": e.to_string()}))
            .collect();
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"schema validation failed", "errors": errs}))).into_response();
    }
    // Apply and snapshot
    cfg["egress"] = egress.clone();
    let snapshot_id = if state.kernel_enabled() {
        match state.kernel().insert_config_snapshot_async(cfg.clone()).await {
            Ok(id) => id,
            Err(e) => return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
                ),
            )
                .into_response(),
        }
    } else {
        "kernel-disabled".to_string()
    };
    {
        let cfg_state = state.config_state();
        let mut cur = cfg_state.lock().await;
        *cur = cfg.clone();
    }
    {
        let history = state.config_history();
        let mut hist = history.lock().await;
        hist.push((snapshot_id.clone(), cfg.clone()));
    }

    // publish event, apply proxy toggle, and return effective settings with snapshot id
    state.bus().publish(topics::TOPIC_EGRESS_SETTINGS_UPDATED, &json!({"ts": chrono::Utc::now().to_rfc3339(), "who": "admin", "snapshot_id": snapshot_id }));
    egress_proxy::apply_current(state.clone()).await;

    let mut body = compose_egress_payload(&state).await;
    if let Some(map) = body.as_object_mut() {
        map.insert("snapshot_id".into(), json!(snapshot_id));
    }
    (axum::http::StatusCode::OK, Json(body)).into_response()
}
