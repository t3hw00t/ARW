use std::sync::Arc;

use arw_events::Bus;
use arw_kernel::Kernel;
use arw_wasi::ToolHost;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    autonomy, capsule_guard, chat, cluster, compression, context_capability, experiments, feedback,
    governor, identity, metrics, models, persona, policy, queue, runtime, runtime_bundles,
    runtime_supervisor, tool_cache, training,
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
    queue_signals: Arc<queue::QueueSignals>,
    kernel_enabled: bool,
    persona_enabled: bool,
    persona_service: Option<Arc<persona::PersonaService>>,
    models: Arc<models::ModelStore>,
    tool_cache: Arc<tool_cache::ToolCache>,
    governor: Arc<governor::GovernorState>,
    autonomy: Arc<autonomy::AutonomyRegistry>,
    feedback: Arc<feedback::FeedbackHub>,
    cluster: Arc<cluster::ClusterRegistry>,
    experiments: Arc<experiments::Experiments>,
    capsules: Arc<capsule_guard::CapsuleStore>,
    chat: Arc<chat::ChatState>,
    runtime: Arc<runtime::RuntimeRegistry>,
    runtime_supervisor: Arc<runtime_supervisor::RuntimeSupervisor>,
    runtime_bundles: Arc<runtime_bundles::RuntimeBundleStore>,
    logic_history: Arc<training::LogicUnitHistoryStore>,
    identity: Arc<identity::IdentityRegistry>,
    capability: Arc<crate::capability::CapabilityService>,
    compression: Arc<compression::CompressionService>,
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
        queue_signals: Arc<queue::QueueSignals>,
        kernel_enabled: bool,
        persona_enabled: bool,
        persona_service: Option<Arc<persona::PersonaService>>,
        models: Arc<models::ModelStore>,
        tool_cache: Arc<tool_cache::ToolCache>,
        governor: Arc<governor::GovernorState>,
        autonomy: Arc<autonomy::AutonomyRegistry>,
        feedback: Arc<feedback::FeedbackHub>,
        cluster: Arc<cluster::ClusterRegistry>,
        experiments: Arc<experiments::Experiments>,
        capsules: Arc<capsule_guard::CapsuleStore>,
        chat: Arc<chat::ChatState>,
        runtime: Arc<runtime::RuntimeRegistry>,
        runtime_supervisor: Arc<runtime_supervisor::RuntimeSupervisor>,
        runtime_bundles: Arc<runtime_bundles::RuntimeBundleStore>,
        logic_history: Arc<training::LogicUnitHistoryStore>,
        identity: Arc<identity::IdentityRegistry>,
        capability: Arc<crate::capability::CapabilityService>,
        compression: Arc<compression::CompressionService>,
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
            queue_signals,
            kernel_enabled,
            persona_enabled,
            persona_service,
            models,
            tool_cache,
            governor,
            autonomy,
            feedback,
            cluster,
            experiments,
            capsules,
            chat,
            runtime,
            runtime_supervisor,
            runtime_bundles,
            logic_history,
            identity,
            capability,
            compression,
        }
    }

    pub fn kernel_enabled(&self) -> bool {
        self.kernel_enabled
    }

    pub fn persona_enabled(&self) -> bool {
        self.persona_enabled
    }

    pub fn persona(&self) -> Option<Arc<persona::PersonaService>> {
        self.persona_service.clone()
    }

    pub fn capability(&self) -> Arc<crate::capability::CapabilityService> {
        self.capability.clone()
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

    pub fn queue_signals(&self) -> Arc<queue::QueueSignals> {
        self.queue_signals.clone()
    }

    pub fn signal_action_queue(&self) {
        self.queue_signals.wake();
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

    pub fn autonomy(&self) -> Arc<autonomy::AutonomyRegistry> {
        self.autonomy.clone()
    }

    pub fn feedback(&self) -> Arc<feedback::FeedbackHub> {
        self.feedback.clone()
    }

    pub fn compression(&self) -> Arc<compression::CompressionService> {
        self.compression.clone()
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

    pub fn runtime_supervisor(&self) -> Arc<runtime_supervisor::RuntimeSupervisor> {
        self.runtime_supervisor.clone()
    }

    pub fn runtime_bundles(&self) -> Arc<runtime_bundles::RuntimeBundleStore> {
        self.runtime_bundles.clone()
    }

    pub fn logic_history(&self) -> Arc<training::LogicUnitHistoryStore> {
        self.logic_history.clone()
    }

    pub fn identity(&self) -> Arc<identity::IdentityRegistry> {
        self.identity.clone()
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
    persona_enabled: bool,
    config_state: Option<SharedConfigState>,
    config_history: Option<SharedConfigHistory>,
    sse_id_map: Option<Arc<Mutex<crate::sse_cache::SseIdCache>>>,
    sse_capacity: usize,
    endpoints: Option<Arc<Vec<String>>>,
    endpoints_meta: Option<Arc<Vec<serde_json::Value>>>,
    metrics: Option<Arc<metrics::Metrics>>,
    queue_signals: Option<Arc<queue::QueueSignals>>,
    models: Option<Arc<models::ModelStore>>,
    tool_cache: Option<Arc<tool_cache::ToolCache>>,
    governor: Option<Arc<governor::GovernorState>>,
    autonomy: Option<Arc<autonomy::AutonomyRegistry>>,
    feedback: Option<Arc<feedback::FeedbackHub>>,
    cluster: Option<Arc<cluster::ClusterRegistry>>,
    experiments: Option<Arc<experiments::Experiments>>,
    capsules: Option<Arc<capsule_guard::CapsuleStore>>,
    chat: Option<Arc<chat::ChatState>>,
    runtime: Option<Arc<runtime::RuntimeRegistry>>,
    runtime_supervisor: Option<Arc<runtime_supervisor::RuntimeSupervisor>>,
    runtime_bundles: Option<Arc<runtime_bundles::RuntimeBundleStore>>,
    logic_history: Option<Arc<training::LogicUnitHistoryStore>>,
    identity: Option<Arc<identity::IdentityRegistry>>,
    persona: Option<Arc<persona::PersonaService>>,
    capability: Option<Arc<crate::capability::CapabilityService>>,
    compression: Option<Arc<compression::CompressionService>>,
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
            persona_enabled: false,
            config_state: None,
            config_history: None,
            sse_id_map: None,
            sse_capacity: 2048,
            endpoints: None,
            endpoints_meta: None,
            metrics: None,
            queue_signals: None,
            models: None,
            tool_cache: None,
            governor: None,
            autonomy: None,
            feedback: None,
            cluster: None,
            experiments: None,
            capsules: None,
            chat: None,
            runtime: None,
            runtime_supervisor: None,
            runtime_bundles: None,
            logic_history: None,
            identity: None,
            persona: None,
            capability: None,
            compression: None,
        }
    }
}

#[allow(dead_code)]
impl AppStateBuilder {
    pub(crate) fn with_config_state(mut self, config_state: SharedConfigState) -> Self {
        self.config_state = Some(config_state);
        self
    }

    pub(crate) fn with_persona_enabled(mut self, enabled: bool) -> Self {
        self.persona_enabled = enabled;
        self
    }

    pub(crate) fn with_persona_service(mut self, service: Arc<persona::PersonaService>) -> Self {
        self.persona = Some(service);
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

    pub(crate) fn with_queue_signals(mut self, signals: Arc<queue::QueueSignals>) -> Self {
        self.queue_signals = Some(signals);
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

    pub(crate) fn with_autonomy(mut self, autonomy: Arc<autonomy::AutonomyRegistry>) -> Self {
        self.autonomy = Some(autonomy);
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

    pub(crate) fn with_runtime_supervisor(
        mut self,
        supervisor: Arc<runtime_supervisor::RuntimeSupervisor>,
    ) -> Self {
        self.runtime_supervisor = Some(supervisor);
        self
    }

    pub(crate) fn with_runtime_bundles(
        mut self,
        bundles: Arc<runtime_bundles::RuntimeBundleStore>,
    ) -> Self {
        self.runtime_bundles = Some(bundles);
        self
    }

    pub(crate) fn with_logic_history(
        mut self,
        store: Arc<training::LogicUnitHistoryStore>,
    ) -> Self {
        self.logic_history = Some(store);
        self
    }

    pub(crate) fn with_identity(mut self, identity: Arc<identity::IdentityRegistry>) -> Self {
        self.identity = Some(identity);
        self
    }

    pub(crate) fn with_capability_service(
        mut self,
        capability: Arc<crate::capability::CapabilityService>,
    ) -> Self {
        self.capability = Some(capability);
        self
    }

    pub(crate) fn with_compression(
        mut self,
        compression: Arc<compression::CompressionService>,
    ) -> Self {
        self.compression = Some(compression);
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
        let queue_signals = self
            .queue_signals
            .unwrap_or_else(|| Arc::new(queue::QueueSignals::default()));

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
        let autonomy_registry = match self.autonomy {
            Some(state) => state,
            None => autonomy::AutonomyRegistry::new(self.bus.clone(), metrics.clone()).await,
        };
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
        let runtime_registry = match self.runtime {
            Some(state) => state,
            None => {
                let bus = self.bus.clone();
                let path = crate::util::state_dir()
                    .join("runtime")
                    .join("registry.json");
                Arc::new(runtime::RuntimeRegistry::with_storage(bus, path).await)
            }
        };
        let capability_service = self
            .capability
            .unwrap_or_else(|| Arc::new(crate::capability::CapabilityService::new()));
        let capability_profile = capability_service.maybe_refresh(false);
        let capability_plan = context_capability::plan_for_profile(&capability_profile);
        let compression_service = self
            .compression
            .unwrap_or_else(|| Arc::new(compression::CompressionService::initialise()));
        let runtime_supervisor = match self.runtime_supervisor {
            Some(state) => state,
            None => {
                runtime_supervisor::RuntimeSupervisor::new_with_capability_plan(
                    runtime_registry.clone(),
                    self.bus.clone(),
                    capability_plan.clone(),
                )
                .await
            }
        };
        let runtime_bundles_store = match self.runtime_bundles {
            Some(store) => store,
            None => runtime_bundles::RuntimeBundleStore::load_default().await,
        };
        let logic_history_store = self.logic_history.unwrap_or_else(|| {
            let path = crate::util::state_dir()
                .join("training")
                .join("logic_history.json");
            Arc::new(training::LogicUnitHistoryStore::new(path, 100))
        });
        let identity_registry = match self.identity {
            Some(registry) => registry,
            None => identity::IdentityRegistry::new(self.bus.clone()).await,
        };

        let persona_service = if self.persona_enabled {
            match self.persona {
                Some(service) => Some(service),
                None => Some(persona::PersonaService::new(self.kernel.clone())),
            }
        } else {
            None
        };

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
            metrics.clone(),
            queue_signals.clone(),
            self.kernel_enabled,
            self.persona_enabled,
            persona_service,
            models_store,
            tool_cache,
            governor_state,
            autonomy_registry.clone(),
            feedback_hub,
            cluster_state,
            experiments_state,
            capsules_store,
            chat_state,
            runtime_registry,
            runtime_supervisor,
            runtime_bundles_store,
            logic_history_store,
            identity_registry,
            capability_service,
            compression_service,
        )
    }
}
