use utoipa::{OpenApi, ToSchema};

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

#[allow(dead_code)]
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api_meta::healthz,
        crate::api_meta::about,
        crate::api_metrics::metrics_prometheus,
        crate::api_state::state_models,
        crate::api_state::state_runtime_matrix,
        crate::api_state::state_actions,
        crate::api_state::state_experiments,
        crate::api_state::state_research_watcher,
        crate::api_state::state_staging_actions,
        crate::api_state::state_training_telemetry,
        crate::api_state::admin_state_observations,
        crate::api_state::admin_state_beliefs,
        crate::api_state::admin_state_intents,
        crate::api_state::admin_state_actions,
        crate::api_state::admin_state_guardrails_metrics,
        crate::api_state::admin_state_cluster,
        crate::api_state::admin_state_world,
        crate::api_state::admin_state_world_select,
        crate::api_events::events_sse,
        crate::api_models::models_summary,
        crate::api_models::models_list,
        crate::api_models::models_refresh,
        crate::api_models::models_save,
        crate::api_models::models_load,
        crate::api_models::models_add,
        crate::api_models::models_remove,
        crate::api_models::models_default_get,
        crate::api_models::models_default_set,
        crate::api_models::models_concurrency_get,
        crate::api_models::models_concurrency_set,
        crate::api_models::models_jobs,
        crate::api_models::models_metrics,
        crate::api_models::models_hashes,
        crate::api_models::models_download,
        crate::api_models::models_download_cancel,
        crate::api_models::models_cas_gc,
        crate::api_tools::tools_list,
        crate::api_tools::tools_run,
        crate::api_tools::tools_cache_stats,
        crate::api_probe::probe_effective_paths,
        crate::api_probe::probe_hw,
        crate::api_probe::probe_metrics,
        crate::api_metrics::metrics_overview,
        crate::api_governor::governor_profile_get,
        crate::api_governor::governor_profile_set,
        crate::api_governor::governor_hints_get,
        crate::api_governor::governor_hints_set,
        crate::api_feedback::feedback_state,
        crate::api_feedback::feedback_signal,
        crate::api_feedback::feedback_analyze,
        crate::api_feedback::feedback_apply,
        crate::api_feedback::feedback_auto,
        crate::api_feedback::feedback_reset,
        crate::api_distill::distill_run,
        crate::api_feedback::feedback_suggestions,
        crate::api_feedback::feedback_updates,
        crate::api_feedback::feedback_policy,
        crate::api_feedback::feedback_versions,
        crate::api_feedback::feedback_rollback,
        crate::api_goldens::goldens_list,
        crate::api_goldens::goldens_add,
        crate::api_goldens::goldens_run,
        crate::api_experiments::experiments_define,
        crate::api_experiments::experiments_run,
        crate::api_experiments::experiments_activate,
        crate::api_experiments::experiments_list,
        crate::api_experiments::experiments_scoreboard,
        crate::api_experiments::experiments_winners,
        crate::api_experiments::experiments_start,
        crate::api_experiments::experiments_stop,
        crate::api_experiments::experiments_assign,
        crate::api_hierarchy::hierarchy_state,
        crate::api_hierarchy::hierarchy_role_set,
        crate::api_hierarchy::hierarchy_hello,
        crate::api_hierarchy::hierarchy_offer,
        crate::api_hierarchy::hierarchy_accept,
        crate::api_self_model::self_model_propose,
        crate::api_self_model::self_model_apply,
        crate::api_projects::projects_list,
        crate::api_projects::projects_create,
        crate::api_projects::projects_tree,
        crate::api_projects::projects_notes_get,
        crate::api_projects::projects_notes_set,
        crate::api_projects::projects_file_get,
        crate::api_projects::projects_file_set,
        crate::api_projects::projects_file_patch,
        crate::api_projects::projects_import,
        crate::api_research_watcher::research_watcher_approve,
        crate::api_research_watcher::research_watcher_archive,
        crate::api_review::memory_quarantine_get,
        crate::api_review::memory_quarantine_queue,
        crate::api_review::memory_quarantine_admit,
        crate::api_review::world_diffs_get,
        crate::api_review::world_diffs_queue,
        crate::api_review::world_diffs_decision,
        crate::api_staging::staging_action_approve,
        crate::api_staging::staging_action_deny,
    ),
    components(
        schemas(
            HealthOk,
            HttpInfo,
            AboutCounts,
            PerfPreset,
            AboutResponse,
            crate::governor::Hints,
            crate::api_hierarchy::RoleRequest,
            crate::api_self_model::SelfModelProposeRequest,
            crate::api_self_model::SelfModelApplyRequest,
            crate::api_projects::ProjectCreateRequest,
            crate::api_projects::ProjectFileWrite,
            crate::api_projects::ProjectPatchRequest,
            crate::api_projects::ProjectImportRequest,
            crate::api_research_watcher::WatcherDecision,
            crate::api_staging::StagingDecision,
            crate::api_tools::ToolRunRequest,
            crate::feedback::FeedbackState,
            crate::api_feedback::FeedbackSignalRequest,
            crate::api_feedback::FeedbackApplyRequest,
            crate::api_feedback::FeedbackAutoRequest,
            crate::api_feedback::FeedbackUpdatesQuery,
            crate::api_feedback::FeedbackRollbackQuery,
            crate::feedback::FeedbackSignal,
            crate::feedback::Suggestion,
            crate::cluster::ClusterNode,
            crate::goldens::GoldenItem,
            crate::goldens::EvalOptions,
            crate::goldens::EvalSummary,
            crate::api_goldens::GoldensListQuery,
            crate::api_goldens::GoldensAddRequest,
            crate::api_goldens::GoldensRunRequest,
            crate::experiments::VariantCfg,
            crate::experiments::Experiment,
            crate::experiments::WinnerInfo,
            crate::experiments::ScoreEntry,
            crate::experiments::ScoreRow,
            crate::experiments::RunPlan,
            crate::experiments::RunOutcome,
            crate::experiments::RunOutcomeVariant,
            crate::api_experiments::ExperimentDefineRequest,
            crate::api_experiments::ExperimentRunRequest,
            crate::api_experiments::ExperimentActivateRequest,
            crate::api_experiments::ExperimentStartRequest,
            crate::api_experiments::ExperimentStopRequest,
            crate::api_experiments::ExperimentAssignRequest,
            crate::review::MemoryQuarantineRequest,
            crate::review::MemoryQuarantineAdmit,
            crate::review::WorldDiffQueueRequest,
            crate::review::WorldDiffDecision
        )
    ),
    tags(
        (name = "Meta", description = "Service metadata and health"),
        (name = "State", description = "Read‑models (actions, models, egress, episodes)"),
        (name = "Events", description = "Server‑Sent Events stream"),
        (name = "Models", description = "Model steward admin endpoints"),
        (name = "Admin/Introspect", description = "Administrative probes and telemetry"),
        (name = "Admin/Hierarchy", description = "Hierarchy coordination endpoints"),
        (name = "Admin/SelfModel", description = "Self-model lifecycle endpoints"),
        (name = "Admin/Projects", description = "Project workspace management"),
        (name = "Admin/Review", description = "Memory quarantine and world diff review queue"),
        (name = "Admin/State", description = "Administrative state read-models"),
        (name = "Admin/Feedback", description = "Feedback engine signals and suggestions"),
        (name = "Admin/Tools", description = "Tool Forge and action cache"),
        (name = "Research", description = "Research watcher workflow"),
        (name = "Staging", description = "Human-in-the-loop approvals queue")
    ),
    info(description = "Unified ARW server API surface (headless-first)."),
    modifiers(&OperationIdModifier)
)]
pub struct ApiDoc;

struct OperationIdModifier;

impl utoipa::Modify for OperationIdModifier {
    fn modify(&self, doc: &mut utoipa::openapi::OpenApi) {
        apply_operation_ids(doc);
    }
}

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

fn normalize_operation_id(raw: &str) -> String {
    let mut snake = to_snake_case(raw);
    if !snake.ends_with("_doc") {
        snake.push_str("_doc");
    }
    snake
}

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
