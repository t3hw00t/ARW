use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use arw_events::Bus;
use arw_topics as topics;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tracing::warn;
use utoipa::ToSchema;

use crate::responses;

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
        }
    }
}

#[derive(Copy, Clone)]
pub enum FlushScope {
    All,
    QueuedOnly,
    InFlightOnly,
}

pub struct AutonomyRegistry {
    bus: Bus,
    lanes: RwLock<HashMap<String, AutonomyLaneSnapshot>>,
    path: PathBuf,
}

impl AutonomyRegistry {
    pub async fn new(bus: Bus) -> Arc<Self> {
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

        Arc::new(Self {
            bus,
            lanes: RwLock::new(initial),
            path,
        })
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
        self.persist().await;

        let topic = match mode {
            AutonomyMode::Paused => topics::TOPIC_AUTONOMY_RUN_PAUSED,
            AutonomyMode::Guided => topics::TOPIC_AUTONOMY_RUN_RESUMED,
            AutonomyMode::Autonomous => topics::TOPIC_AUTONOMY_RUN_STARTED,
        };
        self.publish_event(topic, &snapshot, operator, reason);
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

    pub async fn flush_jobs(&self, lane_id: &str, scope: FlushScope) -> AutonomyLaneSnapshot {
        let snapshot = self
            .update_lane(lane_id, |lane| {
                lane.last_event = Some("jobs_flushed".to_string());
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
        self.persist().await;

        let reason = match scope {
            FlushScope::All => Some("all".to_string()),
            FlushScope::QueuedOnly => Some("queued".to_string()),
            FlushScope::InFlightOnly => Some("in_flight".to_string()),
        };
        self.publish_event(topics::TOPIC_AUTONOMY_INTERRUPT, &snapshot, None, reason);
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
        self.persist().await;
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
            })
            .await;
        self.persist().await;

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
        lane.clone()
    }

    async fn persist(&self) {
        let snapshot = {
            let guard = self.lanes.read().await;
            guard.values().cloned().collect::<Vec<_>>()
        };
        if let Some(parent) = self.path.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                warn!(?err, path=%self.path.display(), "failed to create autonomy state dir");
                return;
            }
        }
        match serde_json::to_vec_pretty(&snapshot) {
            Ok(bytes) => {
                if let Err(err) = tokio::fs::write(&self.path, bytes).await {
                    warn!(?err, path=%self.path.display(), "failed to persist autonomy lanes");
                }
            }
            Err(err) => warn!(?err, "failed to serialize autonomy lanes"),
        }
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
        let registry = AutonomyRegistry::new(bus).await;

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
        let registry = AutonomyRegistry::new(bus).await;

        registry
            .record_job_counts("trial-lane", Some(3), Some(7))
            .await;

        let after_flush = registry
            .flush_jobs("trial-lane", FlushScope::InFlightOnly)
            .await;
        assert_eq!(after_flush.active_jobs, 0);
        assert_eq!(after_flush.queued_jobs, 7);

        let final_state = registry.flush_jobs("trial-lane", FlushScope::All).await;
        assert_eq!(final_state.queued_jobs, 0);
    }
}
