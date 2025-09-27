use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type RuntimeId = String;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Unknown,
    Starting,
    Ready,
    Degraded,
    Error,
    Offline,
}

impl Default for RuntimeState {
    fn default() -> Self {
        RuntimeState::Unknown
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSeverity {
    Info,
    Warn,
    Error,
}

impl Default for RuntimeSeverity {
    fn default() -> Self {
        RuntimeSeverity::Info
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModality {
    Text,
    Audio,
    Vision,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAccelerator {
    Cpu,
    GpuCuda,
    GpuRocm,
    GpuMetal,
    GpuVulkan,
    NpuDirectml,
    NpuCoreml,
    NpuOther,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeDescriptor {
    pub id: RuntimeId,
    pub name: Option<String>,
    pub adapter: String,
    pub profile: Option<String>,
    pub modalities: Vec<RuntimeModality>,
    pub accelerator: Option<RuntimeAccelerator>,
    #[serde(default)]
    pub tags: BTreeMap<String, String>,
}

impl RuntimeDescriptor {
    pub fn new(id: impl Into<String>, adapter: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            adapter: adapter.into(),
            profile: None,
            modalities: Vec::new(),
            accelerator: None,
            tags: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RuntimeHealth {
    pub latency_ms: Option<f64>,
    pub capacity: Option<u32>,
    pub inflight_jobs: Option<u32>,
    pub prompt_cache_warm: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub id: RuntimeId,
    pub state: RuntimeState,
    pub severity: RuntimeSeverity,
    pub summary: String,
    #[serde(default)]
    pub detail: Vec<String>,
    #[serde(default)]
    pub health: Option<RuntimeHealth>,
    pub updated_at: DateTime<Utc>,
}

impl RuntimeStatus {
    pub fn new(id: impl Into<String>, state: RuntimeState) -> Self {
        let state_label = format!("state set to {:?}", state);
        Self {
            id: id.into(),
            state,
            severity: RuntimeSeverity::Info,
            summary: state_label,
            detail: Vec::new(),
            health: None,
            updated_at: Utc::now(),
        }
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    pub fn push_detail(mut self, msg: impl Into<String>) -> Self {
        self.detail.push(msg.into());
        self
    }

    pub fn touch(mut self) -> Self {
        self.updated_at = Utc::now();
        self
    }

    pub fn from_health_payload(id: &str, payload: &Value) -> Option<Self> {
        let status_obj = payload.get("status")?;
        let code = status_obj
            .get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let state = match code {
            "ok" | "ready" => RuntimeState::Ready,
            "degraded" => RuntimeState::Degraded,
            "offline" | "disabled" => RuntimeState::Offline,
            "error" => RuntimeState::Error,
            "starting" => RuntimeState::Starting,
            _ => RuntimeState::Unknown,
        };
        let severity = match status_obj
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
        {
            "error" => RuntimeSeverity::Error,
            "warn" | "warning" => RuntimeSeverity::Warn,
            _ => RuntimeSeverity::Info,
        };

        let mut status = RuntimeStatus::new(id.to_string(), state);
        status.severity = severity;
        if let Some(label) = status_obj.get("label").and_then(|v| v.as_str()) {
            status.summary = label.to_string();
        }
        if let Some(detail_arr) = status_obj.get("detail").and_then(|v| v.as_array()) {
            status.detail = detail_arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(hint) = status_obj.get("aria_hint").and_then(|v| v.as_str()) {
            if !hint.is_empty() {
                status.detail.push(hint.to_string());
            }
        }

        if let Some(ts) = payload.get("generated").and_then(|v| v.as_str()) {
            if let Ok(parsed) = ts.parse::<DateTime<Utc>>() {
                status.updated_at = parsed;
            }
        }

        if let Some(http_obj) = payload.get("http").and_then(|v| v.as_object()) {
            let mut health = RuntimeHealth::default();
            if let Some(avg) = http_obj.get("avg_ewma_ms").and_then(|v| v.as_f64()) {
                health.latency_ms = Some(avg);
            }
            if let Some(errors) = http_obj.get("errors").and_then(|v| v.as_u64()) {
                health.inflight_jobs = Some(errors as u32);
            }
            if let Some(slow_routes) = http_obj.get("slow_routes").and_then(|v| v.as_array()) {
                for entry in slow_routes.iter().filter_map(|v| v.as_str()) {
                    status.detail.push(format!("Slow route: {}", entry));
                }
            }
            if health.latency_ms.is_some() || health.inflight_jobs.is_some() {
                status.health = Some(health);
            }
        }

        Some(status)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeRecord {
    pub descriptor: RuntimeDescriptor,
    pub status: RuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    pub updated_at: DateTime<Utc>,
    pub runtimes: Vec<RuntimeRecord>,
}

impl RegistrySnapshot {
    pub fn empty() -> Self {
        Self {
            updated_at: Utc::now(),
            runtimes: Vec::new(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum AdapterError {
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("launch failure: {0}")]
    Launch(String),
    #[error("io error: {0}")]
    Io(String),
}

#[derive(Clone, Debug)]
pub struct PrepareContext<'a> {
    pub descriptor: &'a RuntimeDescriptor,
}

#[derive(Clone, Debug)]
pub struct PreparedRuntime {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RuntimeHandle {
    pub id: RuntimeId,
    pub pid: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct RuntimeHealthReport {
    pub status: RuntimeStatus,
}

#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn id(&self) -> &'static str;

    fn supports(&self) -> Vec<RuntimeModality> {
        Vec::new()
    }

    async fn prepare(&self, ctx: PrepareContext<'_>) -> Result<PreparedRuntime, AdapterError>;

    async fn launch(&self, prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError>;

    async fn shutdown(&self, handle: RuntimeHandle) -> Result<(), AdapterError>;

    async fn health(&self, handle: &RuntimeHandle) -> Result<RuntimeHealthReport, AdapterError>;
}

pub type BoxedAdapter = Box<dyn RuntimeAdapter>;
