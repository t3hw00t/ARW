use std::mem;

use axum::{
    handler::Handler,
    routing::{delete, get, patch, post, put},
    Router,
};
use serde_json::{json, Value};

use crate::{api, AppState};

#[derive(Copy, Clone)]
pub(crate) enum Stability {
    Stable,
    Beta,
    Experimental,
}

impl Stability {
    fn as_str(self) -> &'static str {
        match self {
            Stability::Stable => "stable",
            Stability::Beta => "beta",
            Stability::Experimental => "experimental",
        }
    }
}

pub(crate) struct RouterBuilder {
    router: Router<AppState>,
    endpoints: Vec<String>,
    endpoints_meta: Vec<Value>,
}

impl RouterBuilder {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            endpoints: Vec::new(),
            endpoints_meta: Vec::new(),
        }
    }

    fn record(&mut self, method: &str, path: &'static str, stability: Option<Stability>) {
        self.endpoints.push(format!("{} {}", method, path));
        if let Some(stability) = stability {
            self.endpoints_meta.push(json!({
                "method": method,
                "path": path,
                "stability": stability.as_str(),
            }));
        }
    }

    pub fn route_get<H, T>(
        &mut self,
        path: &'static str,
        handler: H,
        stability: Option<Stability>,
    ) -> &mut Self
    where
        H: Handler<T, AppState> + Clone + 'static,
        T: Send + 'static,
    {
        self.record("GET", path, stability);
        let router = mem::take(&mut self.router);
        self.router = router.route(path, get(handler));
        self
    }

    pub fn route_post<H, T>(
        &mut self,
        path: &'static str,
        handler: H,
        stability: Option<Stability>,
    ) -> &mut Self
    where
        H: Handler<T, AppState> + Clone + 'static,
        T: Send + 'static,
    {
        self.record("POST", path, stability);
        let router = mem::take(&mut self.router);
        self.router = router.route(path, post(handler));
        self
    }

    pub fn route_put<H, T>(
        &mut self,
        path: &'static str,
        handler: H,
        stability: Option<Stability>,
    ) -> &mut Self
    where
        H: Handler<T, AppState> + Clone + 'static,
        T: Send + 'static,
    {
        self.record("PUT", path, stability);
        let router = mem::take(&mut self.router);
        self.router = router.route(path, put(handler));
        self
    }

    pub fn route_patch<H, T>(
        &mut self,
        path: &'static str,
        handler: H,
        stability: Option<Stability>,
    ) -> &mut Self
    where
        H: Handler<T, AppState> + Clone + 'static,
        T: Send + 'static,
    {
        self.record("PATCH", path, stability);
        let router = mem::take(&mut self.router);
        self.router = router.route(path, patch(handler));
        self
    }

    pub fn route_delete<H, T>(
        &mut self,
        path: &'static str,
        handler: H,
        stability: Option<Stability>,
    ) -> &mut Self
    where
        H: Handler<T, AppState> + Clone + 'static,
        T: Send + 'static,
    {
        self.record("DELETE", path, stability);
        let router = mem::take(&mut self.router);
        self.router = router.route(path, delete(handler));
        self
    }

    pub fn build(self) -> (Router<AppState>, Vec<String>, Vec<Value>) {
        (self.router, self.endpoints, self.endpoints_meta)
    }
}

pub(crate) mod paths {
    pub const HEALTHZ: &str = "/healthz";
    pub const ABOUT: &str = "/about";
    pub const EVENTS: &str = "/events";
    pub const ADMIN_EVENTS_JOURNAL: &str = "/admin/events/journal";
    pub const METRICS: &str = "/metrics";
    pub const ACTIONS: &str = "/actions";
    pub const ACTIONS_ID: &str = "/actions/{id}";
    pub const ACTIONS_ID_STATE: &str = "/actions/{id}/state";
    pub const STATE_EPISODES: &str = "/state/episodes";
    pub const STATE_EPISODE_SNAPSHOT: &str = "/state/episode/{id}/snapshot";
    pub const STATE_ROUTE_STATS: &str = "/state/route_stats";
    pub const STATE_ACTIONS: &str = "/state/actions";
    pub const STATE_CONTRIBS: &str = "/state/contributions";
    pub const STATE_RESEARCH_WATCHER: &str = "/state/research_watcher";
    pub const STATE_STAGING_ACTIONS: &str = "/state/staging/actions";
    pub const STATE_TRAINING_TELEMETRY: &str = "/state/training/telemetry";
    pub const STATE_TRAINING_ACTIONS: &str = "/state/training/actions";
    pub const STATE_RUNTIME_MATRIX: &str = "/state/runtime_matrix";
    pub const STATE_RUNTIME_SUPERVISOR: &str = "/state/runtime_supervisor";
    pub const STATE_CONTEXT_CASCADE: &str = "/state/context/cascade";
    pub const STATE_TASKS: &str = "/state/tasks";
    pub const STATE_SELF: &str = "/state/self";
    pub const STATE_SELF_AGENT: &str = "/state/self/{agent}";
    pub const STATE_EXPERIMENTS: &str = "/state/experiments";
    pub const LEASES: &str = "/leases";
    pub const STATE_LEASES: &str = "/state/leases";
    pub const STATE_EGRESS: &str = "/state/egress";
    pub const STATE_EGRESS_SETTINGS: &str = "/state/egress/settings";
    pub const EGRESS_SETTINGS: &str = "/egress/settings";
    pub const EGRESS_PREVIEW: &str = "/egress/preview";
    pub const STATE_POLICY: &str = "/state/policy";
    pub const STATE_POLICY_CAPSULES: &str = "/state/policy/capsules";
    pub const STATE_IDENTITY: &str = "/state/identity";
    pub const POLICY_RELOAD: &str = "/policy/reload";
    pub const POLICY_SIMULATE: &str = "/policy/simulate";
    pub const POLICY_GUARDRAILS_APPLY: &str = "/policy/guardrails/apply";
    pub const ADMIN_POLICY_CAPSULES_TEARDOWN: &str = "/admin/policy/capsules/teardown";
    pub const STATE_MODELS: &str = "/state/models";
    pub const STATE_MODELS_METRICS: &str = "/state/models_metrics";
    pub const STATE_OBSERVATIONS: &str = "/state/observations";
    pub const STATE_BELIEFS: &str = "/state/beliefs";
    pub const STATE_INTENTS: &str = "/state/intents";
    pub const STATE_CRASHLOG: &str = "/state/crashlog";
    pub const STATE_SCREENSHOTS: &str = "/state/screenshots";
    pub const STATE_SERVICE_HEALTH: &str = "/state/service_health";
    pub const STATE_SERVICE_STATUS: &str = "/state/service_status";
    pub const STATE_GUARDRAILS_METRICS: &str = "/state/guardrails_metrics";
    pub const STATE_AUTONOMY_LANES: &str = "/state/autonomy/lanes";
    pub const STATE_AUTONOMY_LANE: &str = "/state/autonomy/lanes/{lane}";
    pub const STATE_CLUSTER: &str = "/state/cluster";
    pub const STATE_WORLD: &str = "/state/world";
    pub const STATE_WORLD_SELECT: &str = "/state/world/select";
    pub const STATE_PROJECTS: &str = "/state/projects";
    pub const STATE_PROJECTS_TREE: &str = "/state/projects/{proj}/tree";
    pub const STATE_PROJECTS_NOTES: &str = "/state/projects/{proj}/notes";
    pub const STATE_PROJECTS_FILE: &str = "/state/projects/{proj}/file";
    pub const STATE_MODELS_HASHES: &str = "/state/models_hashes";
    pub const STATE_MEMORY: &str = "/state/memory";
    pub const PROJECTS: &str = "/projects";
    pub const PROJECTS_NOTES: &str = "/projects/{proj}/notes";
    pub const PROJECTS_FILE: &str = "/projects/{proj}/file";
    pub const PROJECTS_IMPORT: &str = "/projects/{proj}/import";
    pub const PROJECTS_SNAPSHOT: &str = "/projects/{proj}/snapshot";
    pub const PROJECTS_SNAPSHOTS: &str = "/projects/{proj}/snapshots";
    pub const PROJECTS_SNAPSHOT_RESTORE: &str = "/projects/{proj}/snapshots/{snapshot}/restore";
    pub const SPEC_OPENAPI: &str = "/spec/openapi.yaml";
    pub const SPEC_ASYNCAPI: &str = "/spec/asyncapi.yaml";
    pub const SPEC_MCP: &str = "/spec/mcp-tools.json";
    pub const SPEC_SCHEMA: &str = "/spec/schemas/{file}";
    pub const SPEC_INDEX: &str = "/spec/index.json";
    pub const SPEC_HEALTH: &str = "/spec/health";
    pub const ADMIN_DEBUG: &str = "/admin/debug";
    pub const ADMIN_MODELS: &str = "/admin/models";
    pub const ADMIN_MODELS_SUMMARY: &str = "/admin/models/summary";
    pub const ADMIN_MODELS_REFRESH: &str = "/admin/models/refresh";
    pub const ADMIN_MODELS_SAVE: &str = "/admin/models/save";
    pub const ADMIN_MODELS_LOAD: &str = "/admin/models/load";
    pub const ADMIN_MODELS_ADD: &str = "/admin/models/add";
    pub const ADMIN_MODELS_REMOVE: &str = "/admin/models/remove";
    pub const ADMIN_MODELS_DEFAULT: &str = "/admin/models/default";
    pub const ADMIN_MODELS_CONCURRENCY: &str = "/admin/models/concurrency";
    pub const ADMIN_MODELS_DOWNLOAD: &str = "/admin/models/download";
    pub const ADMIN_MODELS_DOWNLOAD_CANCEL: &str = "/admin/models/download/cancel";
    pub const ADMIN_MODELS_CAS_GC: &str = "/admin/models/cas_gc";
    pub const ADMIN_MODELS_BY_HASH: &str = "/admin/models/by-hash/{sha256}";
    pub const ADMIN_MODELS_JOBS: &str = "/admin/models/jobs";
    pub const ADMIN_AUTONOMY_LANE_PAUSE: &str = "/admin/autonomy/{lane}/pause";
    pub const ADMIN_AUTONOMY_LANE_STOP: &str = "/admin/autonomy/{lane}/stop";
    pub const ADMIN_AUTONOMY_LANE_RESUME: &str = "/admin/autonomy/{lane}/resume";
    pub const ADMIN_AUTONOMY_LANE_JOBS: &str = "/admin/autonomy/{lane}/jobs";
    pub const ADMIN_AUTONOMY_LANE_BUDGETS: &str = "/admin/autonomy/{lane}/budgets";
    pub const ADMIN_TOOLS: &str = "/admin/tools";
    pub const ADMIN_TOOLS_RUN: &str = "/admin/tools/run";
    pub const ADMIN_TOOLS_CACHE_STATS: &str = "/admin/tools/cache_stats";
    pub const ADMIN_GOVERNOR_PROFILE: &str = "/admin/governor/profile";
    pub const ADMIN_GOVERNOR_HINTS: &str = "/admin/governor/hints";
    pub const ADMIN_MEMORY_QUARANTINE: &str = "/admin/memory/quarantine";
    pub const ADMIN_MEMORY_QUARANTINE_ADMIT: &str = "/admin/memory/quarantine/admit";
    pub const ADMIN_MEMORY: &str = "/admin/memory";
    pub const ADMIN_MEMORY_APPLY: &str = "/admin/memory/apply";
    pub const ADMIN_WORLD_DIFFS: &str = "/admin/world_diffs";
    pub const ADMIN_WORLD_DIFFS_QUEUE: &str = "/admin/world_diffs/queue";
    pub const ADMIN_WORLD_DIFFS_DECISION: &str = "/admin/world_diffs/decision";
    pub const ADMIN_PROBE: &str = "/admin/probe";
    pub const ADMIN_PROBE_HW: &str = "/admin/probe/hw";
    pub const ADMIN_PROBE_METRICS: &str = "/admin/probe/metrics";
    pub const ADMIN_HIERARCHY_STATE: &str = "/admin/hierarchy/state";
    pub const ADMIN_HIERARCHY_ROLE: &str = "/admin/hierarchy/role";
    pub const ADMIN_HIERARCHY_HELLO: &str = "/admin/hierarchy/hello";
    pub const ADMIN_HIERARCHY_OFFER: &str = "/admin/hierarchy/offer";
    pub const ADMIN_HIERARCHY_ACCEPT: &str = "/admin/hierarchy/accept";
    pub const ADMIN_SELF_MODEL_PROPOSE: &str = "/admin/self_model/propose";
    pub const ADMIN_SELF_MODEL_APPLY: &str = "/admin/self_model/apply";
    pub const ADMIN_UI_MODELS: &str = "/admin/ui/models";
    pub const ADMIN_UI_AGENTS: &str = "/admin/ui/agents";
    pub const ADMIN_UI_PROJECTS: &str = "/admin/ui/projects";
    pub const ADMIN_UI_FLOWS: &str = "/admin/ui/flows";
    pub const ADMIN_UI_TOKENS: &str = "/admin/ui/assets/tokens.css";
    pub const ADMIN_UI_KIT: &str = "/admin/ui/assets/ui-kit.css";
    pub const ADMIN_UI_PAGES: &str = "/admin/ui/assets/pages.css";
    pub const ADMIN_UI_MODELS_JS: &str = "/admin/ui/assets/models.js";
    pub const ADMIN_UI_AGENTS_JS: &str = "/admin/ui/assets/agents.js";
    pub const ADMIN_UI_PROJECTS_JS: &str = "/admin/ui/assets/projects.js";
    pub const ADMIN_UI_DEBUG_JS: &str = "/admin/ui/assets/debug.js";
    pub const ADMIN_UI_DEBUG_CORE_JS: &str = "/admin/ui/assets/debug-core.js";
    pub const CATALOG_INDEX: &str = "/catalog/index";
    pub const CATALOG_HEALTH: &str = "/catalog/health";
    pub const ADMIN_RPU_TRUST: &str = "/admin/rpu/trust";
    pub const ADMIN_RPU_RELOAD: &str = "/admin/rpu/reload";
    pub const ADMIN_FEEDBACK_STATE: &str = "/admin/feedback/state";
    pub const ADMIN_FEEDBACK_SIGNAL: &str = "/admin/feedback/signal";
    pub const ADMIN_FEEDBACK_ANALYZE: &str = "/admin/feedback/analyze";
    pub const ADMIN_FEEDBACK_APPLY: &str = "/admin/feedback/apply";
    pub const ADMIN_FEEDBACK_AUTO: &str = "/admin/feedback/auto";
    pub const ADMIN_FEEDBACK_RESET: &str = "/admin/feedback/reset";
    pub const ADMIN_DISTILL: &str = "/admin/distill";
    pub const ADMIN_FEEDBACK_SUGGESTIONS: &str = "/admin/feedback/suggestions";
    pub const ADMIN_FEEDBACK_UPDATES: &str = "/admin/feedback/updates";
    pub const ADMIN_FEEDBACK_POLICY: &str = "/admin/feedback/policy";
    pub const ADMIN_FEEDBACK_VERSIONS: &str = "/admin/feedback/versions";
    pub const ADMIN_FEEDBACK_ROLLBACK: &str = "/admin/feedback/rollback";
    pub const ADMIN_EXPERIMENTS_DEFINE: &str = "/admin/experiments/define";
    pub const ADMIN_EXPERIMENTS_RUN: &str = "/admin/experiments/run";
    pub const ADMIN_EXPERIMENTS_ACTIVATE: &str = "/admin/experiments/activate";
    pub const ADMIN_EXPERIMENTS_LIST: &str = "/admin/experiments/list";
    pub const ADMIN_EXPERIMENTS_SCOREBOARD: &str = "/admin/experiments/scoreboard";
    pub const ADMIN_EXPERIMENTS_WINNERS: &str = "/admin/experiments/winners";
    pub const ADMIN_EXPERIMENTS_START: &str = "/admin/experiments/start";
    pub const ADMIN_EXPERIMENTS_STOP: &str = "/admin/experiments/stop";
    pub const ADMIN_EXPERIMENTS_ASSIGN: &str = "/admin/experiments/assign";
    pub const ADMIN_GOLDENS_LIST: &str = "/admin/goldens/list";
    pub const ADMIN_GOLDENS_ADD: &str = "/admin/goldens/add";
    pub const ADMIN_GOLDENS_RUN: &str = "/admin/goldens/run";
    pub const RESEARCH_WATCHER_APPROVE: &str = "/research_watcher/{id}/approve";
    pub const RESEARCH_WATCHER_ARCHIVE: &str = "/research_watcher/{id}/archive";
    pub const STAGING_ACTION_APPROVE: &str = "/staging/actions/{id}/approve";
    pub const STAGING_ACTION_DENY: &str = "/staging/actions/{id}/deny";
    pub const ADMIN_CHAT: &str = "/admin/chat";
    pub const ADMIN_CHAT_SEND: &str = "/admin/chat/send";
    pub const ADMIN_CHAT_CLEAR: &str = "/admin/chat/clear";
    pub const ADMIN_CHAT_STATUS: &str = "/admin/chat/status";
}

pub(crate) fn build_router() -> (Router<AppState>, Vec<String>, Vec<Value>) {
    let mut builder = RouterBuilder::new();
    builder.route_get(paths::HEALTHZ, api::meta::healthz, Some(Stability::Stable));
    builder.route_get(paths::ABOUT, api::meta::about, Some(Stability::Stable));
    builder.route_get(
        "/shutdown",
        api::meta::shutdown,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ACTIONS,
        api::actions::actions_submit,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ACTIONS_ID,
        api::actions::actions_get,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ACTIONS_ID_STATE,
        api::actions::actions_state_set,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::EVENTS,
        api::events::events_sse,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::ADMIN_EVENTS_JOURNAL,
        api::events::events_journal,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::METRICS,
        api::metrics::metrics_prometheus,
        Some(Stability::Stable),
    );
    register_admin_ui_routes(&mut builder);
    register_admin_ops_routes(&mut builder);
    builder.route_get(
        paths::STATE_MODELS_HASHES,
        api::models::state_models_hashes,
        Some(Stability::Beta),
    );
    register_admin_management_routes(&mut builder);
    builder.route_get(
        paths::STATE_EPISODES,
        api::state::state_episodes,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_EPISODE_SNAPSHOT,
        api::state::state_episode_snapshot,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_PROJECTS,
        api::projects::state_projects_list,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::PROJECTS,
        api::projects::projects_create_unified,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_PROJECTS_TREE,
        api::projects::state_projects_tree,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_PROJECTS_NOTES,
        api::projects::state_projects_notes,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_PROJECTS_FILE,
        api::projects::state_projects_file_get,
        Some(Stability::Beta),
    );
    builder.route_put(
        paths::PROJECTS_NOTES,
        api::projects::projects_notes_put,
        Some(Stability::Beta),
    );
    builder.route_put(
        paths::PROJECTS_FILE,
        api::projects::projects_file_put,
        Some(Stability::Beta),
    );
    builder.route_patch(
        paths::PROJECTS_FILE,
        api::projects::projects_file_patch_unified,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::PROJECTS_IMPORT,
        api::projects::projects_import_unified,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::PROJECTS_SNAPSHOT,
        api::projects::projects_snapshot_create,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::PROJECTS_SNAPSHOTS,
        api::projects::projects_snapshots_list,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::PROJECTS_SNAPSHOT_RESTORE,
        api::projects::projects_snapshot_restore,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_ROUTE_STATS,
        api::state::state_route_stats,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_TASKS,
        api::state::state_tasks,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_OBSERVATIONS,
        api::state::state_observations,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_BELIEFS,
        api::state::state_beliefs,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_INTENTS,
        api::state::state_intents,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_CRASHLOG,
        api::state::state_crashlog,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_SCREENSHOTS,
        api::state::state_screenshots,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_SERVICE_HEALTH,
        api::state::state_service_health,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_SERVICE_STATUS,
        api::state::state_service_status,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_GUARDRAILS_METRICS,
        api::state::state_guardrails_metrics,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_AUTONOMY_LANES,
        api::autonomy::state_autonomy_lanes,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_AUTONOMY_LANE,
        api::autonomy::state_autonomy_lane,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_ACTIONS,
        api::state::state_actions,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_CLUSTER,
        api::state::state_cluster,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_WORLD,
        api::state::state_world,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_WORLD_SELECT,
        api::state::state_world_select,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_MODELS_METRICS,
        api::models::state_models_metrics,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_CONTRIBS,
        api::state::state_contributions,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_RESEARCH_WATCHER,
        api::state::state_research_watcher,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_STAGING_ACTIONS,
        api::state::state_staging_actions,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_TRAINING_TELEMETRY,
        api::state::state_training_telemetry,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_TRAINING_ACTIONS,
        api::state::state_training_actions,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::LEASES,
        api::leases::leases_create,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_LEASES,
        api::leases::state_leases,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_EGRESS,
        api::state::state_egress,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_EGRESS_SETTINGS,
        api::egress_settings::state_egress_settings,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::EGRESS_SETTINGS,
        api::egress_settings::egress_settings_update,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::EGRESS_PREVIEW,
        api::egress::egress_preview,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_POLICY,
        api::policy::state_policy,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_IDENTITY,
        api::state::state_identity,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_POLICY_CAPSULES,
        api::state::state_policy_capsules,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::POLICY_RELOAD,
        api::policy::policy_reload,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::POLICY_SIMULATE,
        api::policy::policy_simulate,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::POLICY_GUARDRAILS_APPLY,
        api::policy::policy_guardrails_apply,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_MODELS,
        api::state::state_models,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::SPEC_OPENAPI,
        api::spec::spec_openapi,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::SPEC_ASYNCAPI,
        api::spec::spec_asyncapi,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::SPEC_MCP,
        api::spec::spec_mcp,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::SPEC_SCHEMA,
        api::spec::spec_schema,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::SPEC_INDEX,
        api::spec::spec_index,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::SPEC_HEALTH,
        api::spec::spec_health,
        Some(Stability::Stable),
    );
    builder.route_get(
        "/spec/openapi.gen.yaml",
        api::spec::spec_openapi_gen,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::CATALOG_INDEX,
        api::spec::catalog_index,
        Some(Stability::Stable),
    );
    builder.route_get(
        paths::CATALOG_HEALTH,
        api::spec::catalog_health,
        Some(Stability::Stable),
    );
    register_admin_rpu_routes(&mut builder);
    builder.route_get("/logic-units", api::logic_units::logic_units_list, None);
    builder.route_get(
        "/state/logic_units",
        api::logic_units::state_logic_units,
        None,
    );
    builder.route_post(
        "/logic-units/install",
        api::logic_units::logic_units_install,
        None,
    );
    builder.route_post(
        "/logic-units/apply",
        api::logic_units::logic_units_apply,
        None,
    );
    builder.route_post(
        "/logic-units/revert",
        api::logic_units::logic_units_revert,
        None,
    );
    builder.route_get("/state/config", api::config::state_config, None);
    builder.route_post("/patch/apply", api::config::patch_apply, None);
    builder.route_post("/patch/revert", api::config::patch_revert, None);
    builder.route_get(
        "/state/config/snapshots",
        api::config::state_config_snapshots,
        None,
    );
    builder.route_get(
        "/state/config/snapshots/{id}",
        api::config::state_config_snapshot_get,
        None,
    );
    builder.route_post("/patch/validate", api::config::patch_validate, None);
    builder.route_get("/state/schema_map", api::config::state_schema_map, None);
    builder.route_post("/patch/infer_schema", api::config::patch_infer_schema, None);
    builder.route_get(
        paths::STATE_RUNTIME_MATRIX,
        api::state::state_runtime_matrix,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_RUNTIME_SUPERVISOR,
        api::state::state_runtime_supervisor,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::STATE_EXPERIMENTS,
        api::state::state_experiments,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_SELF,
        api::state::state_self_list,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::STATE_SELF_AGENT,
        api::state::state_self_get,
        Some(Stability::Beta),
    );
    register_admin_self_model_routes(&mut builder);
    builder.route_post("/context/assemble", api::context::context_assemble, None);
    builder.route_post("/context/rehydrate", api::context::context_rehydrate, None);
    builder.route_get(
        paths::STATE_CONTEXT_CASCADE,
        api::context::state_context_cascade,
        Some(Stability::Experimental),
    );
    builder.route_get("/state/connectors", api::connectors::state_connectors, None);
    builder.route_post(
        "/connectors/register",
        api::connectors::connector_register,
        None,
    );
    builder.route_post(
        "/connectors/token",
        api::connectors::connector_token_set,
        None,
    );
    builder.route_get(
        paths::STATE_MEMORY,
        api::memory::state_memory_stream,
        Some(Stability::Beta),
    );
    builder.route_get(
        "/state/memory/recent",
        api::memory::state_memory_recent,
        None,
    );
    register_admin_memory_routes(&mut builder);
    builder.route_get(
        "/orchestrator/mini_agents",
        api::orchestrator::orchestrator_mini_agents,
        None,
    );
    builder.route_post(
        "/orchestrator/mini_agents/start_training",
        api::orchestrator::orchestrator_start_training,
        None,
    );
    builder.route_post(
        "/orchestrator/runtimes/{id}/restore",
        api::orchestrator::orchestrator_runtime_restore,
        None,
    );
    builder.route_get(
        "/state/orchestrator/jobs",
        api::orchestrator::state_orchestrator_jobs,
        None,
    );
    builder.route_post(
        paths::RESEARCH_WATCHER_APPROVE,
        api::research_watcher::research_watcher_approve,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::RESEARCH_WATCHER_ARCHIVE,
        api::research_watcher::research_watcher_archive,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::STAGING_ACTION_APPROVE,
        api::staging::staging_action_approve,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::STAGING_ACTION_DENY,
        api::staging::staging_action_deny,
        Some(Stability::Experimental),
    );
    register_admin_chat_routes(&mut builder);
    builder.build()
}

fn register_admin_ui_routes(builder: &mut RouterBuilder) {
    builder.route_get(paths::ADMIN_DEBUG, api::ui::debug_ui, Some(Stability::Beta));
    builder.route_get(
        paths::ADMIN_UI_MODELS,
        api::ui::models_ui,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_AGENTS,
        api::ui::agents_ui,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_PROJECTS,
        api::ui::projects_ui,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_FLOWS,
        api::ui::flows_ui,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_TOKENS,
        api::ui::ui_tokens_css,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_KIT,
        api::ui::ui_kit_css,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_PAGES,
        api::ui::ui_pages_css,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_MODELS_JS,
        api::ui::ui_models_js,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_AGENTS_JS,
        api::ui::ui_agents_js,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_PROJECTS_JS,
        api::ui::ui_projects_js,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_DEBUG_JS,
        api::ui::ui_debug_js,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_UI_DEBUG_CORE_JS,
        api::ui::ui_debug_core_js,
        Some(Stability::Beta),
    );
}

fn register_admin_ops_routes(builder: &mut RouterBuilder) {
    builder.route_get(
        paths::ADMIN_MODELS_SUMMARY,
        api::models::models_summary,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_MODELS,
        api::models::models_list,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_REFRESH,
        api::models::models_refresh,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_SAVE,
        api::models::models_save,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_LOAD,
        api::models::models_load,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_ADD,
        api::models::models_add,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_REMOVE,
        api::models::models_remove,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_MODELS_DEFAULT,
        api::models::models_default_get,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_DEFAULT,
        api::models::models_default_set,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_MODELS_CONCURRENCY,
        api::models::models_concurrency_get,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MODELS_CONCURRENCY,
        api::models::models_concurrency_set,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_MODELS_JOBS,
        api::models::models_jobs,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_AUTONOMY_LANE_PAUSE,
        api::autonomy::autonomy_pause,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_AUTONOMY_LANE_STOP,
        api::autonomy::autonomy_stop,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_AUTONOMY_LANE_RESUME,
        api::autonomy::autonomy_resume,
        Some(Stability::Experimental),
    );
    builder.route_delete(
        paths::ADMIN_AUTONOMY_LANE_JOBS,
        api::autonomy::autonomy_jobs_clear,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_AUTONOMY_LANE_BUDGETS,
        api::autonomy::autonomy_budgets_update,
        Some(Stability::Experimental),
    );
}

fn register_admin_management_routes(builder: &mut RouterBuilder) {
    builder.route_get(
        paths::ADMIN_TOOLS,
        api::tools::tools_list,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_TOOLS_RUN,
        api::tools::tools_run,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_TOOLS_CACHE_STATS,
        api::tools::tools_cache_stats,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_MEMORY_QUARANTINE,
        api::review::memory_quarantine_get,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_MEMORY_QUARANTINE,
        api::review::memory_quarantine_queue,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_MEMORY_QUARANTINE_ADMIT,
        api::review::memory_quarantine_admit,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_WORLD_DIFFS,
        api::review::world_diffs_get,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_WORLD_DIFFS_QUEUE,
        api::review::world_diffs_queue,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_WORLD_DIFFS_DECISION,
        api::review::world_diffs_decision,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_GOVERNOR_PROFILE,
        api::governor::governor_profile_get,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_GOVERNOR_PROFILE,
        api::governor::governor_profile_set,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_GOVERNOR_HINTS,
        api::governor::governor_hints_get,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_GOVERNOR_HINTS,
        api::governor::governor_hints_set,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_FEEDBACK_STATE,
        api::feedback::feedback_state,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_SIGNAL,
        api::feedback::feedback_signal,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_ANALYZE,
        api::feedback::feedback_analyze,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_APPLY,
        api::feedback::feedback_apply,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_AUTO,
        api::feedback::feedback_auto,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_RESET,
        api::feedback::feedback_reset,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_DISTILL,
        api::distill::distill_run,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_FEEDBACK_SUGGESTIONS,
        api::feedback::feedback_suggestions,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_FEEDBACK_UPDATES,
        api::feedback::feedback_updates,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_FEEDBACK_POLICY,
        api::feedback::feedback_policy,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_FEEDBACK_VERSIONS,
        api::feedback::feedback_versions,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_FEEDBACK_ROLLBACK,
        api::feedback::feedback_rollback,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_POLICY_CAPSULES_TEARDOWN,
        api::policy::policy_capsules_teardown,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_DEFINE,
        api::experiments::experiments_define,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_RUN,
        api::experiments::experiments_run,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_ACTIVATE,
        api::experiments::experiments_activate,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_EXPERIMENTS_LIST,
        api::experiments::experiments_list,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_EXPERIMENTS_SCOREBOARD,
        api::experiments::experiments_scoreboard,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_EXPERIMENTS_WINNERS,
        api::experiments::experiments_winners,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_START,
        api::experiments::experiments_start,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_STOP,
        api::experiments::experiments_stop,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_EXPERIMENTS_ASSIGN,
        api::experiments::experiments_assign,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_GOLDENS_LIST,
        api::goldens::goldens_list,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_GOLDENS_ADD,
        api::goldens::goldens_add,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_GOLDENS_RUN,
        api::goldens::goldens_run,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_HIERARCHY_STATE,
        api::hierarchy::hierarchy_state,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_HIERARCHY_ROLE,
        api::hierarchy::hierarchy_role_set,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_HIERARCHY_HELLO,
        api::hierarchy::hierarchy_hello,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_HIERARCHY_OFFER,
        api::hierarchy::hierarchy_offer,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_HIERARCHY_ACCEPT,
        api::hierarchy::hierarchy_accept,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_PROBE,
        api::probe::probe_effective_paths,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_PROBE_HW,
        api::probe::probe_hw,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_PROBE_METRICS,
        api::probe::probe_metrics,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_MODELS_DOWNLOAD,
        api::models::models_download,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_MODELS_DOWNLOAD_CANCEL,
        api::models::models_download_cancel,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_MODELS_CAS_GC,
        api::models::models_cas_gc,
        Some(Stability::Experimental),
    );
    builder.route_get(
        paths::ADMIN_MODELS_BY_HASH,
        api::models::models_blob_by_hash,
        Some(Stability::Experimental),
    );
}

fn register_admin_rpu_routes(builder: &mut RouterBuilder) {
    builder.route_get(
        paths::ADMIN_RPU_TRUST,
        api::rpu::rpu_trust_get,
        Some(Stability::Experimental),
    );
    builder.route_post(
        paths::ADMIN_RPU_RELOAD,
        api::rpu::rpu_reload_post,
        Some(Stability::Experimental),
    );
}

fn register_admin_self_model_routes(builder: &mut RouterBuilder) {
    builder.route_post(
        paths::ADMIN_SELF_MODEL_PROPOSE,
        api::self_model::self_model_propose,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_SELF_MODEL_APPLY,
        api::self_model::self_model_apply,
        Some(Stability::Beta),
    );
}

fn register_admin_memory_routes(builder: &mut RouterBuilder) {
    builder.route_get(
        paths::ADMIN_MEMORY,
        api::memory::admin_memory_list,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_MEMORY_APPLY,
        api::memory::admin_memory_apply,
        Some(Stability::Beta),
    );
}

fn register_admin_chat_routes(builder: &mut RouterBuilder) {
    builder.route_get(
        paths::ADMIN_CHAT,
        api::chat::chat_history,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_CHAT_SEND,
        api::chat::chat_send,
        Some(Stability::Beta),
    );
    builder.route_post(
        paths::ADMIN_CHAT_CLEAR,
        api::chat::chat_clear,
        Some(Stability::Beta),
    );
    builder.route_get(
        paths::ADMIN_CHAT_STATUS,
        api::chat::chat_status,
        Some(Stability::Beta),
    );
}
