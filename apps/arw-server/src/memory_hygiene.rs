use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use metrics::counter;
use serde_json::json;
use tokio::time::{interval, MissedTickBehavior};

use crate::{read_models, tasks::TaskHandle, tools, AppState};
use arw_kernel::{MemoryGcCandidate, MemoryGcReason};
use arw_topics as topics;

const DEFAULT_INTERVAL_SECS: u64 = 60;
const DEFAULT_BATCH_LIMIT: usize = 128;

const METRIC_EXPIRED: &str = "arw_memory_gc_expired_total";
const METRIC_EVICTED: &str = "arw_memory_gc_evicted_total";

static DEFAULT_LANE_CAPS: &[(&str, usize)] = &[
    ("ephemeral", 256),
    ("short_term", 512),
    ("episodic", 1024),
    ("episodic_summary", 1024),
    ("semantic", 4096),
    ("profile", 512),
];

pub(crate) fn start(state: AppState) -> TaskHandle {
    let mut ticker = interval(Duration::from_secs(gc_interval_secs()));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    TaskHandle::new(
        "memory.hygiene",
        tokio::spawn(async move {
            loop {
                ticker.tick().await;
                if !state.kernel_enabled() {
                    continue;
                }
                if let Err(err) = sweep_once(&state).await {
                    tracing::warn!(target: "arw::memory", error = %err, "memory hygiene sweep failed");
                }
            }
        }),
    )
}

async fn sweep_once(state: &AppState) -> Result<()> {
    let limit = gc_batch_limit();
    if limit == 0 {
        return Ok(());
    }

    let now = Utc::now();
    let mut removed = Vec::new();
    let mut reasons = Vec::new();
    let mut seen = HashSet::new();

    let expired = state
        .kernel()
        .expired_memory_candidates_async(now, limit)
        .await
        .context("collect expired memory entries")?;
    for cand in expired {
        if seen.insert(cand.id.clone()) {
            let id = cand.id.clone();
            removed.push(id);
            reasons.push(cand);
        }
    }

    let mut remaining = limit.saturating_sub(removed.len());
    if remaining > 0 {
        let caps = lane_caps_from_env();
        for (lane, cap) in caps {
            if cap == 0 || remaining == 0 {
                continue;
            }
            let candidates = state
                .kernel()
                .lane_overflow_candidates_async(lane.clone(), cap, remaining)
                .await
                .with_context(|| format!("collect overflow for lane {lane}"))?;
            for cand in candidates {
                if seen.insert(cand.id.clone()) {
                    remaining = remaining.saturating_sub(1);
                    let id = cand.id.clone();
                    removed.push(id);
                    reasons.push(cand);
                    if remaining == 0 {
                        break;
                    }
                }
            }
            if remaining == 0 {
                break;
            }
        }
    }

    if removed.is_empty() {
        return Ok(());
    }

    state
        .kernel()
        .delete_memory_records_async(removed.clone())
        .await
        .context("delete reclaimed memory records")?;

    publish_events(state, &reasons);
    update_read_model(state).await?;

    let expired_count = reasons
        .iter()
        .filter(|c| matches!(c.reason, MemoryGcReason::TtlExpired { .. }))
        .count();
    if expired_count > 0 {
        counter!(METRIC_EXPIRED).increment(expired_count as u64);
    }
    let evicted_count = reasons.len() - expired_count;
    if evicted_count > 0 {
        counter!(METRIC_EVICTED).increment(evicted_count as u64);
    }

    tracing::debug!(
        target: "arw::memory",
        expired = expired_count,
        evicted = evicted_count,
        total = reasons.len(),
        "memory hygiene reclaimed records",
    );

    state
        .metrics()
        .record_memory_gc(expired_count as u64, evicted_count as u64);

    Ok(())
}

fn publish_events(state: &AppState, candidates: &[MemoryGcCandidate]) {
    let bus = state.bus();
    for cand in candidates {
        let mut payload = json!({
            "id": cand.id,
            "lane": cand.lane,
            "kind": cand.kind,
            "project_id": cand.project_id,
            "agent_id": cand.agent_id,
            "durability": cand.durability,
            "created": cand.created,
            "updated": cand.updated,
            "ttl_s": cand.ttl_s,
        });
        match &cand.reason {
            MemoryGcReason::TtlExpired { ttl_s, expired_at } => {
                payload["reason"] = json!("ttl_expired");
                payload["expired_at"] = json!(expired_at);
                payload["ttl_s"] = json!(*ttl_s);
            }
            MemoryGcReason::LaneCap { cap, overflow } => {
                payload["reason"] = json!("lane_cap");
                payload["cap"] = json!(*cap as u64);
                payload["overflow"] = json!(*overflow as u64);
            }
        }
        tools::ensure_corr(&mut payload);
        bus.publish(topics::TOPIC_MEMORY_ITEM_EXPIRED, &payload);
    }
}

async fn update_read_model(state: &AppState) -> Result<()> {
    let items = state
        .kernel()
        .list_recent_memory_async(None, 200)
        .await
        .context("refresh memory recent read-model")?;
    let bundle = read_models::build_memory_recent_bundle(items);
    read_models::publish_memory_bundle(&state.bus(), &bundle);
    Ok(())
}

fn gc_interval_secs() -> u64 {
    std::env::var("ARW_MEMORY_GC_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|val| *val > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

fn gc_batch_limit() -> usize {
    std::env::var("ARW_MEMORY_GC_BATCH")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .map(|n| n.max(0) as usize)
        .unwrap_or(DEFAULT_BATCH_LIMIT)
}

fn lane_caps_from_env() -> Vec<(String, usize)> {
    let mut caps: Vec<(String, usize)> = DEFAULT_LANE_CAPS
        .iter()
        .map(|(lane, cap)| ((*lane).to_string(), *cap))
        .collect();
    if let Ok(raw) = std::env::var("ARW_MEMORY_LANE_CAPS") {
        for entry in raw.split(',') {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Some((lane, value)) = trimmed.split_once('=') else {
                continue;
            };
            if let Ok(cap) = value.trim().parse::<i64>() {
                apply_lane_cap(&mut caps, lane.trim(), cap);
            }
        }
    }
    caps
}

fn apply_lane_cap(caps: &mut Vec<(String, usize)>, lane: &str, cap: i64) {
    if cap <= 0 {
        caps.retain(|(name, _)| name != lane);
        return;
    }
    if let Some(existing) = caps.iter_mut().find(|(name, _)| name == lane) {
        existing.1 = cap as usize;
    } else {
        caps.push((lane.to_string(), cap as usize));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env as test_env;

    #[test]
    fn interval_respects_env_and_floor() {
        let mut guard = test_env::guard();
        guard.set("ARW_MEMORY_GC_INTERVAL_SECS", "120");
        assert_eq!(gc_interval_secs(), 120);
        guard.set("ARW_MEMORY_GC_INTERVAL_SECS", "0");
        assert_eq!(gc_interval_secs(), DEFAULT_INTERVAL_SECS);
        guard.remove("ARW_MEMORY_GC_INTERVAL_SECS");
        assert_eq!(gc_interval_secs(), DEFAULT_INTERVAL_SECS);
    }

    #[test]
    fn batch_limit_clamps_to_zero_and_default() {
        let mut guard = test_env::guard();
        guard.set("ARW_MEMORY_GC_BATCH", "32");
        assert_eq!(gc_batch_limit(), 32);
        guard.set("ARW_MEMORY_GC_BATCH", "-10");
        assert_eq!(gc_batch_limit(), 0);
        guard.remove("ARW_MEMORY_GC_BATCH");
        assert_eq!(gc_batch_limit(), DEFAULT_BATCH_LIMIT);
    }

    #[test]
    fn lane_caps_default_and_overrides() {
        let mut guard = test_env::guard();
        guard.set(
            "ARW_MEMORY_LANE_CAPS",
            "episodic=2048, semantic=8192 ,profile=0 , custom=300",
        );
        let caps = lane_caps_from_env();
        assert_eq!(
            caps,
            vec![
                ("ephemeral".to_string(), 256),
                ("short_term".to_string(), 512),
                ("episodic".to_string(), 2048),
                ("episodic_summary".to_string(), 1024),
                ("semantic".to_string(), 8192),
                ("custom".to_string(), 300),
            ]
        );
        guard.remove("ARW_MEMORY_LANE_CAPS");
        let defaults = lane_caps_from_env();
        assert_eq!(
            defaults,
            vec![
                ("ephemeral".to_string(), 256),
                ("short_term".to_string(), 512),
                ("episodic".to_string(), 1024),
                ("episodic_summary".to_string(), 1024),
                ("semantic".to_string(), 4096),
                ("profile".to_string(), 512),
            ]
        );
    }
}
