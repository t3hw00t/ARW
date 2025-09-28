use std::sync::Arc;

use arw_events::Bus;
use arw_kernel::Kernel;
use arw_wasi::ToolHost;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    capsule_guard, chat, cluster, experiments, feedback, governor, metrics, models, policy,
    runtime, tool_cache, training,
};

type SharedConfigState = Arc<Mutex<serde_json::Value>>;
type SharedConfigHistory = Arc<Mutex<Vec<(String, serde_json::Value)>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    bus: Bus,
    kernel: Kernel,
    policy: Arc<policy::PolicyHandle>, // hot-reloadable
    host: Arc<dyn ToolHost>,
    config_state: Arc<Mutex<serde_json::Value>>, // effective config (demo)
    config_history: Arc<Mutex<Vec<(String, serde_json::Value)>>>, // snapshots
    sse_id_map: Arc<Mutex<crate::sse_cache::SseIdCache>>,
    endpoints: Arc<Vec<String>>,
    endpoints_meta: Arc<Vec<serde_json::Value>>,
    metrics: Arc<metrics::Metrics>,
    kernel_enabled: bool,
    models: Arc<models::ModelStore>,
    tool_cache: Arc<tool_cache::ToolCache>,
    governor: Arc<governor::GovernorState>,
    feedback: Arc<feedback::FeedbackHub>,
    cluster: Arc<cluster::ClusterRegistry>,
    experiments: Arc<experiments::Experiments>,
    capsules: Arc<capsule_guard::CapsuleStore>,
    chat: Arc<chat::ChatState>,
    runtime: Arc<runtime::RuntimeRegistry>,
    logic_history: Arc<training::LogicUnitHistoryStore>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bus: Bus,
        kernel: Kernel,
        policy: Arc<policy::PolicyHandle>,
        host: Arc<dyn ToolHost>,
        config_state: Arc<Mutex<serde_json::Value>>,
        config_history: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
        sse_id_map: Arc<Mutex<crate::sse_cache::SseIdCache>>,
        endpoints: Arc<Vec<String>>,
        endpoints_meta: Arc<Vec<serde_json::Value>>,
        metrics: Arc<metrics::Metrics>,
        kernel_enabled: bool,
        models: Arc<models::ModelStore>,
        tool_cache: Arc<tool_cache::ToolCache>,
        governor: Arc<governor::GovernorState>,
        feedback: Arc<feedback::FeedbackHub>,
        cluster: Arc<cluster::ClusterRegistry>,
        experiments: Arc<experiments::Experiments>,
        capsules: Arc<capsule_guard::CapsuleStore>,
        chat: Arc<chat::ChatState>,
        runtime: Arc<runtime::RuntimeRegistry>,
        logic_history: Arc<training::LogicUnitHistoryStore>,
    ) -> Self {
        Self {
            bus,
            kernel,
            policy,
            host,
            config_state,
            config_history,
            sse_id_map,
            endpoints,
            endpoints_meta,
            metrics,
            kernel_enabled,
            models,
            tool_cache,
            governor,
            feedback,
            cluster,
            experiments,
            capsules,
            chat,
            runtime,
            logic_history,
        }
    }

    pub fn kernel_enabled(&self) -> bool {
        self.kernel_enabled
    }

    pub fn kernel(&self) -> &Kernel {
        &self.kernel
    }

    pub fn kernel_if_enabled(&self) -> Option<&Kernel> {
        if self.kernel_enabled {
            Some(&self.kernel)
        } else {
            None
        }
    }

    pub fn models(&self) -> Arc<models::ModelStore> {
        self.models.clone()
    }

    pub fn tool_cache(&self) -> Arc<tool_cache::ToolCache> {
        self.tool_cache.clone()
    }

    pub fn policy(&self) -> Arc<policy::PolicyHandle> {
        self.policy.clone()
    }

    pub fn host(&self) -> Arc<dyn ToolHost> {
        self.host.clone()
    }

    pub fn metrics(&self) -> Arc<metrics::Metrics> {
        self.metrics.clone()
    }

    pub fn bus(&self) -> Bus {
        self.bus.clone()
    }

    pub fn capsules(&self) -> Arc<capsule_guard::CapsuleStore> {
        self.capsules.clone()
    }

    #[cfg(feature = "grpc")]
    pub fn sse_cache(&self) -> Arc<Mutex<crate::sse_cache::SseIdCache>> {
        self.sse_id_map.clone()
    }

    pub fn sse_ids(&self) -> Arc<Mutex<crate::sse_cache::SseIdCache>> {
        self.sse_id_map.clone()
    }

    pub fn governor(&self) -> Arc<governor::GovernorState> {
        self.governor.clone()
    }

    pub fn feedback(&self) -> Arc<feedback::FeedbackHub> {
        self.feedback.clone()
    }

    pub fn cluster(&self) -> Arc<cluster::ClusterRegistry> {
        self.cluster.clone()
    }

    pub fn experiments(&self) -> Arc<experiments::Experiments> {
        self.experiments.clone()
    }

    pub fn chat(&self) -> Arc<chat::ChatState> {
        self.chat.clone()
    }

    pub fn runtime(&self) -> Arc<runtime::RuntimeRegistry> {
        self.runtime.clone()
    }

    pub fn logic_history(&self) -> Arc<training::LogicUnitHistoryStore> {
        self.logic_history.clone()
    }

    pub fn config_state(&self) -> Arc<Mutex<serde_json::Value>> {
        self.config_state.clone()
    }

    pub fn config_history(&self) -> Arc<Mutex<Vec<(String, serde_json::Value)>>> {
        self.config_history.clone()
    }

    pub fn endpoints(&self) -> Arc<Vec<String>> {
        self.endpoints.clone()
    }

    pub fn endpoints_meta(&self) -> Arc<Vec<serde_json::Value>> {
        self.endpoints_meta.clone()
    }
}

pub(crate) struct AppStateBuilder {
    bus: Bus,
    kernel: Kernel,
    policy: Arc<policy::PolicyHandle>,
    host: Arc<dyn ToolHost>,
    kernel_enabled: bool,
    config_state: Option<SharedConfigState>,
    config_history: Option<SharedConfigHistory>,
    sse_id_map: Option<Arc<Mutex<crate::sse_cache::SseIdCache>>>,
    sse_capacity: usize,
    endpoints: Option<Arc<Vec<String>>>,
    endpoints_meta: Option<Arc<Vec<serde_json::Value>>>,
    metrics: Option<Arc<metrics::Metrics>>,
    models: Option<Arc<models::ModelStore>>,
    tool_cache: Option<Arc<tool_cache::ToolCache>>,
    governor: Option<Arc<governor::GovernorState>>,
    feedback: Option<Arc<feedback::FeedbackHub>>,
    cluster: Option<Arc<cluster::ClusterRegistry>>,
    experiments: Option<Arc<experiments::Experiments>>,
    capsules: Option<Arc<capsule_guard::CapsuleStore>>,
    chat: Option<Arc<chat::ChatState>>,
    runtime: Option<Arc<runtime::RuntimeRegistry>>,
    logic_history: Option<Arc<training::LogicUnitHistoryStore>>,
}

impl AppState {
    pub(crate) fn builder(
        bus: Bus,
        kernel: Kernel,
        policy: Arc<policy::PolicyHandle>,
        host: Arc<dyn ToolHost>,
        kernel_enabled: bool,
    ) -> AppStateBuilder {
        AppStateBuilder {
            bus,
            kernel,
            policy,
            host,
            kernel_enabled,
            config_state: None,
            config_history: None,
            sse_id_map: None,
            sse_capacity: 2048,
            endpoints: None,
            endpoints_meta: None,
            metrics: None,
            models: None,
            tool_cache: None,
            governor: None,
            feedback: None,
            cluster: None,
            experiments: None,
            capsules: None,
            chat: None,
            runtime: None,
            logic_history: None,
        }
    }
}

#[allow(dead_code)]
impl AppStateBuilder {
    pub(crate) fn with_config_state(mut self, config_state: SharedConfigState) -> Self {
        self.config_state = Some(config_state);
        self
    }

    pub(crate) fn with_config_history(mut self, config_history: SharedConfigHistory) -> Self {
        self.config_history = Some(config_history);
        self
    }

    pub(crate) fn with_sse_cache(
        mut self,
        cache: Arc<Mutex<crate::sse_cache::SseIdCache>>,
    ) -> Self {
        self.sse_id_map = Some(cache);
        self
    }

    pub(crate) fn with_sse_capacity(mut self, capacity: usize) -> Self {
        self.sse_capacity = capacity;
        self
    }

    pub(crate) fn with_endpoints(mut self, endpoints: Arc<Vec<String>>) -> Self {
        self.endpoints = Some(endpoints);
        self
    }

    pub(crate) fn with_endpoints_meta(
        mut self,
        endpoints_meta: Arc<Vec<serde_json::Value>>,
    ) -> Self {
        self.endpoints_meta = Some(endpoints_meta);
        self
    }

    pub(crate) fn with_metrics(mut self, metrics: Arc<metrics::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub(crate) fn with_models(mut self, models: Arc<models::ModelStore>) -> Self {
        self.models = Some(models);
        self
    }

    pub(crate) fn with_tool_cache(mut self, cache: Arc<tool_cache::ToolCache>) -> Self {
        self.tool_cache = Some(cache);
        self
    }

    pub(crate) fn with_governor(mut self, governor: Arc<governor::GovernorState>) -> Self {
        self.governor = Some(governor);
        self
    }

    pub(crate) fn with_feedback(mut self, feedback: Arc<feedback::FeedbackHub>) -> Self {
        self.feedback = Some(feedback);
        self
    }

    pub(crate) fn with_cluster(mut self, cluster: Arc<cluster::ClusterRegistry>) -> Self {
        self.cluster = Some(cluster);
        self
    }

    pub(crate) fn with_experiments(mut self, experiments: Arc<experiments::Experiments>) -> Self {
        self.experiments = Some(experiments);
        self
    }

    pub(crate) fn with_capsules(mut self, capsules: Arc<capsule_guard::CapsuleStore>) -> Self {
        self.capsules = Some(capsules);
        self
    }

    pub(crate) fn with_chat(mut self, chat: Arc<chat::ChatState>) -> Self {
        self.chat = Some(chat);
        self
    }

    pub(crate) fn with_runtime(mut self, runtime: Arc<runtime::RuntimeRegistry>) -> Self {
        self.runtime = Some(runtime);
        self
    }

    pub(crate) fn with_logic_history(
        mut self,
        store: Arc<training::LogicUnitHistoryStore>,
    ) -> Self {
        self.logic_history = Some(store);
        self
    }

    pub(crate) async fn build(self) -> AppState {
        let config_state = self
            .config_state
            .unwrap_or_else(|| Arc::new(Mutex::new(json!({}))));
        let config_history = self
            .config_history
            .unwrap_or_else(|| Arc::new(Mutex::new(Vec::new())));
        let sse_id_map = self.sse_id_map.unwrap_or_else(|| {
            Arc::new(Mutex::new(crate::sse_cache::SseIdCache::with_capacity(
                self.sse_capacity,
            )))
        });
        let endpoints = self
            .endpoints
            .unwrap_or_else(|| Arc::new(Vec::<String>::new()));
        let endpoints_meta = self
            .endpoints_meta
            .unwrap_or_else(|| Arc::new(Vec::<serde_json::Value>::new()));
        let metrics = self
            .metrics
            .unwrap_or_else(|| Arc::new(metrics::Metrics::default()));

        let kernel_for_models = if self.kernel_enabled {
            Some(self.kernel.clone())
        } else {
            None
        };
        let models_store = match self.models {
            Some(models) => models,
            None => {
                let store = Arc::new(models::ModelStore::new(self.bus.clone(), kernel_for_models));
                store.bootstrap().await;
                store
            }
        };
        let tool_cache = self
            .tool_cache
            .unwrap_or_else(|| Arc::new(tool_cache::ToolCache::new()));
        let governor_state = match self.governor {
            Some(state) => state,
            None => governor::GovernorState::new().await,
        };
        let feedback_hub = match self.feedback {
            Some(hub) => hub,
            None => {
                feedback::FeedbackHub::new(
                    self.bus.clone(),
                    metrics.clone(),
                    governor_state.clone(),
                )
                .await
            }
        };
        let cluster_state = self
            .cluster
            .unwrap_or_else(|| cluster::ClusterRegistry::new(self.bus.clone()));
        let experiments_state = match self.experiments {
            Some(state) => state,
            None => experiments::Experiments::new(self.bus.clone(), governor_state.clone()).await,
        };
        let capsules_store = self
            .capsules
            .unwrap_or_else(|| Arc::new(capsule_guard::CapsuleStore::new()));
        let chat_state = self
            .chat
            .unwrap_or_else(|| Arc::new(chat::ChatState::new()));
        let runtime_registry = self
            .runtime
            .unwrap_or_else(|| Arc::new(runtime::RuntimeRegistry::new(self.bus.clone())));
        let logic_history_store = self.logic_history.unwrap_or_else(|| {
            let path = crate::util::state_dir()
                .join("training")
                .join("logic_history.json");
            Arc::new(training::LogicUnitHistoryStore::new(path, 100))
        });

        AppState::new(
            self.bus,
            self.kernel,
            self.policy,
            self.host,
            config_state,
            config_history,
            sse_id_map,
            endpoints,
            endpoints_meta,
            metrics,
            self.kernel_enabled,
            models_store,
            tool_cache,
            governor_state,
            feedback_hub,
            cluster_state,
            experiments_state,
            capsules_store,
            chat_state,
            runtime_registry,
            logic_history_store,
        )
    }
}
