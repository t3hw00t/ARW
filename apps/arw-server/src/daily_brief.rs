use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
    time::Duration,
};

use arw_events::Bus;
use arw_topics as topics;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::{sync::RwLock, time::sleep};
use tracing::warn;

use crate::{
    autonomy::AutonomyMode,
    economy::{EconomyLedgerEntry, EconomyLedgerSnapshot},
    responses,
    tasks::TaskHandle,
    AppState,
};

const DEFAULT_INTERVAL_SECS: u64 = 3600;
const MAX_RECENT_ENTRIES: usize = 5;

#[derive(Clone, Debug, Serialize)]
pub struct DailyBriefSnapshot {
    pub generated_at: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub economy: Option<BriefEconomySection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<BriefRuntimeSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<BriefPersonaSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<BriefMemorySection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autonomy: Option<BriefAutonomySection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefEconomySection {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub totals: Vec<BriefEconomyTotal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_entries: Vec<BriefEconomyEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefEconomyTotal {
    pub currency: String,
    pub settled: f64,
    pub pending: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefEconomyEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefRuntimeSection {
    pub total: usize,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_state: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_severity: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefPersonaSection {
    pub total: usize,
    pub approvals_pending: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibe_average: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feedback_samples: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefMemorySection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_needs_more_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_reasons: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall_risk_ratio: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BriefAutonomySection {
    pub lanes_total: usize,
    pub lanes_autonomous: usize,
    pub lanes_paused: usize,
    pub active_jobs: u64,
    pub queued_jobs: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

pub struct DailyBriefService {
    bus: Bus,
    snapshot: RwLock<Option<DailyBriefSnapshot>>,
}

impl DailyBriefService {
    pub fn new(bus: Bus) -> Arc<Self> {
        Arc::new(Self {
            bus,
            snapshot: RwLock::new(None),
        })
    }

    pub async fn latest(&self) -> Option<DailyBriefSnapshot> {
        self.snapshot.read().await.clone()
    }

    pub async fn publish(&self, snapshot: DailyBriefSnapshot) {
        {
            let mut guard = self.snapshot.write().await;
            *guard = Some(snapshot.clone());
        }
        if let Ok(mut payload) = serde_json::to_value(&snapshot) {
            responses::attach_corr(&mut payload);
            self.bus
                .publish(topics::TOPIC_BRIEF_DAILY_PUBLISHED, &payload);
        }
    }
}

pub fn start(state: AppState) -> TaskHandle {
    let interval = interval_from_env();
    crate::tasks::spawn_supervised("daily_brief.generator", move || {
        let state = state.clone();
        async move {
            if let Err(err) = generate_and_publish(&state).await {
                warn!(target: "arw::daily_brief", error = %err, "initial brief generation failed");
            }
            loop {
                sleep(interval).await;
                if let Err(err) = generate_and_publish(&state).await {
                    warn!(
                        target: "arw::daily_brief",
                        error = %err,
                        "daily brief generation failed"
                    );
                }
            }
        }
    })
}

async fn generate_and_publish(state: &AppState) -> anyhow::Result<()> {
    let snapshot = generate_brief(state).await?;
    state.daily_brief().publish(snapshot).await;
    Ok(())
}

async fn generate_brief(state: &AppState) -> anyhow::Result<DailyBriefSnapshot> {
    let economy_snapshot = state.economy().snapshot().await;
    let runtime_snapshot = state.runtime().snapshot().await;

    let economy_section = summarise_economy(&economy_snapshot);
    let runtime_section = summarise_runtime(&runtime_snapshot);
    let persona_section = summarise_persona(state).await;
    let memory_section = summarise_memory(state);
    let autonomy_section = summarise_autonomy(state).await;

    let mut attention: Vec<String> = economy_snapshot.attention.clone();
    if let Some(runtime) = &runtime_section {
        attention.extend(runtime.alerts.clone());
    }
    if let Some(persona) = &persona_section {
        attention.extend(persona.alerts.clone());
    }
    if let Some(memory) = &memory_section {
        attention.extend(memory.alerts.clone());
    }
    if let Some(autonomy) = &autonomy_section {
        attention.extend(autonomy.alerts.clone());
    }
    attention.sort();
    attention.dedup();

    let summary_line = render_summary(
        &economy_section,
        &runtime_section,
        &persona_section,
        &memory_section,
        &autonomy_section,
    );

    Ok(DailyBriefSnapshot {
        generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        summary: summary_line,
        economy: economy_section,
        runtime: runtime_section,
        persona: persona_section,
        memory: memory_section,
        autonomy: autonomy_section,
        attention,
    })
}

fn summarise_economy(snapshot: &EconomyLedgerSnapshot) -> Option<BriefEconomySection> {
    if snapshot.entries.is_empty() && snapshot.totals.is_empty() {
        return None;
    }

    let mut aggregate: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for total in &snapshot.totals {
        let entry = aggregate
            .entry(total.currency.clone())
            .or_insert((0.0, 0.0));
        if let Some(pending) = total.pending {
            entry.0 += pending;
        }
        if let Some(settled) = total.settled {
            entry.1 += settled;
        }
    }
    if aggregate.is_empty() {
        aggregate.insert("unitless".to_string(), (0.0, 0.0));
    }
    let totals = aggregate
        .into_iter()
        .map(|(currency, (pending, settled))| BriefEconomyTotal {
            currency,
            settled,
            pending,
        })
        .collect::<Vec<_>>();

    let mut entries = snapshot.entries.clone();
    entries.sort_by_key(|entry| Reverse(issued_ts(entry)));
    let recent_entries = entries
        .into_iter()
        .take(MAX_RECENT_ENTRIES)
        .map(|entry| BriefEconomyEntry {
            id: entry.id.clone(),
            status: entry.status.clone(),
            currency: entry.currency.clone(),
            amount: entry.net_amount.or(entry.gross_amount),
            issued_at: entry.issued_at.clone(),
            tags: build_tags(&entry),
        })
        .collect();

    Some(BriefEconomySection {
        totals,
        recent_entries,
    })
}

fn summarise_runtime(snapshot: &arw_runtime::RegistrySnapshot) -> Option<BriefRuntimeSection> {
    if snapshot.runtimes.is_empty() {
        return None;
    }
    let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_severity: BTreeMap<String, u64> = BTreeMap::new();
    let mut alerts = Vec::new();
    for record in &snapshot.runtimes {
        let state_label = record.status.state.display_label().to_string();
        *by_state.entry(state_label.clone()).or_default() += 1;
        let severity_label = record.status.severity.display_label().to_string();
        *by_severity.entry(severity_label).or_default() += 1;
        if matches!(
            record.status.state,
            arw_runtime::RuntimeState::Error | arw_runtime::RuntimeState::Offline
        ) {
            alerts.push(format!(
                "Runtime {} {}",
                runtime_name(record),
                record.status.state.display_label()
            ));
        } else if matches!(record.status.state, arw_runtime::RuntimeState::Degraded) {
            alerts.push(format!("Runtime {} degraded", runtime_name(record)));
        }
        if let Some(health) = &record.status.health {
            if let Some(inflight) = health.inflight_jobs {
                if inflight > 0 {
                    alerts.push(format!(
                        "Runtime {} {inflight} job{} in flight",
                        runtime_name(record),
                        if inflight == 1 { "" } else { "s" }
                    ));
                }
            }
            if let Some(error_rate) = health.error_rate {
                if error_rate > 0.0 {
                    alerts.push(format!(
                        "Runtime {} error rate {:.2}%",
                        runtime_name(record),
                        error_rate * 100.0
                    ));
                }
            }
        }
    }
    alerts.sort();
    alerts.dedup();

    Some(BriefRuntimeSection {
        total: snapshot.runtimes.len(),
        by_state,
        by_severity,
        alerts,
    })
}

async fn summarise_persona(state: &AppState) -> Option<BriefPersonaSection> {
    if !state.persona_enabled() {
        return None;
    }
    let service = state.persona()?;
    let entries = match service.list_entries(None, None, 64).await {
        Ok(items) => items,
        Err(err) => {
            warn!(
                target: "arw::daily_brief",
                error = %err,
                "failed to load persona entries for daily brief"
            );
            return None;
        }
    };
    let total = entries.len();
    let pending = match service
        .list_proposals(None, Some("pending".to_string()), 256)
        .await
    {
        Ok(items) => items,
        Err(err) => {
            warn!(
                target: "arw::daily_brief",
                error = %err,
                "failed to load pending persona proposals for daily brief"
            );
            Vec::new()
        }
    };
    let mut pending_map: HashMap<String, usize> = HashMap::new();
    for proposal in pending {
        *pending_map.entry(proposal.persona_id).or_default() += 1;
    }
    let mut approvals: Vec<String> = Vec::new();
    let mut approvals_pending = 0usize;
    for entry in &entries {
        if let Some(count) = pending_map.get(&entry.id) {
            approvals_pending += *count;
            if *count > 0 {
                let label = entry
                    .name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| entry.id.clone());
                approvals.push(label);
            }
        }
    }
    approvals.sort();
    approvals.dedup();
    if approvals.len() > 3 {
        approvals.truncate(3);
    }
    let primary_entry = entries
        .iter()
        .find(|entry| pending_map.contains_key(&entry.id))
        .or_else(|| entries.first());
    let mut vibe_average = None;
    let mut last_signal = None;
    let mut feedback_samples = None;
    let primary_label = primary_entry.map(|entry| {
        entry
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| entry.id.clone())
    });
    if let Some(entry) = primary_entry {
        let metrics = service.vibe_metrics_snapshot(entry.id.clone()).await;
        vibe_average = metrics.average_strength.map(|value| value as f64);
        last_signal = metrics.last_signal.clone();
        if metrics.total_feedback > 0 {
            feedback_samples = Some(metrics.total_feedback);
        }
    }
    let mut alerts = Vec::new();
    if approvals_pending > 0 {
        alerts.push(format!("Persona approvals pending ({approvals_pending})"));
    }
    Some(BriefPersonaSection {
        total,
        approvals_pending,
        primary_persona: primary_label,
        vibe_average,
        last_signal,
        feedback_samples,
        approvals,
        alerts,
    })
}

fn summarise_memory(state: &AppState) -> Option<BriefMemorySection> {
    let bus = state.bus();
    let snapshot = crate::context_metrics::snapshot(&bus);
    if snapshot.is_null() {
        return None;
    }
    let coverage_object = snapshot
        .get("coverage")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let coverage_needs_more_ratio = coverage_object
        .get("needs_more_ratio")
        .and_then(Value::as_f64);
    let top_reasons = coverage_object
        .get("top_reasons")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|value| {
                    let reason = value.get("reason").and_then(Value::as_str)?;
                    let count = value.get("count").and_then(Value::as_u64)?;
                    Some(format!("{reason} ({count})"))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let recall_object = snapshot
        .get("recall_risk")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let recall_risk_ratio = recall_object.get("risk_ratio").and_then(Value::as_f64);

    if coverage_needs_more_ratio.is_none() && recall_risk_ratio.is_none() && top_reasons.is_empty()
    {
        return None;
    }
    let mut alerts = Vec::new();
    if let Some(ratio) = coverage_needs_more_ratio {
        if ratio > 0.25 {
            alerts.push(format!("Memory coverage gaps {:.0}%", ratio * 100.0));
        }
    }
    if let Some(ratio) = recall_risk_ratio {
        if ratio > 0.2 {
            alerts.push(format!("Recall risk {:.0}%", ratio * 100.0));
        }
    }
    Some(BriefMemorySection {
        coverage_needs_more_ratio,
        top_reasons,
        recall_risk_ratio,
        alerts,
    })
}

async fn summarise_autonomy(state: &AppState) -> Option<BriefAutonomySection> {
    let lanes = state.autonomy().lanes().await;
    if lanes.is_empty() {
        return None;
    }
    let mut lanes_autonomous = 0usize;
    let mut lanes_paused = 0usize;
    let mut active_jobs = 0u64;
    let mut queued_jobs = 0u64;
    let mut alerts = BTreeSet::new();
    for lane in &lanes {
        match lane.mode {
            AutonomyMode::Autonomous => lanes_autonomous += 1,
            AutonomyMode::Paused => {
                lanes_paused += 1;
                alerts.insert(format!("Autonomy lane {} paused", lane.lane_id));
            }
            AutonomyMode::Guided => {}
        }
        active_jobs += lane.active_jobs;
        queued_jobs += lane.queued_jobs;
        for alert in &lane.alerts {
            alerts.insert(format!("{}: {}", lane.lane_id, alert));
        }
        if let Some(budgets) = &lane.budgets {
            if let Some(remaining) = budgets.wall_clock_remaining_secs {
                if remaining == 0 {
                    alerts.insert(format!("{} budget exhausted", lane.lane_id));
                } else if remaining < 900 {
                    alerts.insert(format!("{} budget low ({}s)", lane.lane_id, remaining));
                }
            }
            if let Some(tokens) = budgets.tokens_remaining {
                if tokens == 0 {
                    alerts.insert(format!("{} token budget exhausted", lane.lane_id));
                }
            }
        }
    }
    Some(BriefAutonomySection {
        lanes_total: lanes.len(),
        lanes_autonomous,
        lanes_paused,
        active_jobs,
        queued_jobs,
        alerts: alerts.into_iter().collect(),
    })
}

fn render_summary(
    economy: &Option<BriefEconomySection>,
    runtime: &Option<BriefRuntimeSection>,
    persona: &Option<BriefPersonaSection>,
    memory: &Option<BriefMemorySection>,
    autonomy: &Option<BriefAutonomySection>,
) -> String {
    let mut parts = Vec::new();
    if let Some(economy) = economy {
        if let Some(primary) = economy.totals.first() {
            parts.push(format!(
                "{} settled {}, pending {}",
                primary.currency,
                format_amount(primary.settled),
                format_amount(primary.pending)
            ));
        } else {
            parts.push("Economy awaiting activity".to_string());
        }
    }
    if let Some(runtime) = runtime {
        let ready = runtime
            .by_state
            .get(arw_runtime::RuntimeState::Ready.display_label())
            .copied()
            .unwrap_or(0);
        parts.push(format!("Runtimes ready {ready}/{}", runtime.total));
        if !runtime.alerts.is_empty() {
            parts.push(format!(
                "{} runtime alert{}",
                runtime.alerts.len(),
                if runtime.alerts.len() == 1 { "" } else { "s" }
            ));
        }
    }
    if let Some(persona) = persona {
        if persona.total > 0 {
            let mut line = format!("Personas {}", persona.total);
            if persona.approvals_pending > 0 {
                line.push_str(&format!(
                    " ({} approval{})",
                    persona.approvals_pending,
                    if persona.approvals_pending == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
            if let Some(avg) = persona.vibe_average {
                line.push_str(&format!(" vibe {:.0}%", avg * 100.0));
            }
            parts.push(line);
        } else {
            parts.push("No personas yet".to_string());
        }
    }
    if let Some(memory) = memory {
        if let Some(ratio) = memory.coverage_needs_more_ratio {
            if ratio > 0.0 {
                parts.push(format!("Memory gaps {:.0}%", ratio * 100.0));
            }
        }
        if let Some(risk) = memory.recall_risk_ratio {
            if risk > 0.0 {
                parts.push(format!("Recall risk {:.0}%", risk * 100.0));
            }
        }
    }
    if let Some(autonomy) = autonomy {
        if autonomy.lanes_total > 0 {
            let mut line = format!(
                "Autonomy {}/{} auto",
                autonomy.lanes_autonomous, autonomy.lanes_total
            );
            if autonomy.lanes_paused > 0 {
                line.push_str(&format!(" ({} paused)", autonomy.lanes_paused));
            }
            if autonomy.active_jobs > 0 {
                line.push_str(&format!(
                    ", {} active job{}",
                    autonomy.active_jobs,
                    if autonomy.active_jobs == 1 { "" } else { "s" }
                ));
            }
            parts.push(line);
        }
    }
    if parts.is_empty() {
        "No brief data available yet.".to_string()
    } else {
        parts.join(" • ")
    }
}

fn runtime_name(record: &arw_runtime::RuntimeRecord) -> String {
    record
        .descriptor
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| record.descriptor.id.clone())
}

fn format_amount(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let rounded = (value * 100.0).round() / 100.0;
    if (rounded - rounded.trunc()).abs() < f64::EPSILON {
        format!("{:.0}", rounded)
    } else {
        format!("{:.2}", rounded)
    }
}

fn issued_ts(entry: &EconomyLedgerEntry) -> i64 {
    entry
        .issued_at
        .as_ref()
        .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

fn build_tags(entry: &EconomyLedgerEntry) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(job) = &entry.job_id {
        tags.push(job.clone());
    }
    if let Some(contract) = &entry.contract_id {
        tags.push(contract.clone());
    }
    if let Some(persona) = &entry.persona_id {
        tags.push(persona.clone());
    }
    tags
}

fn interval_from_env() -> Duration {
    let secs = std::env::var("ARW_DAILY_BRIEF_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS);
    Duration::from_secs(secs)
}
