use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type RuntimeId = String;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    #[default]
    Unknown,
    Starting,
    Ready,
    Degraded,
    Error,
    Offline,
}

impl RuntimeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeState::Unknown => "unknown",
            RuntimeState::Starting => "starting",
            RuntimeState::Ready => "ready",
            RuntimeState::Degraded => "degraded",
            RuntimeState::Error => "error",
            RuntimeState::Offline => "offline",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            RuntimeState::Unknown => "Unknown",
            RuntimeState::Starting => "Starting",
            RuntimeState::Ready => "Ready",
            RuntimeState::Degraded => "Degraded",
            RuntimeState::Error => "Error",
            RuntimeState::Offline => "Offline",
        }
    }

    pub fn from_slug(value: &str) -> Self {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "ready" | "ok" => RuntimeState::Ready,
            "starting" | "start" => RuntimeState::Starting,
            "degraded" => RuntimeState::Degraded,
            "offline" | "disabled" => RuntimeState::Offline,
            "error" => RuntimeState::Error,
            _ => RuntimeState::Unknown,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSeverity {
    #[default]
    Info,
    Warn,
    Error,
}

impl RuntimeSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeSeverity::Info => "info",
            RuntimeSeverity::Warn => "warn",
            RuntimeSeverity::Error => "error",
        }
    }

    pub fn display_label(&self) -> &'static str {
        match self {
            RuntimeSeverity::Info => "Info",
            RuntimeSeverity::Warn => "Warn",
            RuntimeSeverity::Error => "Error",
        }
    }

    pub fn from_slug(value: &str) -> Self {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "error" => RuntimeSeverity::Error,
            "warn" | "warning" => RuntimeSeverity::Warn,
            _ => RuntimeSeverity::Info,
        }
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

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct RuntimeHealth {
    pub latency_ms: Option<f64>,
    pub capacity: Option<u32>,
    pub inflight_jobs: Option<u32>,
    pub error_count: Option<u64>,
    pub request_count: Option<u64>,
    pub error_rate: Option<f64>,
    pub prompt_cache_warm: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RuntimeRestartBudget {
    pub window_seconds: u64,
    pub max_restarts: u32,
    pub used: u32,
    pub remaining: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<DateTime<Utc>>,
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
    #[serde(default)]
    pub restart_budget: Option<RuntimeRestartBudget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity_label: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl RuntimeStatus {
    pub fn new(id: impl Into<String>, state: RuntimeState) -> Self {
        let state_label = format!("state set to {:?}", state);
        let mut status = Self {
            id: id.into(),
            state,
            severity: RuntimeSeverity::Info,
            summary: state_label,
            detail: Vec::new(),
            health: None,
            restart_budget: None,
            state_label: None,
            severity_label: None,
            updated_at: Utc::now(),
        };
        status.refresh_labels();
        status
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

    pub fn same_payload(&self, other: &Self) -> bool {
        self.id == other.id
            && self.state == other.state
            && self.severity == other.severity
            && self.summary == other.summary
            && self.detail == other.detail
            && self.health == other.health
            && self.restart_budget == other.restart_budget
    }

    pub fn refresh_labels(&mut self) {
        self.state_label = Some(self.state.display_label().to_string());
        self.severity_label = Some(self.severity.display_label().to_string());
    }

    pub fn set_severity(&mut self, severity: RuntimeSeverity) {
        self.severity = severity;
        self.refresh_labels();
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
        status.set_severity(severity);
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

        status.refresh_labels();

        if let Some(http_obj) = payload.get("http").and_then(|v| v.as_object()) {
            let mut health = RuntimeHealth::default();
            if let Some(avg) = http_obj.get("avg_ewma_ms").and_then(|v| v.as_f64()) {
                health.latency_ms = Some(avg);
            }
            if let Some(errors) = http_obj.get("errors").and_then(|v| v.as_u64()) {
                health.error_count = Some(errors);
            }
            if let Some(hits) = http_obj.get("hits").and_then(|v| v.as_u64()) {
                health.request_count = Some(hits);
            }
            if let Some(rate) = http_obj.get("error_rate").and_then(|v| v.as_f64()) {
                health.error_rate = Some(rate);
            } else if let (Some(errors), Some(hits)) = (health.error_count, health.request_count) {
                if hits > 0 {
                    health.error_rate = Some(errors as f64 / hits as f64);
                }
            }
            if let Some(slow_routes) = http_obj.get("slow_routes").and_then(|v| v.as_array()) {
                for entry in slow_routes.iter().filter_map(|v| v.as_str()) {
                    status.detail.push(format!("Slow route: {}", entry));
                }
            }
            if health.latency_ms.is_some()
                || health.inflight_jobs.is_some()
                || health.error_count.is_some()
                || health.request_count.is_some()
                || health.error_rate.is_some()
                || health.capacity.is_some()
                || health.prompt_cache_warm.is_some()
            {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    #[test]
    fn maps_http_error_counts_into_health() {
        let payload = json!({
            "status": {
                "code": "ready",
                "severity": "warn",
                "label": "Ready with warnings",
                "detail": ["Heads up"],
                "aria_hint": "Some additional context",
            },
            "generated": "2024-05-20T12:00:00Z",
            "http": {
                "avg_ewma_ms": 125.0,
                "errors": 7u64,
                "hits": 32u64,
                "slow_routes": ["/slow"],
            }
        });

        let status =
            RuntimeStatus::from_health_payload("runtime-1", &payload).expect("status should parse");

        assert_eq!(status.id, "runtime-1");
        assert_eq!(status.state, RuntimeState::Ready);
        assert_eq!(status.severity, RuntimeSeverity::Warn);
        assert_eq!(status.state_label.as_deref(), Some("Ready"));
        assert_eq!(status.severity_label.as_deref(), Some("Warn"));
        assert!(status
            .detail
            .iter()
            .any(|entry| entry.contains("Slow route")));
        let expected = "2024-05-20T12:00:00Z"
            .parse::<DateTime<Utc>>()
            .expect("expected timestamp parses");
        assert_eq!(status.updated_at, expected);

        let health = status.health.expect("health payload should exist");
        assert_eq!(health.latency_ms, Some(125.0));
        assert_eq!(health.error_count, Some(7));
        assert_eq!(health.request_count, Some(32));
        let error_rate = health.error_rate.expect("error rate present");
        assert!((error_rate - (7.0 / 32.0)).abs() < 1e-12);
        assert_eq!(health.inflight_jobs, None);
    }

    #[test]
    fn runtime_state_labels_match_snake_case() {
        assert_eq!(RuntimeState::Ready.as_str(), "ready");
        assert_eq!(RuntimeState::Starting.as_str(), "starting");
        assert_eq!(RuntimeState::Unknown.as_str(), "unknown");
        assert_eq!(RuntimeState::Degraded.as_str(), "degraded");
        assert_eq!(RuntimeState::Offline.as_str(), "offline");
        assert_eq!(RuntimeState::Error.as_str(), "error");
    }

    #[test]
    fn runtime_severity_labels_match_snake_case() {
        assert_eq!(RuntimeSeverity::Info.as_str(), "info");
        assert_eq!(RuntimeSeverity::Warn.as_str(), "warn");
        assert_eq!(RuntimeSeverity::Error.as_str(), "error");
    }

    #[test]
    fn runtime_state_from_slug_is_case_insensitive() {
        assert_eq!(RuntimeState::from_slug("READY"), RuntimeState::Ready);
        assert_eq!(RuntimeState::from_slug(" ok "), RuntimeState::Ready);
        assert_eq!(RuntimeState::from_slug("Disabled"), RuntimeState::Offline);
        assert_eq!(RuntimeState::from_slug("start"), RuntimeState::Starting);
        assert_eq!(RuntimeState::from_slug("unknown"), RuntimeState::Unknown);
    }

    #[test]
    fn runtime_severity_from_slug_handles_synonyms() {
        assert_eq!(RuntimeSeverity::from_slug("warning"), RuntimeSeverity::Warn);
        assert_eq!(RuntimeSeverity::from_slug("WARN"), RuntimeSeverity::Warn);
        assert_eq!(RuntimeSeverity::from_slug("error"), RuntimeSeverity::Error);
        assert_eq!(RuntimeSeverity::from_slug("info"), RuntimeSeverity::Info);
    }

    #[test]
    fn runtime_status_payload_comparison_ignores_timestamps() {
        use chrono::Duration as ChronoDuration;

        let mut base = RuntimeStatus::new("runtime-a", RuntimeState::Ready)
            .with_summary("Ready")
            .touch();
        base.detail.push("All systems nominal".to_string());
        base.health = Some(RuntimeHealth {
            latency_ms: Some(42.0),
            capacity: Some(2),
            inflight_jobs: Some(1),
            error_count: Some(0),
            request_count: Some(10),
            error_rate: Some(0.0),
            prompt_cache_warm: Some(true),
        });
        base.restart_budget = Some(RuntimeRestartBudget {
            window_seconds: 600,
            max_restarts: 3,
            used: 1,
            remaining: 2,
            reset_at: None,
        });
        base.refresh_labels();

        let mut same = base.clone();
        same.updated_at = same.updated_at + ChronoDuration::seconds(30);
        assert!(base.same_payload(&same));

        let mut different = same.clone();
        different.summary = "Ready with warnings".to_string();
        assert!(!base.same_payload(&different));
    }
}
