#[cfg(not(test))]
use utoipa::OpenApi;
use utoipa::ToSchema;

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct HealthOk {
    pub ok: bool,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct HttpInfo {
    pub bind: String,
    pub port: u16,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct AboutCounts {
    pub public: usize,
    pub admin: usize,
    pub total: usize,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct PerfPreset {
    pub tier: Option<String>,
    pub http_max_conc: Option<usize>,
    pub actions_queue_max: Option<i64>,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct AboutResponse {
    pub service: String,
    pub version: String,
    pub http: HttpInfo,
    #[schema(nullable, value_type = Option<String>)]
    pub docs_url: Option<String>,
    #[schema(nullable, value_type = Option<String>)]
    pub security_posture: Option<String>,
    pub counts: AboutCounts,
    #[schema(example = json!( ["GET /healthz", "GET /about"] ))]
    pub endpoints: Vec<String>,
    #[schema(value_type = Vec<serde_json::Value>)]
    pub endpoints_meta: Vec<serde_json::Value>,
    pub perf_preset: PerfPreset,
}

#[cfg_attr(test, allow(dead_code))]
#[cfg(not(test))]
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::meta::healthz,
        crate::api::meta::about,
        crate::api::spec::spec_health,
        crate::api::metrics::metrics_prometheus,
        crate::api::state::state_models,
        crate::api::state::state_runtime_matrix,
        crate::api::state::state_route_stats,
        crate::api::state::state_tasks,
        crate::api::state::state_observations,
        crate::api::state::state_beliefs,
        crate::api::state::state_intents,
        crate::api::state::state_actions,
        crate::api::state::state_experiments,
        crate::api::state::state_research_watcher,
        crate::api::state::state_staging_actions,
        crate::api::state::state_training_telemetry,
        crate::api::state::state_cluster,
        crate::api::state::state_world,
        crate::api::state::state_world_select,
        crate::api::state::state_guardrails_metrics,
        crate::api::events::events_sse,
        crate::api::models::models_summary,
        crate::api::models::models_list,
        crate::api::models::models_refresh,
        crate::api::models::models_save,
        crate::api::models::models_load,
        crate::api::models::models_add,
        crate::api::models::models_remove,
        crate::api::models::models_default_get,
        crate::api::models::models_default_set,
        crate::api::models::models_concurrency_get,
        crate::api::models::models_concurrency_set,
        crate::api::models::models_jobs,
        crate::api::models::state_models_metrics,
        crate::api::models::state_models_hashes,
        crate::api::models::models_download,
        crate::api::models::models_download_cancel,
        crate::api::models::models_cas_gc,
        crate::api::models::models_blob_by_hash,
        crate::api::tools::tools_list,
        crate::api::tools::tools_run,
        crate::api::tools::tools_cache_stats,
        crate::api::memory::admin_memory_list,
        crate::api::memory::admin_memory_apply,
        crate::api::orchestrator::orchestrator_mini_agents,
        crate::api::orchestrator::orchestrator_start_training,
        crate::api::orchestrator::state_orchestrator_jobs,
        crate::api::probe::probe_effective_paths,
        crate::api::probe::probe_hw,
        crate::api::probe::probe_metrics,
        crate::api::metrics::metrics_overview,
        crate::api::governor::governor_profile_get,
        crate::api::governor::governor_profile_set,
        crate::api::governor::governor_hints_get,
        crate::api::governor::governor_hints_set,
        crate::api::feedback::feedback_state,
        crate::api::feedback::feedback_signal,
        crate::api::feedback::feedback_analyze,
        crate::api::feedback::feedback_apply,
        crate::api::feedback::feedback_auto,
        crate::api::feedback::feedback_reset,
        crate::api::distill::distill_run,
        crate::api::feedback::feedback_suggestions,
        crate::api::feedback::feedback_updates,
        crate::api::feedback::feedback_policy,
        crate::api::feedback::feedback_versions,
        crate::api::feedback::feedback_rollback,
        crate::api::goldens::goldens_list,
        crate::api::goldens::goldens_add,
        crate::api::goldens::goldens_run,
        crate::api::experiments::experiments_define,
        crate::api::experiments::experiments_run,
        crate::api::experiments::experiments_activate,
        crate::api::experiments::experiments_list,
        crate::api::experiments::experiments_scoreboard,
        crate::api::experiments::experiments_winners,
        crate::api::experiments::experiments_start,
        crate::api::experiments::experiments_stop,
        crate::api::experiments::experiments_assign,
        crate::api::hierarchy::hierarchy_state,
        crate::api::hierarchy::hierarchy_role_set,
        crate::api::hierarchy::hierarchy_hello,
        crate::api::hierarchy::hierarchy_offer,
        crate::api::hierarchy::hierarchy_accept,
        crate::api::self_model::self_model_propose,
        crate::api::self_model::self_model_apply,
        crate::api::projects::state_projects_list,
        crate::api::projects::projects_create_unified,
        crate::api::projects::state_projects_tree,
        crate::api::projects::state_projects_notes,
        crate::api::projects::state_projects_file_get,
        crate::api::projects::projects_notes_put,
        crate::api::projects::projects_file_put,
        crate::api::projects::projects_file_patch_unified,
        crate::api::projects::projects_import_unified,
        crate::api::research_watcher::research_watcher_approve,
        crate::api::research_watcher::research_watcher_archive,
        crate::api::review::memory_quarantine_get,
        crate::api::review::memory_quarantine_queue,
        crate::api::review::memory_quarantine_admit,
        crate::api::review::world_diffs_get,
        crate::api::review::world_diffs_queue,
        crate::api::review::world_diffs_decision,
        crate::api::staging::staging_action_approve,
        crate::api::staging::staging_action_deny,
        crate::api::chat::chat_history,
        crate::api::chat::chat_send,
        crate::api::chat::chat_clear,
        crate::api::chat::chat_status,
    ),
    components(
        schemas(
            HealthOk,
            HttpInfo,
            AboutCounts,
            PerfPreset,
            AboutResponse,
            crate::governor::Hints,
            crate::api::hierarchy::RoleRequest,
            crate::api::self_model::SelfModelProposeRequest,
            crate::api::self_model::SelfModelApplyRequest,
            crate::api::projects::ProjectCreateRequest,
            crate::api::projects::ProjectFileWrite,
            crate::api::projects::ProjectPatchRequest,
            crate::api::projects::ProjectImportRequest,
            crate::api::orchestrator::OrchestratorStartReq,
            crate::api::research_watcher::WatcherDecision,
            crate::api::staging::StagingDecision,
            crate::api::tools::ToolRunRequest,
            crate::feedback::FeedbackState,
            crate::api::feedback::FeedbackSignalRequest,
            crate::api::feedback::FeedbackApplyRequest,
            crate::api::feedback::FeedbackAutoRequest,
            crate::feedback::FeedbackSignal,
            crate::feedback::Suggestion,
            crate::goldens::GoldenItem,
            crate::goldens::EvalSummary,
            crate::api::goldens::GoldensAddRequest,
            crate::api::goldens::GoldensRunRequest,
            crate::api::memory::MemoryApplyReq,
            crate::experiments::VariantCfg,
            crate::experiments::Experiment,
            crate::experiments::WinnerInfo,
            crate::experiments::ScoreEntry,
            crate::experiments::ScoreRow,
            crate::experiments::RunOutcome,
            crate::experiments::RunOutcomeVariant,
            crate::api::experiments::ExperimentDefineRequest,
            crate::api::experiments::ExperimentRunRequest,
            crate::api::experiments::ExperimentActivateRequest,
            crate::api::experiments::ExperimentStartRequest,
            crate::api::experiments::ExperimentStopRequest,
            crate::api::experiments::ExperimentAssignRequest,
            crate::review::MemoryQuarantineRequest,
            crate::review::MemoryQuarantineAdmit,
            crate::api::chat::ChatSendReq,
            crate::api::chat::ChatSendResp,
            crate::api::chat::ChatHistory,
            crate::api::chat::ChatStatusResp,
            crate::chat::ChatMessage,
            crate::review::WorldDiffQueueRequest,
            crate::review::WorldDiffDecision
        )
    ),
    tags(
        (name = "Meta", description = "Service metadata and health"),
        (name = "State", description = "Read‑models (actions, models, egress, episodes)"),
        (name = "State/Projects", description = "Project tree and notes read-models"),
        (name = "Events", description = "Server‑Sent Events stream"),
        (name = "Models", description = "Model steward admin endpoints"),
        (name = "Projects", description = "Project workspace management"),
        (name = "Review", description = "World diff and memory review utilities"),
        (name = "Specs", description = "Specification document endpoints"),
        (name = "Distill", description = "Distillation endpoints"),
        (name = "Orchestrator", description = "Orchestrator endpoints"),
        (name = "Public", description = "Public endpoints"),
        (name = "Admin/Introspect", description = "Administrative probes and telemetry"),
        (name = "Admin/Hierarchy", description = "Hierarchy coordination endpoints"),
        (name = "Admin/SelfModel", description = "Self-model lifecycle endpoints"),
        (name = "Admin/Review", description = "Memory quarantine and world diff review queue"),
        (name = "Admin/Feedback", description = "Feedback engine signals and suggestions"),
        (name = "Admin/Tools", description = "Tool Forge and action cache"),
        (name = "Admin/Chat", description = "Administrative chat endpoints"),
        (name = "Admin/Experiments", description = "Administrative experiments endpoints"),
        (name = "Admin/Memory", description = "Administrative memory endpoints"),
        (name = "Research", description = "Research watcher workflow"),
        (name = "Staging", description = "Human-in-the-loop approvals queue")
    ),
    info(description = "Unified ARW server API surface (headless-first)."),
    modifiers(&OperationIdModifier)
)]
pub struct ApiDoc;

#[cfg(test)]
pub struct ApiDoc;

#[cfg(test)]
impl ApiDoc {
    pub fn openapi() -> utoipa::openapi::OpenApi {
        utoipa::openapi::OpenApi::default()
    }
}

#[cfg(not(test))]
struct OperationIdModifier;

#[cfg(not(test))]
impl utoipa::Modify for OperationIdModifier {
    fn modify(&self, doc: &mut utoipa::openapi::OpenApi) {
        apply_operation_ids(doc);
    }
}

#[cfg(not(test))]
fn apply_operation_ids(doc: &mut utoipa::openapi::OpenApi) {
    use utoipa::openapi::path::Operation;

    for (path, item) in doc.paths.paths.iter_mut() {
        let update = |op: &mut Operation, method: &str| {
            let existing_id = op
                .operation_id
                .clone()
                .unwrap_or_else(|| format!("{}_{}", method, sanitize_path(path)));
            op.operation_id = Some(normalize_operation_id(&existing_id));
        };

        if let Some(op) = item.get.as_mut() {
            update(op, "get");
        }
        if let Some(op) = item.put.as_mut() {
            update(op, "put");
        }
        if let Some(op) = item.post.as_mut() {
            update(op, "post");
        }
        if let Some(op) = item.delete.as_mut() {
            update(op, "delete");
        }
        if let Some(op) = item.options.as_mut() {
            update(op, "options");
        }
        if let Some(op) = item.head.as_mut() {
            update(op, "head");
        }
        if let Some(op) = item.patch.as_mut() {
            update(op, "patch");
        }
        if let Some(op) = item.trace.as_mut() {
            update(op, "trace");
        }
    }
}

#[cfg(not(test))]
fn sanitize_path(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '{' | '}' | '-' | '/' => {
                if !out.ends_with('_') {
                    out.push('_');
                }
            }
            _ => out.push(ch),
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "root".into()
    } else {
        out
    }
}

#[cfg(not(test))]
fn normalize_operation_id(raw: &str) -> String {
    let mut snake = to_snake_case(raw);
    if !snake.ends_with("_doc") {
        snake.push_str("_doc");
    }
    snake
}

#[cfg(not(test))]
fn to_snake_case(input: &str) -> String {
    let mut out = String::new();
    let mut prev_is_lower_or_digit = false;
    for ch in input.chars() {
        if ch.is_ascii_uppercase() {
            if prev_is_lower_or_digit && !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = true;
        } else if ch.is_ascii_alphanumeric() {
            if ch == '_' {
                if !out.ends_with('_') {
                    out.push('_');
                }
                prev_is_lower_or_digit = false;
            } else {
                out.push(ch);
                prev_is_lower_or_digit = true;
            }
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_is_lower_or_digit = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        "op".into()
    } else {
        out
    }
}
