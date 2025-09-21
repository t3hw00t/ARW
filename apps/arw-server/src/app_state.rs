use std::sync::Arc;

use arw_events::Bus;
use arw_kernel::Kernel;
use arw_policy::PolicyEngine;
use arw_wasi::ToolHost;
use tokio::sync::Mutex;

use crate::{
    capsule_guard, chat, cluster, experiments, feedback, governor, metrics, models, tool_cache,
};

pub(crate) type Policy = PolicyEngine;

#[derive(Clone)]
pub(crate) struct AppState {
    bus: Bus,
    kernel: Kernel,
    policy: Arc<Mutex<Policy>>, // hot-reloadable
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
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bus: Bus,
        kernel: Kernel,
        policy: Arc<Mutex<Policy>>,
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

    pub fn policy(&self) -> Arc<Mutex<Policy>> {
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
