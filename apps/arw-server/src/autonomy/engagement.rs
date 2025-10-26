use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::RwLock;

use super::AutonomyMode;

const DEFAULT_SCORE: f32 = 0.8;
const MIN_SCORE_FOR_AUTONOMY: f32 = 0.35;
const STALE_AFTER: Duration = Duration::from_secs(6 * 60 * 60); // 6h
const HALF_LIFE: Duration = Duration::from_secs(60 * 60); // 1h

#[derive(Clone, Debug)]
pub struct EngagementLedger {
    inner: Arc<RwLock<HashMap<String, LaneEngagement>>>,
    min_score: f32,
    stale_after: Duration,
    half_life: Duration,
}

#[derive(Clone, Debug, Default)]
struct LaneEngagement {
    score: f32,
    last_confirmation: Option<SystemTime>,
    last_touch: Option<SystemTime>,
    pending_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EngagementSnapshot {
    pub score: f32,
    pub stale_for: Option<Duration>,
    pub pending_reason: Option<String>,
    #[allow(dead_code)]
    pub last_confirmation: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngagementDecision {
    Allow {
        score: f32,
        stale_for: Option<Duration>,
    },
    NeedsAttention {
        score: f32,
        reason: String,
        stale_for: Option<Duration>,
    },
}

impl Default for EngagementLedger {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            min_score: MIN_SCORE_FOR_AUTONOMY,
            stale_after: STALE_AFTER,
            half_life: HALF_LIFE,
        }
    }
}

impl EngagementLedger {
    pub async fn record_confirmation(&self, lane_id: &str, boost: f32) {
        let boost = boost.clamp(0.0, 1.0);
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));
        entry.score = (entry.score + boost).min(1.0);
        entry.last_confirmation = Some(now);
        entry.last_touch = Some(now);
        entry.pending_reason = None;
    }

    pub async fn record_rejection(&self, lane_id: &str, penalty: f32, reason: impl Into<String>) {
        let penalty = penalty.clamp(0.0, 1.0);
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));
        entry.score = (entry.score - penalty).max(0.0);
        entry.last_touch = Some(now);
        entry.pending_reason = Some(reason.into());
    }

    #[allow(dead_code)]
    pub async fn flag_attention(&self, lane_id: &str, reason: impl Into<String>) {
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));
        entry.pending_reason = Some(reason.into());
        entry.last_touch = Some(now);
    }

    #[allow(dead_code)]
    pub async fn clear_attention(&self, lane_id: &str) {
        if let Some(entry) = self.inner.write().await.get_mut(lane_id) {
            entry.pending_reason = None;
        }
    }

    pub async fn record_mode_request(&self, lane_id: &str, _mode: AutonomyMode, allowed: bool) {
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));
        entry.last_touch = Some(now);
        if allowed && entry.last_confirmation.is_none() {
            entry.last_confirmation = Some(now);
        }
    }

    pub async fn decision_for_autonomy(&self, lane_id: &str) -> EngagementDecision {
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));

        let stale_for = entry
            .last_confirmation
            .and_then(|ts| now.duration_since(ts).ok());
        let effective_score = entry.effective_score(now, self.half_life);

        if let Some(reason) = entry.pending_reason.clone() {
            return EngagementDecision::NeedsAttention {
                score: effective_score,
                reason,
                stale_for,
            };
        }

        if let Some(duration) = stale_for {
            if duration >= self.stale_after {
                return EngagementDecision::NeedsAttention {
                    score: effective_score,
                    reason: format!("no confirmation for {}", format_duration_human(duration)),
                    stale_for: Some(duration),
                };
            }
        }

        if effective_score < self.min_score {
            return EngagementDecision::NeedsAttention {
                score: effective_score,
                reason: format!(
                    "confidence {:.2} below threshold {:.2}",
                    effective_score, self.min_score
                ),
                stale_for,
            };
        }

        EngagementDecision::Allow {
            score: effective_score,
            stale_for,
        }
    }

    pub async fn snapshot(&self, lane_id: &str) -> EngagementSnapshot {
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(lane_id.to_string())
            .or_insert_with(|| LaneEngagement::fresh(now));
        let score = entry.effective_score(now, self.half_life);
        let stale_for = entry
            .last_confirmation
            .and_then(|ts| now.duration_since(ts).ok());
        EngagementSnapshot {
            score,
            stale_for,
            pending_reason: entry.pending_reason.clone(),
            last_confirmation: entry.last_confirmation,
        }
    }

    pub async fn reset(&self, lane_id: &str) {
        let now = SystemTime::now();
        let mut guard = self.inner.write().await;
        guard.insert(lane_id.to_string(), LaneEngagement::fresh(now));
    }
}

impl LaneEngagement {
    fn fresh(now: SystemTime) -> Self {
        Self {
            score: DEFAULT_SCORE,
            last_confirmation: Some(now),
            last_touch: Some(now),
            pending_reason: None,
        }
    }

    fn effective_score(&mut self, now: SystemTime, half_life: Duration) -> f32 {
        let last = self.last_touch.or(self.last_confirmation).unwrap_or(now);
        let elapsed = now
            .duration_since(last)
            .unwrap_or_else(|_| Duration::from_secs(0));
        // Guard against immediate micro-decay right after a touch/reset to keep
        // responses stable within a short window and avoid test flakiness.
        if elapsed < Duration::from_millis(1000) {
            return self.score;
        }
        let factor = decay_factor(elapsed, half_life);
        self.score = (self.score * factor).clamp(0.0, 1.0);
        self.score
    }
}

fn decay_factor(elapsed: Duration, half_life: Duration) -> f32 {
    if half_life.is_zero() {
        return 1.0;
    }
    let elapsed_secs = elapsed.as_secs_f64();
    let half_life_secs = half_life.as_secs_f64();
    if half_life_secs <= 0.0 {
        return 1.0;
    }
    let exponent = -(elapsed_secs / half_life_secs) * std::f64::consts::LN_2;
    exponent.exp() as f32
}

fn format_duration_human(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    if total_secs >= 86_400 {
        let days = total_secs / 86_400;
        let hours = (total_secs % 86_400) / 3_600;
        let mut buf = String::new();
        let _ = write!(&mut buf, "{}d", days);
        if hours > 0 {
            let _ = write!(&mut buf, " {}h", hours);
        }
        return buf;
    }
    if total_secs >= 3_600 {
        let hours = total_secs / 3_600;
        let minutes = (total_secs % 3_600) / 60;
        let mut buf = String::new();
        let _ = write!(&mut buf, "{}h", hours);
        if minutes > 0 {
            let _ = write!(&mut buf, " {}m", minutes);
        }
        return buf;
    }
    if total_secs >= 60 {
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        let mut buf = String::new();
        let _ = write!(&mut buf, "{}m", minutes);
        if seconds > 0 {
            let _ = write!(&mut buf, " {}s", seconds);
        }
        return buf;
    }
    format!("{}s", total_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn confirmation_clears_pending_reason() {
        let ledger = EngagementLedger::default();
        ledger.flag_attention("lane-a", "needs review").await;
        assert!(matches!(
            ledger.decision_for_autonomy("lane-a").await,
            EngagementDecision::NeedsAttention { .. }
        ));
        ledger.record_confirmation("lane-a", 0.5).await;
        assert!(matches!(
            ledger.decision_for_autonomy("lane-a").await,
            EngagementDecision::Allow { .. }
        ));
    }

    #[tokio::test]
    async fn stale_confirmation_triggers_attention() {
        let ledger = EngagementLedger::default();
        ledger.record_confirmation("lane-a", 0.1).await;

        {
            let mut guard = ledger.inner.write().await;
            if let Some(entry) = guard.get_mut("lane-a") {
                entry.last_confirmation =
                    Some(SystemTime::now() - (STALE_AFTER + Duration::from_secs(30)));
            }
        }

        match ledger.decision_for_autonomy("lane-a").await {
            EngagementDecision::NeedsAttention { reason, .. } => {
                assert!(
                    reason.contains("no confirmation"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected attention, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn penalty_drops_score_below_threshold() {
        let ledger = EngagementLedger::default();
        ledger.record_confirmation("lane", 0.1).await;
        ledger.record_rejection("lane", 1.0, "mismatch").await;
        match ledger.decision_for_autonomy("lane").await {
            EngagementDecision::NeedsAttention { reason, .. } => {
                assert!(reason.contains("mismatch"), "unexpected reason: {reason}");
            }
            other => panic!("expected needs attention, got {other:?}"),
        }
    }
}
