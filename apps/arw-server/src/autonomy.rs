use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arw_events::Bus;
use arw_topics as topics;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::timeout;
use tracing::warn;
use utoipa::ToSchema;

use crate::{metrics, responses};

const ALERT_BUDGET_NEAR: &str = "Budgets nearing limit";
const ALERT_BUDGET_EXHAUSTED: &str = "Budgets exhausted";
const PERSIST_DEBOUNCE: Duration = Duration::from_millis(120);
const PERSIST_RETRY_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyMode {
    #[default]
    Guided,
    Autonomous,
    Paused,
}

impl AutonomyMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AutonomyMode::Guided => "guided",
            AutonomyMode::Autonomous => "autonomous",
            AutonomyMode::Paused => "paused",
        }
    }
}

impl std::str::FromStr for AutonomyMode {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "guided" | "resume" => Ok(AutonomyMode::Guided),
            "autonomous" | "autonomy" | "auto" => Ok(AutonomyMode::Autonomous),
            "paused" | "pause" => Ok(AutonomyMode::Paused),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct AutonomyBudgets {
    #[serde(default)]
    pub wall_clock_remaining_secs: Option<u64>,
    #[serde(default)]
    pub tokens_remaining: Option<u64>,
    #[serde(default)]
    pub spend_remaining_cents: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct AutonomySignal {
    pub lane: String,
    pub kind: AutonomySignalKind,
    #[allow(dead_code)]
    pub issued_ms: u64,
}

#[derive(Clone, Debug)]
pub enum AutonomySignalKind {
    ModeChanged {
        mode: AutonomyMode,
        operator: Option<String>,
        reason: Option<String>,
    },
    Flush {
        scope: FlushScope,
    },
}

impl AutonomySignalKind {
    pub fn interrupts_execution(&self) -> bool {
        match self {
            AutonomySignalKind::ModeChanged { mode, .. } => matches!(mode, AutonomyMode::Paused),
            AutonomySignalKind::Flush { scope } => !matches!(scope, FlushScope::QueuedOnly),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct AutonomyLaneSnapshot {
    pub lane_id: String,
    #[serde(default)]
    pub mode: AutonomyMode,
    #[serde(default)]
    pub active_jobs: u64,
    #[serde(default)]
    pub queued_jobs: u64,
    #[serde(default)]
    pub last_event: Option<String>,
    #[serde(default)]
    pub last_operator: Option<String>,
    #[serde(default)]
    pub last_reason: Option<String>,
    #[serde(default)]
    pub updated_ms: Option<u64>,
    #[serde(default)]
    pub budgets: Option<AutonomyBudgets>,
    #[serde(default)]
    pub alerts: Vec<String>,
    #[serde(default)]
    pub last_budget_update_ms: Option<u64>,
}

impl AutonomyLaneSnapshot {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            lane_id: id.into(),
            mode: AutonomyMode::Guided,
            active_jobs: 0,
            queued_jobs: 0,
            last_event: None,
            last_operator: None,
            last_reason: None,
            updated_ms: None,
            budgets: None,
            alerts: Vec::new(),
            last_budget_update_ms: None,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlushScope {
    All,
    QueuedOnly,
    InFlightOnly,
}

pub struct AutonomyRegistry {
    bus: Bus,
    lanes: RwLock<HashMap<String, AutonomyLaneSnapshot>>,
    path: PathBuf,
    interrupts: broadcast::Sender<AutonomySignal>,
    metrics: Arc<metrics::Metrics>,
    persist_tx: mpsc::Sender<()>,
}

impl AutonomyRegistry {
    pub async fn new(bus: Bus, metrics: Arc<metrics::Metrics>) -> Arc<Self> {
        let path = crate::util::state_dir().join("autonomy").join("lanes.json");
        let initial = match tokio::fs::read(&path).await {
            Ok(bytes) => match serde_json::from_slice::<Vec<AutonomyLaneSnapshot>>(&bytes) {
                Ok(entries) => entries
                    .into_iter()
                    .map(|lane| (lane.lane_id.clone(), lane))
                    .collect::<HashMap<_, _>>(),
                Err(err) => {
                    warn!(?err, "failed to parse autonomy lane state, starting fresh");
                    HashMap::new()
                }
            },
            Err(err) if err.kind() == ErrorKind::NotFound => HashMap::new(),
            Err(err) => {
                warn!(?err, path=%path.display(), "failed to read autonomy lane state");
                HashMap::new()
            }
        };
        let mut initial = initial;
        for lane in initial.values_mut() {
            Self::normalize_alerts(lane);
        }

        let (persist_tx, persist_rx) = mpsc::channel(16);
        let (interrupts, _) = broadcast::channel(32);
        let registry = Arc::new(Self {
            bus,
            lanes: RwLock::new(initial),
            path,
            interrupts,
            metrics,
            persist_tx,
        });
        registry.spawn_persist_worker(persist_rx);
        registry
    }

    pub async fn lanes(&self) -> Vec<AutonomyLaneSnapshot> {
        let mut items = {
            let guard = self.lanes.read().await;
            guard.values().cloned().collect::<Vec<_>>()
        };
        items.sort_by(|a, b| a.lane_id.cmp(&b.lane_id));
        items
    }

    pub async fn lane(&self, lane_id: &str) -> Option<AutonomyLaneSnapshot> {
        let guard = self.lanes.read().await;
        guard.get(lane_id).cloned()
    }

    pub async fn is_any_paused(&self) -> bool {
        let guard = self.lanes.read().await;
        guard
            .values()
            .any(|lane| matches!(lane.mode, AutonomyMode::Paused))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AutonomySignal> {
        self.interrupts.subscribe()
    }

    pub async fn set_lane_mode(
        &self,
        lane_id: &str,
        mode: AutonomyMode,
        operator: Option<String>,
        reason: Option<String>,
    ) -> AutonomyLaneSnapshot {
        let operator_clone = operator.clone();
        let reason_clone = reason.clone();
        let mode_clone = mode.clone();
        let previous_mode = self.lane(lane_id).await.map(|lane| lane.mode);
        let snapshot = self
            .update_lane(lane_id, |lane| {
                lane.mode = mode_clone.clone();
                lane.last_event = Some(match mode_clone {
                    AutonomyMode::Paused => "paused".to_string(),
                    AutonomyMode::Guided => "resumed".to_string(),
                    AutonomyMode::Autonomous => "autonomous".to_string(),
                });
                lane.last_operator = operator_clone.clone();
                lane.last_reason = reason_clone.clone();
            })
            .await;
        self.schedule_persist().await;

        let topic = match mode {
            AutonomyMode::Paused => topics::TOPIC_AUTONOMY_RUN_PAUSED,
            AutonomyMode::Guided => topics::TOPIC_AUTONOMY_RUN_RESUMED,
            AutonomyMode::Autonomous => topics::TOPIC_AUTONOMY_RUN_STARTED,
        };
        self.publish_event(topic, &snapshot, operator, reason);
        if matches!(mode, AutonomyMode::Paused) {
            self.metrics.record_autonomy_interrupt("pause");
        }
        if previous_mode != Some(mode_clone.clone()) {
            self.emit_signal(
                lane_id,
                AutonomySignalKind::ModeChanged {
                    mode: mode_clone,
                    operator: operator_clone,
                    reason: reason_clone,
                },
            );
        }
        snapshot
    }

    pub async fn pause_lane(
        &self,
        lane_id: &str,
        operator: Option<String>,
        reason: Option<String>,
    ) -> AutonomyLaneSnapshot {
        self.set_lane_mode(lane_id, AutonomyMode::Paused, operator, reason)
            .await
    }

    pub async fn resume_lane(
        &self,
        lane_id: &str,
        operator: Option<String>,
        reason: Option<String>,
    ) -> AutonomyLaneSnapshot {
        self.set_lane_mode(lane_id, AutonomyMode::Guided, operator, reason)
            .await
    }

    pub async fn flush_jobs(
        &self,
        lane_id: &str,
        scope: FlushScope,
        operator: Option<String>,
        reason: Option<String>,
    ) -> AutonomyLaneSnapshot {
        let operator_clone = operator.clone();
        let reason_clone = reason.clone();
        let snapshot = self
            .update_lane(lane_id, |lane| {
                lane.last_event = Some(match scope {
                    FlushScope::QueuedOnly => "jobs_flushed".to_string(),
                    FlushScope::InFlightOnly | FlushScope::All => "stopped".to_string(),
                });
                if let Some(op) = operator_clone.clone() {
                    lane.last_operator = Some(op);
                }
                if let Some(rs) = reason_clone.clone() {
                    lane.last_reason = Some(rs);
                }
                match scope {
                    FlushScope::All => {
                        lane.active_jobs = 0;
                        lane.queued_jobs = 0;
                    }
                    FlushScope::QueuedOnly => {
                        lane.queued_jobs = 0;
                    }
                    FlushScope::InFlightOnly => {
                        lane.active_jobs = 0;
                    }
                }
            })
            .await;
        self.schedule_persist().await;

        let scope_reason = match scope {
            FlushScope::All => Some("all".to_string()),
            FlushScope::QueuedOnly => Some("queued".to_string()),
            FlushScope::InFlightOnly => Some("in_flight".to_string()),
        };
        let interrupt_reason = reason_clone.clone().or_else(|| scope_reason.clone());
        self.publish_event(
            topics::TOPIC_AUTONOMY_INTERRUPT,
            &snapshot,
            operator.clone(),
            interrupt_reason,
        );
        if matches!(scope, FlushScope::All | FlushScope::InFlightOnly) {
            self.publish_event(
                topics::TOPIC_AUTONOMY_RUN_STOPPED,
                &snapshot,
                operator,
                reason_clone,
            );
        }
        self.metrics
            .record_autonomy_interrupt(metric_reason_for_scope(scope));
        self.emit_signal(lane_id, AutonomySignalKind::Flush { scope });
        self.schedule_persist().await;
        snapshot
    }

    pub async fn record_job_counts(
        &self,
        lane_id: &str,
        active_jobs: Option<u64>,
        queued_jobs: Option<u64>,
    ) -> AutonomyLaneSnapshot {
        let snapshot = self
            .update_lane(lane_id, |lane| {
                if let Some(active) = active_jobs {
                    lane.active_jobs = active;
                }
                if let Some(queued) = queued_jobs {
                    lane.queued_jobs = queued;
                }
                lane.last_event
                    .get_or_insert_with(|| "jobs_updated".to_string());
            })
            .await;
        self.schedule_persist().await;
        snapshot
    }

    #[allow(dead_code)]
    #[allow(dead_code)]
    pub async fn update_budgets(
        &self,
        lane_id: &str,
        budgets: Option<AutonomyBudgets>,
    ) -> AutonomyLaneSnapshot {
        let snapshot = self
            .update_lane(lane_id, |lane| {
                lane.budgets = budgets.clone();
                lane.last_event
                    .get_or_insert_with(|| "budgets_updated".to_string());
                lane.last_budget_update_ms = Some(now_ms());
            })
            .await;
        self.schedule_persist().await;

        if snapshot.budgets.is_none() {
            return snapshot;
        }

        let BudgetFlags {
            close_to_limit,
            exhausted,
        } = budget_flags(&snapshot);
        if close_to_limit {
            self.publish_event(topics::TOPIC_AUTONOMY_BUDGET_CLOSE, &snapshot, None, None);
        }
        if exhausted {
            self.publish_event(
                topics::TOPIC_AUTONOMY_BUDGET_EXHAUSTED,
                &snapshot,
                None,
                None,
            );
        }
        snapshot
    }

    async fn schedule_persist(&self) {
        if let Err(err) = self.persist_tx.send(()).await {
            warn!(
                target: "autonomy",
                ?err,
                "failed to schedule autonomy state persist"
            );
        }
    }

    async fn update_lane<F>(&self, lane_id: &str, mut apply: F) -> AutonomyLaneSnapshot
    where
        F: FnMut(&mut AutonomyLaneSnapshot),
    {
        let mut guard = self.lanes.write().await;
        let lane = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| AutonomyLaneSnapshot::new(lane_id));
        apply(lane);
        lane.updated_ms = Some(now_ms());
        Self::normalize_alerts(lane);
        lane.clone()
    }

    async fn persist_snapshot(&self) -> Result<(), std::io::Error> {
        let snapshot = {
            let guard = self.lanes.read().await;
            guard.values().cloned().collect::<Vec<_>>()
        };
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = serde_json::to_vec(&snapshot)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        tokio::fs::write(&self.path, bytes).await
    }

    fn spawn_persist_worker(self: &Arc<Self>, mut rx: mpsc::Receiver<()>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(_) = rx.recv().await {
                let mut channel_closed = false;
                loop {
                    match timeout(PERSIST_DEBOUNCE, rx.recv()).await {
                        Ok(Some(_)) => continue,
                        Ok(None) => {
                            channel_closed = true;
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if let Err(err) = this.persist_snapshot().await {
                    warn!(
                        target: "autonomy",
                        ?err,
                        "failed to persist autonomy lanes; will retry"
                    );
                    tokio::time::sleep(PERSIST_RETRY_DELAY).await;
                    let tx = this.persist_tx.clone();
                    let _ = tx.try_send(());
                }
                if channel_closed {
                    break;
                }
            }
        });
    }

    fn publish_event(
        &self,
        topic: &str,
        lane: &AutonomyLaneSnapshot,
        operator: Option<String>,
        reason: Option<String>,
    ) {
        let mut payload = json!({
            "lane": lane.lane_id,
            "mode": lane.mode.as_str(),
            "active_jobs": lane.active_jobs,
            "queued_jobs": lane.queued_jobs,
            "updated_ms": lane.updated_ms,
            "operator": operator,
            "reason": reason,
        });
        responses::attach_corr(&mut payload);
        self.bus.publish(topic, &payload);
    }

    fn emit_signal(&self, lane_id: &str, kind: AutonomySignalKind) {
        let signal = AutonomySignal {
            lane: lane_id.to_string(),
            kind,
            issued_ms: now_ms(),
        };
        let _ = self.interrupts.send(signal);
    }
}

#[allow(dead_code)]
struct BudgetFlags {
    close_to_limit: bool,
    exhausted: bool,
}

#[allow(dead_code)]
fn budget_flags(lane: &AutonomyLaneSnapshot) -> BudgetFlags {
    let budgets = match &lane.budgets {
        Some(b) => b,
        None => {
            return BudgetFlags {
                close_to_limit: false,
                exhausted: false,
            }
        }
    };

    let wall = budgets.wall_clock_remaining_secs.unwrap_or(u64::MAX);
    let tokens = budgets.tokens_remaining.unwrap_or(u64::MAX);
    let spend = budgets.spend_remaining_cents.unwrap_or(u64::MAX);

    BudgetFlags {
        close_to_limit: wall <= 120 || tokens <= 5_000 || spend <= 500,
        exhausted: wall == 0 || tokens == 0 || spend == 0,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl AutonomyRegistry {
    fn normalize_alerts(lane: &mut AutonomyLaneSnapshot) {
        lane.alerts.retain(|alert| !alert.trim().is_empty());
        // Deduplicate while keeping the first occurrence order.
        let mut seen: Vec<String> = Vec::new();
        lane.alerts.retain(|alert| {
            if seen.iter().any(|existing| existing == alert) {
                false
            } else {
                seen.push(alert.clone());
                true
            }
        });

        // Remove budget alerts if budgets are absent.
        if lane.budgets.is_none() {
            lane.alerts
                .retain(|alert| alert != ALERT_BUDGET_NEAR && alert != ALERT_BUDGET_EXHAUSTED);
            return;
        }

        let flags = budget_flags(lane);
        lane.alerts
            .retain(|alert| alert != ALERT_BUDGET_NEAR && alert != ALERT_BUDGET_EXHAUSTED);
        if flags.exhausted {
            push_unique_alert(&mut lane.alerts, ALERT_BUDGET_EXHAUSTED);
        } else if flags.close_to_limit {
            push_unique_alert(&mut lane.alerts, ALERT_BUDGET_NEAR);
        }
    }
}

fn push_unique_alert(alerts: &mut Vec<String>, value: &str) {
    if !alerts.iter().any(|existing| existing == value) {
        alerts.push(value.to_string());
    }
}

fn metric_reason_for_scope(scope: FlushScope) -> &'static str {
    match scope {
        FlushScope::All => "stop_flush_all",
        FlushScope::QueuedOnly => "stop_flush_queued",
        FlushScope::InFlightOnly => "stop_flush_inflight",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use tempfile::tempdir;

    #[tokio::test]
    async fn pause_and_resume_lane() {
        test_support::init_tracing();
        let dir = tempdir().unwrap();
        let _ctx = test_support::begin_state_env(dir.path());
        let bus = Bus::new(32);
        let metrics = Arc::new(metrics::Metrics::default());
        let registry = AutonomyRegistry::new(bus, metrics.clone()).await;

        let paused = registry
            .pause_lane(
                "trial-g4-autonomy",
                Some("alice".to_string()),
                Some("manual halt".to_string()),
            )
            .await;
        assert_eq!(paused.mode, AutonomyMode::Paused);
        assert_eq!(paused.last_operator.as_deref(), Some("alice"));

        let resumed = registry
            .resume_lane(
                "trial-g4-autonomy",
                Some("alice".to_string()),
                Some("resume guided".to_string()),
            )
            .await;
        assert_eq!(resumed.mode, AutonomyMode::Guided);
        assert_eq!(resumed.last_event.as_deref(), Some("resumed"));
    }

    #[tokio::test]
    async fn flush_jobs_resets_counts() {
        test_support::init_tracing();
        let dir = tempdir().unwrap();
        let _ctx = test_support::begin_state_env(dir.path());
        let bus = Bus::new(8);
        let metrics = Arc::new(metrics::Metrics::default());
        let registry = AutonomyRegistry::new(bus, metrics.clone()).await;

        registry
            .record_job_counts("trial-lane", Some(3), Some(7))
            .await;

        let after_flush = registry
            .flush_jobs("trial-lane", FlushScope::InFlightOnly, None, None)
            .await;
        assert_eq!(after_flush.active_jobs, 0);
        assert_eq!(after_flush.queued_jobs, 7);

        let final_state = registry
            .flush_jobs("trial-lane", FlushScope::All, None, None)
            .await;
        assert_eq!(final_state.queued_jobs, 0);
    }

    #[tokio::test]
    async fn pause_and_flush_emit_signals() {
        test_support::init_tracing();
        let dir = tempdir().unwrap();
        let _ctx = test_support::begin_state_env(dir.path());
        let bus = Bus::new(8);
        let metrics = Arc::new(metrics::Metrics::default());
        let registry = AutonomyRegistry::new(bus, metrics.clone()).await;
        let mut rx = registry.subscribe();

        registry
            .pause_lane(
                "trial-g4-autonomy",
                Some("alice".into()),
                Some("kill switch".into()),
            )
            .await;
        let pause_signal = rx.recv().await.expect("pause signal");
        assert_eq!(pause_signal.lane, "trial-g4-autonomy");
        match pause_signal.kind {
            AutonomySignalKind::ModeChanged {
                mode,
                reason,
                operator,
            } => {
                assert_eq!(mode, AutonomyMode::Paused);
                assert_eq!(reason.as_deref(), Some("kill switch"));
                assert_eq!(operator.as_deref(), Some("alice"));
            }
            other => panic!("unexpected signal: {:?}", other),
        }

        registry
            .flush_jobs(
                "trial-g4-autonomy",
                FlushScope::InFlightOnly,
                Some("alice".into()),
                Some("kill switch".into()),
            )
            .await;
        let flush_signal = rx.recv().await.expect("flush signal");
        match flush_signal.kind {
            AutonomySignalKind::Flush { scope } => {
                assert_eq!(scope, FlushScope::InFlightOnly);
            }
            other => panic!("unexpected signal kind {:?}", other),
        }

        let summary = metrics.snapshot();
        assert_eq!(summary.autonomy.interrupts.get("pause"), Some(&1));
        assert_eq!(
            summary.autonomy.interrupts.get("stop_flush_inflight"),
            Some(&1)
        );
    }
}
