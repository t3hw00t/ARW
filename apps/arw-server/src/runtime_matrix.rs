use arw_runtime::{
    RuntimeAccelerator, RuntimeRecord, RuntimeRestartBudget, RuntimeSeverity, RuntimeState,
};
use arw_topics::TOPIC_RUNTIME_HEALTH;
use chrono::SecondsFormat;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration as StdDuration, Instant};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::warn;
use utoipa::ToSchema;

use crate::{read_models, tasks::TaskHandle, AppState};

#[cfg(test)]
use arw_runtime::{RuntimeDescriptor, RuntimeStatus};

const MATRIX_TTL_DEFAULT_SECS: u64 = 60;
const MATRIX_TTL_MIN_SECS: u64 = 10;
const MATRIX_TTL_MAX_SECS: u64 = 900;

const STATE_READY: u8 = 0;
const STATE_STARTING: u8 = 1;
const STATE_UNKNOWN: u8 = 2;
const STATE_DEGRADED: u8 = 3;
const STATE_OFFLINE: u8 = 4;
const STATE_ERROR: u8 = 5;

const SEVERITY_INFO: u8 = 0;
const SEVERITY_WARN: u8 = 1;
const SEVERITY_ERROR: u8 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixEntry {
    pub target: String,
    pub status: RuntimeMatrixStatus,
    pub generated: String,
    pub kernel: RuntimeMatrixKernel,
    pub bus: RuntimeMatrixBus,
    pub events: RuntimeMatrixEvents,
    pub http: RuntimeMatrixHttp,
    pub runtime: RuntimeMatrixRuntimeSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixStatus {
    pub code: String,
    pub severity: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail: Vec<String>,
    pub aria_hint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixKernel {
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixBus {
    pub published: u64,
    pub delivered: u64,
    pub receivers: u64,
    pub lagged: u64,
    #[serde(rename = "no_receivers")]
    pub no_receivers: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixEvents {
    pub total: u64,
    pub kinds: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixHttp {
    pub routes: u64,
    pub hits: u64,
    pub errors: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_ewma_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_routes: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema, Default)]
pub struct RuntimeMatrixRuntimeSummary {
    pub total: u64,
    pub updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub states: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<BTreeMap<String, u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alerts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_pressure: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_accelerator: Option<BTreeMap<String, RuntimeMatrixAcceleratorSummary>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RuntimeMatrixAcceleratorSummary {
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub states: Option<BTreeMap<String, u64>>,
}

#[derive(Clone)]
struct TimedValue {
    inserted_at: Instant,
    value: RuntimeMatrixEntry,
}

impl TimedValue {
    fn new(value: RuntimeMatrixEntry) -> Self {
        Self {
            inserted_at: Instant::now(),
            value,
        }
    }

    fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.inserted_at) > matrix_ttl()
    }
}

fn store() -> &'static RwLock<HashMap<String, TimedValue>> {
    static MATRIX: OnceCell<RwLock<HashMap<String, TimedValue>>> = OnceCell::new();
    MATRIX.get_or_init(|| RwLock::new(HashMap::new()))
}

#[derive(Default)]
struct AcceleratorRollup {
    total: u64,
    states: BTreeMap<String, u64>,
}

fn prune_expired(store: &mut HashMap<String, TimedValue>, now: Instant) {
    store.retain(|_, entry| !entry.is_expired(now));
}

fn matrix_ttl() -> StdDuration {
    static TTL: OnceCell<StdDuration> = OnceCell::new();
    *TTL.get_or_init(compute_matrix_ttl)
}

pub(crate) fn ttl_seconds() -> u64 {
    matrix_ttl().as_secs()
}

fn compute_matrix_ttl() -> StdDuration {
    let raw = std::env::var("ARW_RUNTIME_MATRIX_TTL_SEC").ok();
    parse_matrix_ttl(raw.as_deref())
}

fn parse_matrix_ttl(raw: Option<&str>) -> StdDuration {
    let ttl_secs = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(MATRIX_TTL_MIN_SECS, MATRIX_TTL_MAX_SECS))
        .unwrap_or(MATRIX_TTL_DEFAULT_SECS);
    // Matrix snapshots can now live longer on busy nodes: clamp to a
    // reasonable range so misconfigured values do not turn stale or churny.
    StdDuration::from_secs(ttl_secs)
}

fn node_id() -> String {
    static NODE_ID: OnceCell<String> = OnceCell::new();
    NODE_ID
        .get_or_init(|| {
            std::env::var("ARW_NODE_ID")
                .ok()
                .and_then(|value| {
                    let trimmed = value.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed.to_string())
                    }
                })
                .or_else(|| {
                    sysinfo::System::host_name().and_then(|hostname| {
                        let trimmed = hostname.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    })
                })
                .unwrap_or_else(|| "local".to_string())
        })
        .clone()
}

pub(crate) async fn snapshot() -> HashMap<String, RuntimeMatrixEntry> {
    let mut guard = store().write().await;
    let now = Instant::now();
    prune_expired(&mut guard, now);
    guard
        .iter()
        .map(|(key, entry)| (key.clone(), entry.value.clone()))
        .collect()
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let subscriber_state = state.clone();
    let bus_for_patch = state.bus();
    let mut handles = Vec::new();
    handles.push(TaskHandle::new(
        "runtime_matrix.health_subscriber",
        tokio::spawn(async move {
            let mut rx = subscriber_state.bus().subscribe();
            while let Ok(env) = rx.recv().await {
                if env.kind == TOPIC_RUNTIME_HEALTH {
                    let key = env
                        .payload
                        .get("target")
                        .and_then(|v| v.as_str())
                        .unwrap_or("runtime")
                        .to_string();
                    let entry =
                        match serde_json::from_value::<RuntimeMatrixEntry>(env.payload.clone()) {
                            Ok(value) => value,
                            Err(err) => {
                                warn!(
                                    target: "arw::runtime",
                                    error = %err,
                                    "ignoring malformed runtime health payload"
                                );
                                continue;
                            }
                        };
                    let snapshot = {
                        let mut guard = store().write().await;
                        let now = Instant::now();
                        guard.insert(key, TimedValue::new(entry));
                        prune_expired(&mut guard, now);
                        guard
                            .iter()
                            .map(|(k, entry)| (k.clone(), entry.value.clone()))
                            .collect::<HashMap<_, _>>()
                    };
                    if let Ok(payload) = serde_json::to_value(&snapshot) {
                        read_models::publish_read_model_patch(
                            &bus_for_patch,
                            "runtime_matrix",
                            &json!({
                                "items": payload,
                                "ttl_seconds": ttl_seconds(),
                                "updated": chrono::Utc::now()
                                    .to_rfc3339_opts(SecondsFormat::Millis, true)
                            }),
                        );
                    }
                }
            }
        }),
    ));

    let publisher_state = state.clone();
    handles.push(TaskHandle::new(
        "runtime_matrix.local_publisher",
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(5));
            loop {
                tick.tick().await;
                if let Some(entry) = build_local_health_payload(&publisher_state).await {
                    if let Ok(json_payload) = serde_json::to_value(&entry) {
                        publisher_state
                            .bus()
                            .publish(TOPIC_RUNTIME_HEALTH, &json_payload);
                    }
                }
            }
        }),
    ));

    handles
}

async fn build_local_health_payload(state: &AppState) -> Option<RuntimeMatrixEntry> {
    let generated = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let metrics = state.metrics().snapshot();
    let bus_stats = state.bus().stats();

    let mut total_hits = 0u64;
    let mut total_errors = 0u64;
    let mut ewma_sum = 0.0f64;
    let mut ewma_count = 0u64;
    let mut slow_routes: Vec<String> = Vec::new();
    for (path, summary) in metrics.routes.by_path.iter() {
        total_hits = total_hits.saturating_add(summary.hits);
        total_errors = total_errors.saturating_add(summary.errors);
        if summary.ewma_ms.is_finite() && summary.ewma_ms > 0.0 {
            ewma_sum += summary.ewma_ms;
            ewma_count = ewma_count.saturating_add(1);
            if summary.ewma_ms > 1_000.0 {
                slow_routes.push(format!("{} ({:.0} ms)", path, summary.ewma_ms));
            }
        }
    }
    let avg_ewma = if ewma_count == 0 {
        None
    } else {
        Some(ewma_sum / ewma_count as f64)
    };

    let error_rate = if total_hits == 0 {
        None
    } else {
        Some(total_errors as f64 / total_hits as f64)
    };

    let runtime_snapshot = state.runtime().snapshot().await;
    let mut runtime_states: BTreeMap<String, u64> = BTreeMap::new();
    let mut runtime_severities: BTreeMap<String, u64> = BTreeMap::new();
    let mut accelerator_rollup: BTreeMap<String, AcceleratorRollup> = BTreeMap::new();
    let mut runtime_alerts: Vec<String> = Vec::new();
    let mut restart_pressure: Vec<String> = Vec::new();
    let mut worst_state_rank: u8 = 0;
    let mut worst_severity_rank: u8 = 0;

    for record in &runtime_snapshot.runtimes {
        let status = &record.status;
        let state_slug = runtime_state_slug(&status.state);
        *runtime_states.entry(state_slug.to_string()).or_default() += 1;

        let severity_slug = runtime_severity_slug(&status.severity);
        *runtime_severities
            .entry(severity_slug.to_string())
            .or_default() += 1;

        let accelerator_label = record
            .descriptor
            .accelerator
            .as_ref()
            .map(runtime_accelerator_label)
            .unwrap_or("Unspecified");
        let rollup = accelerator_rollup
            .entry(accelerator_label.to_string())
            .or_default();
        rollup.total = rollup.total.saturating_add(1);
        *rollup.states.entry(state_slug.to_string()).or_default() += 1;

        let state_rank = runtime_state_weight(&status.state);
        if state_rank > worst_state_rank {
            worst_state_rank = state_rank;
        }
        let severity_rank = runtime_severity_weight(&status.severity);
        if severity_rank > worst_severity_rank {
            worst_severity_rank = severity_rank;
        }

        if state_rank >= STATE_DEGRADED {
            runtime_alerts.push(runtime_alert_line(record));
        }

        if let Some(budget) = &status.restart_budget {
            if let Some(message) = runtime_restart_message(record, budget) {
                restart_pressure.push(message);
            }
        }
    }

    let runtime_issue_summary = if !runtime_alerts.is_empty() {
        Some(format!("Runtime issues: {}", runtime_alerts.join("; ")))
    } else {
        None
    };
    let restart_summary = if !restart_pressure.is_empty() {
        Some(format!("Restart budgets: {}", restart_pressure.join("; ")))
    } else {
        None
    };

    let mut status_code = if state.kernel_enabled() {
        "ok".to_string()
    } else {
        "offline".to_string()
    };
    let mut severity = if state.kernel_enabled() {
        "info".to_string()
    } else {
        "error".to_string()
    };
    let mut reasons: Vec<String> = Vec::new();

    if !state.kernel_enabled() {
        reasons.push("Kernel runtime disabled".to_string());
    }

    if bus_stats.lagged > 0 {
        if status_code == "ok" {
            status_code = "degraded".to_string();
        }
        severity = "warn".to_string();
        reasons.push(format!(
            "Bus lag observed ({} lagged events)",
            bus_stats.lagged
        ));
    }

    if total_errors > 0 {
        if status_code == "ok" {
            status_code = "degraded".to_string();
        }
        severity = if total_errors > total_hits / 10 {
            "error".to_string()
        } else {
            "warn".to_string()
        };
        reasons.push(format!("HTTP errors recorded: {}", total_errors));
    }

    if let Some(avg) = avg_ewma {
        if avg > 1_500.0 {
            if status_code == "ok" {
                status_code = "degraded".to_string();
            }
            severity = "warn".to_string();
            reasons.push(format!("High average latency {:.0} ms", avg));
        }
    }

    if let Some(summary) = runtime_issue_summary.as_ref() {
        if worst_state_rank >= STATE_ERROR || worst_severity_rank >= SEVERITY_ERROR {
            status_code = "error".to_string();
            severity = "error".to_string();
        } else {
            if status_code == "ok" {
                status_code = "degraded".to_string();
            }
            if severity == "info" {
                severity = "warn".to_string();
            }
        }
        reasons.push(summary.clone());
    }

    if let Some(summary) = restart_summary.as_ref() {
        if status_code == "ok" {
            status_code = "degraded".to_string();
        }
        if severity == "info" {
            severity = "warn".to_string();
        }
        reasons.push(summary.clone());
    }

    if reasons.is_empty() {
        reasons.push("Running within expected ranges".to_string());
    }

    let primary_reason = reasons.first().cloned().unwrap_or_default();
    let status_label = match status_code.as_str() {
        "offline" => "Offline - Kernel disabled".to_string(),
        "error" => format!("Error - {}", primary_reason),
        "degraded" => format!("Degraded - {}", primary_reason),
        _ => "Ready - Runtime telemetry nominal".to_string(),
    };
    let aria_hint = format!("Runtime status {}. {}", status_label, reasons.join("; "));

    let runtime_summary = RuntimeMatrixRuntimeSummary {
        total: runtime_snapshot.runtimes.len() as u64,
        updated: runtime_snapshot
            .updated_at
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        states: optional_map(runtime_states),
        severity: optional_map(runtime_severities),
        alerts: optional_vec(runtime_alerts),
        restart_pressure: optional_vec(restart_pressure),
        by_accelerator: optional_accelerator_map(accelerator_rollup),
    };

    let entry = RuntimeMatrixEntry {
        target: node_id(),
        status: RuntimeMatrixStatus {
            code: status_code,
            severity,
            label: status_label,
            detail: reasons,
            aria_hint,
        },
        generated,
        kernel: RuntimeMatrixKernel {
            enabled: state.kernel_enabled(),
        },
        bus: RuntimeMatrixBus {
            published: bus_stats.published,
            delivered: bus_stats.delivered,
            receivers: bus_stats.receivers as u64,
            lagged: bus_stats.lagged,
            no_receivers: bus_stats.no_receivers,
        },
        events: RuntimeMatrixEvents {
            total: metrics.events.total,
            kinds: metrics.events.kinds.len() as u64,
        },
        http: RuntimeMatrixHttp {
            routes: metrics.routes.by_path.len() as u64,
            hits: total_hits,
            errors: total_errors,
            avg_ewma_ms: avg_ewma,
            error_rate,
            slow_routes: optional_vec(slow_routes),
        },
        runtime: runtime_summary,
    };

    Some(entry)
}

fn optional_map(map: BTreeMap<String, u64>) -> Option<BTreeMap<String, u64>> {
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn optional_vec(vec: Vec<String>) -> Option<Vec<String>> {
    if vec.is_empty() {
        None
    } else {
        Some(vec)
    }
}

fn optional_accelerator_map(
    rollup: BTreeMap<String, AcceleratorRollup>,
) -> Option<BTreeMap<String, RuntimeMatrixAcceleratorSummary>> {
    if rollup.is_empty() {
        return None;
    }
    let mapped = rollup
        .into_iter()
        .map(|(label, summary)| {
            let AcceleratorRollup { total, states } = summary;
            (
                label,
                RuntimeMatrixAcceleratorSummary {
                    total,
                    states: optional_map(states),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Some(mapped)
}

fn runtime_state_slug(state: &RuntimeState) -> &'static str {
    state.as_str()
}

fn runtime_state_label(state: &RuntimeState) -> &'static str {
    state.display_label()
}

fn runtime_state_weight(state: &RuntimeState) -> u8 {
    match state {
        RuntimeState::Ready => STATE_READY,
        RuntimeState::Starting => STATE_STARTING,
        RuntimeState::Unknown => STATE_UNKNOWN,
        RuntimeState::Degraded => STATE_DEGRADED,
        RuntimeState::Offline => STATE_OFFLINE,
        RuntimeState::Error => STATE_ERROR,
    }
}

fn runtime_severity_slug(severity: &RuntimeSeverity) -> &'static str {
    severity.as_str()
}

fn runtime_severity_weight(severity: &RuntimeSeverity) -> u8 {
    match severity {
        RuntimeSeverity::Info => SEVERITY_INFO,
        RuntimeSeverity::Warn => SEVERITY_WARN,
        RuntimeSeverity::Error => SEVERITY_ERROR,
    }
}

fn runtime_accelerator_label(acc: &RuntimeAccelerator) -> &'static str {
    match acc {
        RuntimeAccelerator::Cpu => "CPU",
        RuntimeAccelerator::GpuCuda => "GPU (CUDA)",
        RuntimeAccelerator::GpuRocm => "GPU (ROCm)",
        RuntimeAccelerator::GpuMetal => "GPU (Metal)",
        RuntimeAccelerator::GpuVulkan => "GPU (Vulkan)",
        RuntimeAccelerator::NpuDirectml => "NPU (DirectML)",
        RuntimeAccelerator::NpuCoreml => "NPU (CoreML)",
        RuntimeAccelerator::NpuOther => "NPU",
        RuntimeAccelerator::Other => "Other",
    }
}

fn runtime_label(record: &RuntimeRecord) -> String {
    record
        .descriptor
        .name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| record.descriptor.id.clone())
}

fn runtime_alert_line(record: &RuntimeRecord) -> String {
    let label = runtime_label(record);
    let summary_raw = record.status.summary.trim();
    let summary_text = if summary_raw.is_empty() {
        runtime_state_label(&record.status.state).to_string()
    } else {
        summary_raw.to_string()
    };
    if let Some(detail) = record
        .status
        .detail
        .iter()
        .find(|detail| !detail.trim().is_empty())
    {
        format!("{}: {} ({})", label, summary_text, detail)
    } else {
        format!("{}: {}", label, summary_text)
    }
}

fn runtime_restart_message(
    record: &RuntimeRecord,
    budget: &RuntimeRestartBudget,
) -> Option<String> {
    let label = runtime_label(record);
    let reset_hint = budget
        .reset_at
        .map(|ts| {
            format!(
                "; resets at {}",
                ts.to_rfc3339_opts(SecondsFormat::Millis, true)
            )
        })
        .unwrap_or_default();
    if budget.remaining == 0 {
        Some(format!(
            "{} restart budget exhausted ({}/{} used{})",
            label, budget.used, budget.max_restarts, reset_hint
        ))
    } else if budget.remaining <= 1 {
        Some(format!(
            "{} restart budget low ({} of {} remaining; window {}s{})",
            label, budget.remaining, budget.max_restarts, budget.window_seconds, reset_hint
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_matrix_ttl_defaults_when_missing() {
        assert_eq!(parse_matrix_ttl(None).as_secs(), MATRIX_TTL_DEFAULT_SECS);
    }

    #[test]
    fn parse_matrix_ttl_trims_valid_values() {
        assert_eq!(parse_matrix_ttl(Some(" 120 ")).as_secs(), 120);
    }

    #[test]
    fn parse_matrix_ttl_clamps_minimum_and_maximum() {
        assert_eq!(parse_matrix_ttl(Some("5")).as_secs(), MATRIX_TTL_MIN_SECS);
        assert_eq!(
            parse_matrix_ttl(Some("1200")).as_secs(),
            MATRIX_TTL_MAX_SECS
        );
    }

    #[test]
    fn parse_matrix_ttl_falls_back_on_invalid_values() {
        assert_eq!(
            parse_matrix_ttl(Some("abc")).as_secs(),
            MATRIX_TTL_DEFAULT_SECS
        );
    }

    fn make_record(
        id: &str,
        name: Option<&str>,
        state: RuntimeState,
        severity: RuntimeSeverity,
        summary: &str,
        detail: &[&str],
    ) -> RuntimeRecord {
        let mut descriptor = RuntimeDescriptor::new(id, "test-adapter");
        descriptor.name = name.map(|s| s.to_string());
        let mut status = RuntimeStatus::new(id.to_string(), state);
        status.summary = summary.to_string();
        status.set_severity(severity);
        status.detail = detail.iter().map(|s| s.to_string()).collect();
        RuntimeRecord { descriptor, status }
    }

    #[test]
    fn alert_line_prefers_detail_when_available() {
        let record = make_record(
            "runtime-a",
            Some("Llama Stub"),
            RuntimeState::Degraded,
            RuntimeSeverity::Warn,
            "Latency spike",
            &["P95 above 1500 ms"],
        );
        let line = runtime_alert_line(&record);
        assert!(line.contains("Llama Stub"));
        assert!(line.contains("Latency spike"));
        assert!(line.contains("1500"));
    }

    #[test]
    fn alert_line_falls_back_to_state_label_when_detail_missing() {
        let record = make_record(
            "runtime-b",
            None,
            RuntimeState::Error,
            RuntimeSeverity::Error,
            "",
            &[""],
        );
        let line = runtime_alert_line(&record);
        assert!(line.contains("runtime-b"));
        assert!(line.contains("Error"));
    }

    #[test]
    fn restart_message_triggers_on_budget_exhaustion() {
        let mut record = make_record(
            "runtime-c",
            Some("GPU Profile"),
            RuntimeState::Ready,
            RuntimeSeverity::Info,
            "Healthy",
            &[],
        );
        let budget = RuntimeRestartBudget {
            window_seconds: 600,
            max_restarts: 3,
            used: 3,
            remaining: 0,
            reset_at: Some(chrono::Utc::now()),
        };
        record.status.restart_budget = Some(budget.clone());
        let message = runtime_restart_message(&record, &budget).expect("budget exhausted");
        assert!(message.contains("GPU Profile"));
        assert!(message.contains("exhausted"));
        assert!(message.contains("3/3"));
    }

    #[test]
    fn restart_message_warns_when_one_attempt_left() {
        let record = make_record(
            "runtime-d",
            None,
            RuntimeState::Ready,
            RuntimeSeverity::Info,
            "Healthy",
            &[],
        );
        let budget = RuntimeRestartBudget {
            window_seconds: 900,
            max_restarts: 4,
            used: 3,
            remaining: 1,
            reset_at: None,
        };
        let message = runtime_restart_message(&record, &budget).expect("warn when one remaining");
        assert!(message.contains("runtime-d"));
        assert!(message.contains("remaining"));
    }

    #[test]
    fn restart_message_absent_when_budget_safe() {
        let record = make_record(
            "runtime-e",
            None,
            RuntimeState::Ready,
            RuntimeSeverity::Info,
            "Healthy",
            &[],
        );
        let budget = RuntimeRestartBudget {
            window_seconds: 1800,
            max_restarts: 5,
            used: 1,
            remaining: 4,
            reset_at: None,
        };
        assert!(runtime_restart_message(&record, &budget).is_none());
    }

    #[test]
    fn accelerator_labels_are_human_readable() {
        assert_eq!(runtime_accelerator_label(&RuntimeAccelerator::Cpu), "CPU");
        assert_eq!(
            runtime_accelerator_label(&RuntimeAccelerator::GpuCuda),
            "GPU (CUDA)"
        );
        assert_eq!(
            runtime_accelerator_label(&RuntimeAccelerator::NpuCoreml),
            "NPU (CoreML)"
        );
    }
}
