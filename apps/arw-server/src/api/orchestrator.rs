use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::{runtime::RuntimeRestoreError, AppState};
use arw_topics as topics;

use arw_runtime::RuntimeRestartBudget;
use chrono::{Duration as ChronoDuration, SecondsFormat as ChronoSecondsFormat};
use tracing::warn;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentCatalog {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub items: Vec<MiniAgentEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub description: String,
    pub category: MiniAgentCategory,
    #[serde(default)]
    pub status: MiniAgentStatus,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub docs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub training: MiniAgentTraining,
    #[serde(default)]
    pub requirements: MiniAgentRequirements,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<MiniAgentOutputs>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MiniAgentLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MiniAgentCategory {
    Memory,
    Validation,
    Governor,
    Planner,
    Tooling,
    Support,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum MiniAgentStatus {
    Alpha,
    #[default]
    Beta,
    Stable,
    Incubating,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MiniAgentPreset {
    Balanced,
    Performance,
    #[serde(rename = "power_saver")]
    PowerSaver,
    Deep,
    Quick,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MiniAgentMode {
    Guided,
    Expert,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentTraining {
    pub goal_template: String,
    pub preset: MiniAgentPreset,
    pub mode: MiniAgentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<MiniAgentTrainingDefaults>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lanes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 1, maximum = 480)]
    pub est_runtime_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct MiniAgentTrainingDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub diversity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub recency: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub compression: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 0)]
    pub budget_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(minimum = 1, maximum = 100)]
    pub episodes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct MiniAgentRequirements {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub datasets: Vec<MiniAgentDataset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentDataset {
    pub id: String,
    pub description: String,
    #[serde(default = "MiniAgentDataset::default_required")]
    pub required: bool,
}

impl MiniAgentDataset {
    fn default_required() -> bool {
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct MiniAgentOutputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logic_unit: Option<MiniAgentLogicUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum MiniAgentLogicUnitKind {
    Config,
    Transform,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentLogicUnit {
    pub kind: MiniAgentLogicUnitKind,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MiniAgentLink {
    pub title: String,
    pub href: String,
}

static MINI_AGENT_CATALOG: Lazy<MiniAgentCatalog> = Lazy::new(|| {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../interfaces/mini_agents.json"
    ));
    serde_json::from_str(raw).expect("mini agent catalog json")
});

/// List available mini-agents and their training defaults.
#[utoipa::path(
    get,
    path = "/orchestrator/mini_agents",
    tag = "Orchestrator",
    responses(
        (status = 200, body = MiniAgentCatalog),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_mini_agents() -> impl IntoResponse {
    Json(MINI_AGENT_CATALOG.clone())
}

async fn ensure_runtime_policy(state: &AppState, action: &str) -> Result<(), Response> {
    let decision = state.policy().evaluate_action(action).await;
    if decision.allow {
        return Ok(());
    }

    let explain_payload = decision.explain.clone();
    if let Some(capability) = decision.require_capability.as_deref() {
        match state
            .kernel()
            .find_valid_lease_async("local", capability)
            .await
        {
            Ok(Some(_)) => Ok(()),
            Ok(None) => {
                state.bus().publish(
                    topics::TOPIC_POLICY_DECISION,
                    &json!({
                        "action": action,
                        "allow": false,
                        "require_capability": capability,
                        "explain": explain_payload,
                    }),
                );
                Err((
                    axum::http::StatusCode::FORBIDDEN,
                    Json(json!({
                        "type":"about:blank","title":"Forbidden","status":403,
                        "detail":"Denied (lease required)",
                        "require_capability": capability,
                        "explain": explain_payload
                    })),
                )
                    .into_response())
            }
            Err(err) => {
                warn!(
                    target: "runtime",
                    %err,
                    action,
                    "failed to verify runtime lease requirement"
                );
                Err((
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "type":"about:blank","title":"Error","status":500,
                        "detail": format!("failed to verify lease: {err}")
                    })),
                )
                    .into_response())
            }
        }
    } else {
        state.bus().publish(
            topics::TOPIC_POLICY_DECISION,
            &json!({
                "action": action,
                "allow": false,
                "explain": explain_payload,
            }),
        );
        Err((
            axum::http::StatusCode::FORBIDDEN,
            Json(json!({
                "type":"about:blank","title":"Forbidden","status":403,
                "detail":"Denied by policy",
                "explain": explain_payload
            })),
        )
            .into_response())
    }
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct OrchestratorStartReq {
    #[serde(default)]
    #[allow(dead_code)]
    pub id: Option<String>,
    pub goal: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    #[allow(dead_code)]
    pub budget: Option<serde_json::Value>,
}

/// Start a training job that results in a suggested Logic Unit (admin).
#[utoipa::path(
    post,
    path = "/orchestrator/mini_agents/start_training",
    tag = "Orchestrator",
    request_body = OrchestratorStartReq,
    responses(
        (status = 202, body = serde_json::Value),
        (status = 401, body = arw_protocol::ProblemDetails),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_start_training(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OrchestratorStartReq>,
) -> axum::response::Response {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let goal = req.goal.clone();
    let data = req.data.clone();

    let parse_hint_number = |value: Option<&serde_json::Value>| -> Option<f64> {
        value.and_then(|v| match v {
            serde_json::Value::Number(num) => num.as_f64(),
            serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
            _ => None,
        })
    };

    let preset_to_mode = |preset: &str| -> Option<&'static str> {
        match preset.to_ascii_lowercase().as_str() {
            "performance" => Some("deep"),
            "balanced" => Some("balanced"),
            "power-saver" | "power_saver" => Some("quick"),
            "quick" => Some("quick"),
            "deep" => Some("deep"),
            "verified" => Some("verified"),
            _ => None,
        }
    };

    let mut training_meta_map = serde_json::Map::new();
    training_meta_map.insert("goal".into(), serde_json::Value::String(goal.clone()));
    let mut job_data_map = serde_json::Map::new();
    if let Some(ref raw) = data {
        job_data_map.insert("submitted".into(), raw.clone());
    }

    let (preset_value, diversity_hint, recency_hint, compression_hint, mode_hint) = data
        .as_ref()
        .and_then(|value| value.as_object())
        .map(|raw| {
            let training_source = raw
                .get("training")
                .and_then(|v| v.as_object())
                .unwrap_or(raw);
            let preset = training_source
                .get("preset")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());
            let diversity =
                parse_hint_number(training_source.get("diversity")).map(|v| v.clamp(0.0, 1.0));
            let recency =
                parse_hint_number(training_source.get("recency")).map(|v| v.clamp(0.0, 1.0));
            let compression =
                parse_hint_number(training_source.get("compression")).map(|v| v.clamp(0.0, 1.0));
            let mode = preset
                .as_deref()
                .and_then(preset_to_mode)
                .map(|m| m.to_string());
            (preset, diversity, recency, compression, mode)
        })
        .unwrap_or((None, None, None, None, None));

    if let Some(ref preset) = preset_value {
        job_data_map.insert("preset".into(), serde_json::Value::String(preset.clone()));
        training_meta_map.insert("preset".into(), serde_json::Value::String(preset.clone()));
    }
    if let Some(div) = diversity_hint {
        if let Some(num) = serde_json::Number::from_f64(div) {
            job_data_map.insert("diversity".into(), serde_json::Value::Number(num.clone()));
            training_meta_map.insert("diversity".into(), serde_json::Value::Number(num));
        }
    }
    if let Some(rec) = recency_hint {
        if let Some(num) = serde_json::Number::from_f64(rec) {
            job_data_map.insert("recency".into(), serde_json::Value::Number(num.clone()));
            training_meta_map.insert("recency".into(), serde_json::Value::Number(num));
        }
    }
    if let Some(comp) = compression_hint {
        if let Some(num) = serde_json::Number::from_f64(comp) {
            job_data_map.insert("compression".into(), serde_json::Value::Number(num.clone()));
            training_meta_map.insert("compression".into(), serde_json::Value::Number(num));
        }
    }
    if let Some(ref mode) = mode_hint {
        job_data_map.insert("mode".into(), serde_json::Value::String(mode.clone()));
        training_meta_map.insert("mode".into(), serde_json::Value::String(mode.clone()));
    }
    if !training_meta_map.is_empty() {
        job_data_map.insert(
            "training".into(),
            serde_json::Value::Object(training_meta_map.clone()),
        );
    }

    let job_data_value = if job_data_map.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(job_data_map.clone()))
    };

    let bus = state.bus();
    if mode_hint.is_some()
        || diversity_hint.is_some()
        || recency_hint.is_some()
        || compression_hint.is_some()
    {
        state
            .governor()
            .apply_hints(
                &bus,
                None,
                None,
                None,
                mode_hint.clone(),
                None,
                None,
                diversity_hint,
                recency_hint,
                compression_hint,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some("orchestrator"),
            )
            .await;
    }

    let id = match state
        .kernel()
        .insert_orchestrator_job_async(req.goal.as_str(), job_data_value.as_ref())
        .await
    {
        Ok(id) => id,
        Err(e) => return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    };
    let mut created_payload = json!({"id": id, "goal": goal});
    if let Some(data_value) = job_data_value.clone() {
        if let serde_json::Value::Object(ref mut map) = created_payload {
            map.insert("data".into(), data_value);
        }
    }
    state
        .bus()
        .publish(topics::TOPIC_ORCHESTRATOR_JOB_CREATED, &created_payload);
    let state2 = state.clone();
    let id_clone = id.clone();
    let training_meta_for_hints = training_meta_map.clone();
    tokio::spawn(async move {
        let steps = 5;
        for i in 1..=steps {
            let p = (i as f64) / (steps as f64);
            let _ = state2
                .kernel()
                .update_orchestrator_job_async(
                    id_clone.clone(),
                    Some(if i < steps { "running" } else { "completed" }.to_string()),
                    Some(p),
                )
                .await;
            state2.bus().publish(
                topics::TOPIC_ORCHESTRATOR_JOB_PROGRESS,
                &json!({"id": id_clone, "progress": p}),
            );
            if i < steps {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        state2.bus().publish(
            topics::TOPIC_ORCHESTRATOR_JOB_COMPLETED,
            &json!({"id": id_clone, "ok": true}),
        );
        // Suggest a Logic Unit manifest as an output of the training
        let lu_id = format!("lu-{}", id_clone);
        let mut hints_map = serde_json::Map::new();
        if let Some(mode) = mode_hint.clone() {
            hints_map.insert("mode".into(), serde_json::Value::String(mode));
        }
        if let Some(div) = diversity_hint {
            if let Some(num) = serde_json::Number::from_f64(div) {
                hints_map.insert("retrieval_div".into(), serde_json::Value::Number(num));
            }
        }
        if let Some(rec) = recency_hint {
            if let Some(num) = serde_json::Number::from_f64(rec) {
                hints_map.insert("mmr_lambda".into(), serde_json::Value::Number(num));
            }
        }
        if let Some(comp) = compression_hint {
            if let Some(num) = serde_json::Number::from_f64(comp) {
                hints_map.insert("compression_aggr".into(), serde_json::Value::Number(num));
            }
        }
        if !training_meta_for_hints.is_empty() {
            hints_map.insert(
                "training".into(),
                serde_json::Value::Object(training_meta_for_hints.clone()),
            );
        }
        let hints_value = serde_json::Value::Object(hints_map.clone());
        let manifest = json!({
            "id": lu_id,
            "kind": "config-only",
            "patches": [
                {"target": "governor.hints", "op": "merge", "value": hints_value.clone()}
            ]
        });
        let _ = state2
            .kernel()
            .insert_logic_unit_async(lu_id.clone(), manifest.clone(), "suggested".to_string())
            .await;
        let mut suggested_payload = json!({"id": lu_id, "job_id": id_clone});
        if let serde_json::Value::Object(ref mut map) = suggested_payload {
            map.insert("hints".into(), hints_value);
        }
        state2
            .bus()
            .publish(topics::TOPIC_LOGICUNIT_SUGGESTED, &suggested_payload);

        let lease_id = Uuid::new_v4().to_string();
        let ttl_until = (chrono::Utc::now() + ChronoDuration::minutes(5))
            .to_rfc3339_opts(ChronoSecondsFormat::Millis, true);
        if let Err(err) = state2
            .kernel()
            .insert_lease_async(
                lease_id.clone(),
                "orchestrator".into(),
                "modular:write".into(),
                Some("orchestrator".into()),
                ttl_until,
                None,
                None,
            )
            .await
        {
            warn!(target: "arw::orchestrator", error = %err, "failed to insert modular lease");
        } else {
            let agent_action_id = Uuid::new_v4().to_string();
            let modular_payload = json!({
                "agent_id": "orchestrator.trainer",
                "turn_id": id_clone,
                "intent": "orchestrator.summary",
                "payload": {
                    "goal": goal,
                    "logic_unit_id": lu_id,
                    "hints": hints_map,
                    "training_meta": training_meta_map,
                },
                "context_refs": [],
                "evidence_ids": [lu_id.clone()],
                "confidence": 0.6,
                "latency_budget_ms": 0,
                "policy_scope": {
                    "leases": [lease_id.clone()],
                    "capabilities": ["modular:write"],
                }
            });
            if let Err(err) = state2
                .kernel()
                .insert_action_async(
                    &agent_action_id,
                    "modular.agent_message",
                    &modular_payload,
                    None,
                    None,
                    "queued",
                )
                .await
            {
                warn!(
                    target: "arw::orchestrator",
                    error = %err,
                    "failed to enqueue modular.agent_message"
                );
            } else {
                state2.signal_action_queue();
                state2.metrics().queue_enqueued();
            }

            let tool_action_id = Uuid::new_v4().to_string();
            let modular_tool_payload = json!({
                "invocation_id": format!("invoke-{}", id_clone),
                "requested_by": "orchestrator.trainer",
                "tool_id": "training.job",
                "operation_id": "training.job@1.0.0",
                "input_payload": {
                    "goal": goal,
                    "job_id": id_clone,
                    "preset": preset_value,
                    "training_meta": training_meta_map,
                },
                "sandbox_requirements": {
                    "needs_network": false,
                    "filesystem_scopes": [],
                },
                "policy_scope": {
                    "leases": [lease_id.clone()],
                    "capabilities": []
                },
                "evidence_id": lu_id,
            });
            if let Err(err) = state2
                .kernel()
                .insert_action_async(
                    &tool_action_id,
                    "modular.tool_invocation",
                    &modular_tool_payload,
                    None,
                    None,
                    "queued",
                )
                .await
            {
                warn!(
                    target: "arw::orchestrator",
                    error = %err,
                    "failed to enqueue modular.tool_invocation"
                );
            } else {
                state2.signal_action_queue();
                state2.metrics().queue_enqueued();
            }
        }
    });
    (
        axum::http::StatusCode::ACCEPTED,
        Json(json!({"job_id": id, "ok": true})),
    )
        .into_response()
}

/// Orchestrator jobs snapshot.
#[utoipa::path(
    get,
    path = "/state/orchestrator/jobs",
    tag = "Orchestrator",
    params(("limit" = Option<i64>, Query)),
    responses(
        (status = 200, body = serde_json::Value),
        (status = 501, description = "Kernel disabled", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn state_orchestrator_jobs(
    State(state): State<AppState>,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    if !state.kernel_enabled() {
        return crate::responses::kernel_disabled();
    }
    let limit = q
        .get("limit")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(200);
    match state.kernel().list_orchestrator_jobs_async(limit).await {
        Ok(items) => Json(json!({"items": items})).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(
                json!({"type":"about:blank","title":"Error","status":500, "detail": e.to_string()}),
            ),
        )
            .into_response(),
    }
}

fn default_restart_true() -> bool {
    true
}

#[derive(Deserialize, ToSchema)]
pub struct RuntimeRestoreRequest {
    #[serde(default = "default_restart_true")]
    pub restart: bool,
    #[serde(default)]
    pub preset: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeRestoreResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub pending: bool,
    pub restart_budget: RuntimeRestartBudgetView,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeRestoreFailureResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub pending: bool,
    pub reason: String,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeRestoreDeniedResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub pending: bool,
    pub reason: String,
    pub restart_budget: RuntimeRestartBudgetView,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeShutdownResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub stopped: bool,
}

#[derive(Serialize, ToSchema)]
pub struct RuntimeShutdownFailureResponse {
    pub ok: bool,
    pub runtime_id: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct RuntimeRestartBudgetView {
    pub window_seconds: u64,
    pub max_restarts: u32,
    pub used: u32,
    pub remaining: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<String>,
}

impl From<RuntimeRestartBudget> for RuntimeRestartBudgetView {
    fn from(budget: RuntimeRestartBudget) -> Self {
        let reset_at = budget
            .reset_at
            .map(|ts| ts.to_rfc3339_opts(ChronoSecondsFormat::Secs, true));
        Self {
            window_seconds: budget.window_seconds,
            max_restarts: budget.max_restarts,
            used: budget.used,
            remaining: budget.remaining,
            reset_at,
        }
    }
}

/// Request a managed runtime restore.
#[utoipa::path(
    post,
    path = "/orchestrator/runtimes/{id}/restore",
    tag = "Orchestrator",
    params(("id" = String, Path, description = "Runtime identifier")),
    request_body = RuntimeRestoreRequest,
    responses(
        (status = 202, description = "Restore requested", body = RuntimeRestoreResponse),
        (status = 500, description = "Restore failed", body = RuntimeRestoreFailureResponse),
        (status = 429, description = "Restart budget exhausted", body = RuntimeRestoreDeniedResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_runtime_restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(runtime_id): Path<String>,
    Json(req): Json<RuntimeRestoreRequest>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }

    if let Err(resp) = ensure_runtime_policy(&state, "runtime.supervisor.restore").await {
        return resp;
    }

    match state
        .runtime()
        .request_restore(&runtime_id, req.restart, req.preset.clone())
        .await
    {
        Ok(budget) => (
            axum::http::StatusCode::ACCEPTED,
            Json(RuntimeRestoreResponse {
                ok: true,
                runtime_id,
                pending: true,
                restart_budget: budget.into(),
            }),
        )
            .into_response(),
        Err(RuntimeRestoreError::RestartDenied { budget }) => (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(RuntimeRestoreDeniedResponse {
                ok: false,
                runtime_id,
                pending: false,
                reason: "Restart budget exhausted".to_string(),
                restart_budget: budget.into(),
            }),
        )
            .into_response(),
        Err(RuntimeRestoreError::RestoreFailed { reason }) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuntimeRestoreFailureResponse {
                ok: false,
                runtime_id,
                pending: false,
                reason,
            }),
        )
            .into_response(),
    }
}

/// Request a managed runtime shutdown.
#[utoipa::path(
    post,
    path = "/orchestrator/runtimes/{id}/shutdown",
    tag = "Orchestrator",
    params(("id" = String, Path, description = "Runtime identifier")),
    responses(
        (status = 202, description = "Shutdown requested", body = RuntimeShutdownResponse),
        (status = 500, description = "Shutdown failed", body = RuntimeShutdownFailureResponse),
        (status = 401, description = "Unauthorized", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn orchestrator_runtime_shutdown(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(runtime_id): Path<String>,
) -> impl IntoResponse {
    if let Err(resp) = crate::responses::require_admin(&headers).await {
        return *resp;
    }

    if let Err(resp) = ensure_runtime_policy(&state, "runtime.supervisor.shutdown").await {
        return resp;
    }

    match state
        .runtime_supervisor()
        .shutdown_runtime(&runtime_id)
        .await
    {
        Ok(_) => (
            axum::http::StatusCode::ACCEPTED,
            Json(RuntimeShutdownResponse {
                ok: true,
                runtime_id,
                stopped: true,
            }),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(RuntimeShutdownFailureResponse {
                ok: false,
                runtime_id,
                reason: err.to_string(),
            }),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_supervisor::ManagedRuntimeDefinition;
    use crate::test_support;
    use arw_runtime::{
        AdapterError, PrepareContext, PreparedRuntime, RuntimeAdapter, RuntimeDescriptor,
        RuntimeHandle, RuntimeHealthReport, RuntimeModality, RuntimeState, RuntimeStatus,
    };
    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
    use serde_json::Value;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[test]
    fn mini_agent_catalog_includes_entries() {
        assert!(
            !MINI_AGENT_CATALOG.items.is_empty(),
            "expected mini-agent catalog to contain at least one entry"
        );
        for entry in MINI_AGENT_CATALOG.items.iter() {
            assert!(
                !entry.training.goal_template.trim().is_empty(),
                "mini-agent {} missing goal template",
                entry.id
            );
        }
    }

    #[derive(Debug)]
    struct StubRuntimeAdapter;

    #[async_trait]
    impl RuntimeAdapter for StubRuntimeAdapter {
        fn id(&self) -> &'static str {
            "stub.test"
        }

        async fn prepare(&self, ctx: PrepareContext<'_>) -> Result<PreparedRuntime, AdapterError> {
            Ok(PreparedRuntime {
                command: "stub".to_string(),
                args: Vec::new(),
                runtime_id: Some(ctx.descriptor.id.clone()),
            })
        }

        async fn launch(&self, prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError> {
            Ok(RuntimeHandle {
                id: prepared
                    .runtime_id
                    .unwrap_or_else(|| "stub-runtime".to_string()),
                pid: None,
            })
        }

        async fn shutdown(&self, _handle: RuntimeHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn health(
            &self,
            handle: &RuntimeHandle,
        ) -> Result<RuntimeHealthReport, AdapterError> {
            let status =
                RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready).with_summary("Ready");
            Ok(RuntimeHealthReport { status })
        }
    }

    async fn install_runtime_with_adapter(
        supervisor: Arc<crate::runtime_supervisor::RuntimeSupervisor>,
        runtime_id: &str,
        adapter: Arc<dyn RuntimeAdapter>,
    ) {
        let adapter_id = adapter.id().to_string();
        supervisor.register_adapter(adapter).await;
        let mut descriptor = RuntimeDescriptor::new(runtime_id, adapter_id.clone());
        descriptor.modalities.push(RuntimeModality::Text);
        supervisor
            .install_definition(ManagedRuntimeDefinition::new(
                descriptor,
                adapter_id,
                false,
                None,
                Some("tests".into()),
            ))
            .await
            .expect("install runtime definition");
    }

    async fn install_stub_runtime(
        supervisor: Arc<crate::runtime_supervisor::RuntimeSupervisor>,
        runtime_id: &str,
    ) {
        install_runtime_with_adapter(supervisor, runtime_id, Arc::new(StubRuntimeAdapter)).await;
    }

    #[derive(Debug)]
    struct FailingRuntimeAdapter;

    #[async_trait]
    impl RuntimeAdapter for FailingRuntimeAdapter {
        fn id(&self) -> &'static str {
            "stub.fail"
        }

        async fn prepare(&self, ctx: PrepareContext<'_>) -> Result<PreparedRuntime, AdapterError> {
            Ok(PreparedRuntime {
                command: "stub-fail".to_string(),
                args: Vec::new(),
                runtime_id: Some(ctx.descriptor.id.clone()),
            })
        }

        async fn launch(&self, _prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError> {
            Err(AdapterError::Launch("stub launch failed".into()))
        }

        async fn shutdown(&self, _handle: RuntimeHandle) -> Result<(), AdapterError> {
            Ok(())
        }

        async fn health(
            &self,
            handle: &RuntimeHandle,
        ) -> Result<RuntimeHealthReport, AdapterError> {
            let status =
                RuntimeStatus::new(handle.id.clone(), RuntimeState::Ready).with_summary("Ready");
            Ok(RuntimeHealthReport { status })
        }
    }

    #[tokio::test]
    async fn runtime_restore_denied_without_runtime_manage_lease() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        let app = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/restore",
                post(orchestrator_runtime_restore),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/orchestrator/runtimes/runtime-test/restore")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .body(Body::from("{}"))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(
            payload.get("require_capability").and_then(Value::as_str),
            Some("runtime:manage")
        );
        assert_eq!(
            payload.get("detail").and_then(Value::as_str),
            Some("Denied (lease required)")
        );
    }

    #[tokio::test]
    async fn runtime_restore_allowed_when_runtime_manage_lease_present() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        let supervisor = state.runtime_supervisor();
        install_stub_runtime(supervisor.clone(), "runtime-test").await;
        let ttl =
            (Utc::now() + ChronoDuration::minutes(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease(
                "lease-runtime-manage",
                "local",
                "runtime:manage",
                None,
                &ttl,
                None,
                None,
            )
            .expect("insert lease");

        let app = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/restore",
                post(orchestrator_runtime_restore),
            )
            .with_state(state.clone());

        let request = Request::builder()
            .method("POST")
            .uri("/orchestrator/runtimes/runtime-test/restore")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .body(Body::from("{}"))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(payload.get("ok"), Some(&Value::Bool(true)));
        assert_eq!(
            payload.get("runtime_id").and_then(Value::as_str),
            Some("runtime-test")
        );
    }

    #[tokio::test]
    async fn runtime_restore_denied_when_restart_budget_exhausted() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");
        ctx.env.set("ARW_RUNTIME_RESTART_MAX", "1");
        ctx.env.set("ARW_RUNTIME_RESTART_WINDOW_SEC", "3600");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        install_stub_runtime(state.runtime_supervisor(), "runtime-test").await;

        let ttl =
            (Utc::now() + ChronoDuration::minutes(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease(
                "lease-runtime-manage",
                "local",
                "runtime:manage",
                None,
                &ttl,
                None,
                None,
            )
            .expect("insert lease");

        fn build_request() -> Request<Body> {
            Request::builder()
                .method("POST")
                .uri("/orchestrator/runtimes/runtime-test/restore")
                .header("content-type", "application/json")
                .header("x-arw-admin", "secret-token")
                .body(Body::from("{}"))
                .expect("request")
        }

        let response_ok = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/restore",
                post(orchestrator_runtime_restore),
            )
            .with_state(state.clone())
            .oneshot(build_request())
            .await
            .expect("router response");
        assert_eq!(response_ok.status(), StatusCode::ACCEPTED);

        let response_budget = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/restore",
                post(orchestrator_runtime_restore),
            )
            .with_state(state.clone())
            .oneshot(build_request())
            .await
            .expect("router response");
        assert_eq!(response_budget.status(), StatusCode::TOO_MANY_REQUESTS);
        let bytes = to_bytes(response_budget.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason").and_then(Value::as_str),
            Some("Restart budget exhausted")
        );
        let remaining = payload
            .get("restart_budget")
            .and_then(|v| v.get("remaining"))
            .and_then(Value::as_u64);
        assert_eq!(remaining, Some(0));
    }

    #[tokio::test]
    async fn runtime_restore_returns_error_when_launch_fails() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        install_runtime_with_adapter(
            state.runtime_supervisor(),
            "runtime-fail",
            Arc::new(FailingRuntimeAdapter),
        )
        .await;

        let ttl =
            (Utc::now() + ChronoDuration::minutes(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease(
                "lease-runtime-manage",
                "local",
                "runtime:manage",
                None,
                &ttl,
                None,
                None,
            )
            .expect("insert lease");

        let app = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/restore",
                post(orchestrator_runtime_restore),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/orchestrator/runtimes/runtime-fail/restore")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .body(Body::from("{}"))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(payload.get("ok"), Some(&Value::Bool(false)));
        assert_eq!(
            payload.get("reason").and_then(Value::as_str),
            Some("launch failure: stub launch failed")
        );
    }

    #[tokio::test]
    async fn runtime_shutdown_denied_without_runtime_manage_lease() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        install_stub_runtime(state.runtime_supervisor(), "runtime-test").await;

        let app = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/shutdown",
                post(orchestrator_runtime_shutdown),
            )
            .with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/orchestrator/runtimes/runtime-test/shutdown")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .body(Body::from("{}"))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(
            payload.get("require_capability").and_then(Value::as_str),
            Some("runtime:manage")
        );
        assert_eq!(
            payload.get("detail").and_then(Value::as_str),
            Some("Denied (lease required)")
        );
    }

    #[tokio::test]
    async fn runtime_shutdown_allowed_when_runtime_manage_lease_present() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_SECURITY_POSTURE", "standard");
        ctx.env.set("ARW_ADMIN_TOKEN", "secret-token");

        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        install_stub_runtime(state.runtime_supervisor(), "runtime-test").await;

        let ttl =
            (Utc::now() + ChronoDuration::minutes(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease(
                "lease-runtime-manage",
                "local",
                "runtime:manage",
                None,
                &ttl,
                None,
                None,
            )
            .expect("insert lease");

        let app = Router::new()
            .route(
                "/orchestrator/runtimes/{id}/shutdown",
                post(orchestrator_runtime_shutdown),
            )
            .with_state(state.clone());

        let request = Request::builder()
            .method("POST")
            .uri("/orchestrator/runtimes/runtime-test/shutdown")
            .header("content-type", "application/json")
            .header("x-arw-admin", "secret-token")
            .body(Body::from("{}"))
            .expect("request");

        let response = app.oneshot(request).await.expect("router response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("json payload");
        assert_eq!(payload.get("ok"), Some(&Value::Bool(true)));
        assert_eq!(
            payload.get("runtime_id").and_then(Value::as_str),
            Some("runtime-test")
        );
        assert_eq!(payload.get("stopped").and_then(Value::as_bool), Some(true));
    }
}
