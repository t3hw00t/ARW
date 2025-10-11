use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use metrics::{counter, gauge};
use serde_json::{json, Value};
use tokio::{
    fs as afs,
    time::{interval, Duration as TokioDuration, MissedTickBehavior},
};
use tracing::{debug, warn};

use crate::{api::state, tasks::TaskHandle, util, AppState};

const TASK_NAME: &str = "context.cascade";
const CURSOR_FILE: &str = "context_cascade.cursor";
const DEFAULT_INTERVAL_SECS: u64 = 300;
const DEFAULT_EVENT_LIMIT: usize = 4096;
const DEFAULT_EPISODE_LIMIT: usize = 128;
const DEFAULT_PER_EPISODE_EVENT_LIMIT: usize = 2000;
const DEFAULT_COOLDOWN_SECS: i64 = 45;
const DEFAULT_MIN_EVENTS: usize = 3;
const DEFAULT_TTL_SECS: i64 = 60 * 60 * 24 * 30; // 30 days
const SUMMARY_LANE: &str = "episodic_summary";
const SUMMARY_KIND: &str = "episode.cascade";
const SUMMARY_VERSION: u32 = 1;
const OUTLINE_LIMIT: usize = 8;
const EXTRACT_LIMIT: usize = 6;
const TEXT_MAX_LEN: usize = 480;

#[derive(Default)]
struct CascadeStats {
    processed: usize,
    skipped: usize,
    cursor: u64,
}

pub(crate) fn start(state: AppState) -> TaskHandle {
    TaskHandle::new(
        TASK_NAME,
        tokio::spawn(async move {
            let mut ticker = interval(TokioDuration::from_secs(interval_secs()));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            if state.kernel_enabled() {
                if let Err(err) = run_once(&state).await {
                    warn!(target: TASK_NAME, error = %err, "initial cascade run failed");
                }
            }
            // Consume the immediate tick so the next wait spans a full interval.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if !state.kernel_enabled() {
                    continue;
                }
                match run_once(&state).await {
                    Ok(stats) => {
                        if stats.processed > 0 || stats.skipped > 0 {
                            debug!(
                                target: TASK_NAME,
                                processed = stats.processed,
                                skipped = stats.skipped,
                                cursor = stats.cursor,
                                "cascade run complete"
                            );
                        }
                    }
                    Err(err) => warn!(target: TASK_NAME, error = %err, "cascade run failed"),
                }
            }
        }),
    )
}

async fn run_once(state: &AppState) -> Result<CascadeStats> {
    let mut cursor = load_cursor().await?;
    let limit = event_limit();
    let events = state
        .kernel()
        .recent_events_async(
            limit as i64,
            if cursor > 0 {
                Some(cursor as i64)
            } else {
                None
            },
        )
        .await
        .context("fetch recent events for cascade")?;

    if events.is_empty() {
        return Ok(CascadeStats {
            cursor,
            ..Default::default()
        });
    }

    let per_episode_limit = per_episode_event_limit();
    let cooldown = cooldown_secs();
    let min_events = min_events_required();
    let max_episodes = episode_limit();

    let mut by_corr: HashMap<String, u64> = HashMap::new();
    for event in events {
        let corr = match event.corr_id {
            Some(ref cid) if !cid.is_empty() => cid.clone(),
            _ => continue,
        };
        let entry = by_corr.entry(corr).or_insert(0);
        if event.id > 0 {
            *entry = (*entry).max(event.id as u64);
        }
    }

    if by_corr.is_empty() {
        return Ok(CascadeStats {
            cursor,
            ..Default::default()
        });
    }

    let mut pairs: Vec<(String, u64)> = by_corr.into_iter().collect();
    // Process older correlations first so cursor advances monotonically.
    pairs.sort_by_key(|(_, max_id)| *max_id);
    if pairs.len() > max_episodes {
        pairs.truncate(max_episodes);
    }

    let mut processed_max = cursor;
    let mut processed = 0usize;
    let mut skipped = 0usize;
    let mut latest_event_time: Option<DateTime<Utc>> = None;

    for (corr_id, _) in pairs {
        let events = state
            .kernel()
            .events_by_corr_id_async(&corr_id, Some(per_episode_limit as i64))
            .await
            .with_context(|| format!("load events for corr_id {corr_id}"))?;
        if events.is_empty() {
            skipped += 1;
            continue;
        }
        let last_event_id = events.iter().map(|e| e.id.max(0)).max().unwrap_or(0) as u64;
        if last_event_id <= cursor {
            // Already accounted for.
            continue;
        }
        if !episode_ready(&events, cooldown, min_events) {
            skipped += 1;
            continue;
        }
        let episode = match state::episode_from_events(corr_id.clone(), events.clone()) {
            Some(ep) => ep,
            None => {
                skipped += 1;
                continue;
            }
        };
        let episode_value = episode.into_value();
        let Some(summary) = build_cascade_summary(&episode_value, last_event_id) else {
            skipped += 1;
            continue;
        };

        if let Some(last_time) = events.iter().rev().find_map(|ev| parse_time(&ev.time)) {
            latest_event_time = match latest_event_time {
                Some(existing) if existing >= last_time => Some(existing),
                _ => Some(last_time),
            };
        }

        if should_skip_existing(state, &summary.record_id, summary.stats.last_event_id).await? {
            processed_max = processed_max.max(summary.stats.last_event_id);
            continue;
        }

        persist_summary(state, summary).await?;
        processed += 1;
        processed_max = processed_max.max(last_event_id);
    }

    if processed_max > cursor {
        store_cursor(processed_max).await?;
        cursor = processed_max;
    }

    counter!("arw_context_cascade_processed_total").increment(processed as u64);
    counter!("arw_context_cascade_skipped_total").increment(skipped as u64);
    if processed_max > 0 {
        gauge!("arw_context_cascade_last_event_id").set(processed_max as f64);
    }
    gauge!("arw_context_cascade_processed_last").set(processed as f64);
    gauge!("arw_context_cascade_skipped_last").set(skipped as f64);
    if let Some(last_time) = latest_event_time {
        let age_ms = (Utc::now() - last_time).num_milliseconds().max(0) as f64;
        gauge!("arw_context_cascade_last_event_age_ms").set(age_ms);
    }

    Ok(CascadeStats {
        processed,
        skipped,
        cursor,
    })
}

async fn should_skip_existing(
    state: &AppState,
    record_id: &str,
    last_event_id: u64,
) -> Result<bool> {
    let existing = state
        .kernel()
        .get_memory_async(record_id.to_string())
        .await
        .context("load existing cascade summary")?;
    if let Some(record) = existing {
        let known = record
            .get("extra")
            .and_then(|v| v.get("last_event_id"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return Ok(known >= last_event_id);
    }
    Ok(false)
}

async fn persist_summary(state: &AppState, summary: CascadeSummary) -> Result<()> {
    let ttl_s = ttl_seconds();
    let mut record = arw_memory_core::MemoryInsertOwned {
        id: Some(summary.record_id.clone()),
        lane: SUMMARY_LANE.to_string(),
        kind: Some(SUMMARY_KIND.to_string()),
        key: Some(summary.key.clone()),
        value: summary.value.clone(),
        embed: None,
        embed_hint: None,
        tags: Some(summary.tags.clone()),
        score: summary.score,
        prob: summary.prob,
        agent_id: None,
        project_id: summary.project.clone(),
        text: Some(summary.text.clone()),
        durability: Some("short".to_string()),
        trust: None,
        privacy: Some("internal".to_string()),
        ttl_s: Some(ttl_s),
        keywords: None,
        entities: None,
        source: Some(summary.source.clone()),
        links: None,
        extra: Some(summary.extra.clone()),
        hash: None,
    };
    let hash = record.compute_hash();
    record.hash = Some(hash.clone());
    let (inserted_id, inserted_record) = state
        .kernel()
        .insert_memory_with_record_async(record)
        .await
        .context("insert cascade summary into memory")?;
    debug_assert_eq!(inserted_id, summary.record_id);
    debug_assert_eq!(
        inserted_record
            .get("lane")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        SUMMARY_LANE,
    );

    state.bus().publish(
        arw_topics::TOPIC_CONTEXT_CASCADE_UPDATED,
        &json!({
            "episode_id": summary.episode_id,
            "memory_id": summary.record_id,
            "last_event_id": summary.stats.last_event_id,
            "events": summary.stats.count,
            "errors": summary.stats.errors,
            "projects": summary.projects,
            "updated": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        }),
    );

    counter!("arw_context_cascade_written_total").increment(1);
    Ok(())
}

fn episode_ready(events: &[arw_kernel::EventRow], cooldown_secs: i64, min_events: usize) -> bool {
    if events.len() < min_events {
        return false;
    }
    let Some(last) = events.iter().rev().find_map(|e| parse_time(&e.time)) else {
        return true;
    };
    let threshold = Utc::now() - Duration::seconds(cooldown_secs.max(0));
    last <= threshold
}

fn parse_time(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn interval_secs() -> u64 {
    std::env::var("ARW_CONTEXT_CASCADE_INTERVAL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

fn event_limit() -> usize {
    std::env::var("ARW_CONTEXT_CASCADE_EVENT_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_EVENT_LIMIT)
}

fn episode_limit() -> usize {
    std::env::var("ARW_CONTEXT_CASCADE_EPISODE_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_EPISODE_LIMIT)
}

fn per_episode_event_limit() -> usize {
    std::env::var("ARW_CONTEXT_CASCADE_EPISODE_EVENT_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_PER_EPISODE_EVENT_LIMIT)
}

fn cooldown_secs() -> i64 {
    std::env::var("ARW_CONTEXT_CASCADE_COOLDOWN_SECS")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|v| *v >= 0)
        .unwrap_or(DEFAULT_COOLDOWN_SECS)
}

fn min_events_required() -> usize {
    std::env::var("ARW_CONTEXT_CASCADE_MIN_EVENTS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MIN_EVENTS)
}

fn ttl_seconds() -> i64 {
    std::env::var("ARW_CONTEXT_CASCADE_TTL_SECS")
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_TTL_SECS)
}

fn cursor_path() -> PathBuf {
    util::state_dir().join(CURSOR_FILE)
}

async fn load_cursor() -> Result<u64> {
    let path = cursor_path();
    match afs::read_to_string(&path).await {
        Ok(body) => {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                Ok(0)
            } else {
                trimmed.parse::<u64>().map_err(|err| err.into())
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err.into()),
    }
}

async fn store_cursor(cursor: u64) -> Result<()> {
    let path = cursor_path();
    if let Some(parent) = path.parent() {
        afs::create_dir_all(parent)
            .await
            .with_context(|| format!("create cursor dir {:?}", parent))?;
    }
    afs::write(&path, cursor.to_string())
        .await
        .with_context(|| format!("write cursor {:?}", path))
}

#[derive(Clone)]
struct CascadeSummary {
    episode_id: String,
    record_id: String,
    key: String,
    value: Value,
    text: String,
    tags: Vec<String>,
    score: Option<f64>,
    prob: Option<f64>,
    project: Option<String>,
    projects: Vec<String>,
    source: Value,
    extra: Value,
    stats: CascadeStatsMeta,
}

#[derive(Clone)]
struct CascadeStatsMeta {
    last_event_id: u64,
    count: usize,
    errors: usize,
}

fn build_cascade_summary(episode: &Value, last_event_id: u64) -> Option<CascadeSummary> {
    let episode_id = episode.get("id")?.as_str()?.trim().to_string();
    if episode_id.is_empty() {
        return None;
    }
    let events = episode.get("events")?.as_array()?;
    if events.is_empty() {
        return None;
    }

    let projects = to_string_vec(episode.get("projects"));
    let actors = to_string_vec(episode.get("actors"));
    let kinds = to_string_vec(episode.get("kinds"));
    let count = events.len();
    let errors = events
        .iter()
        .filter(|ev| ev.get("error").and_then(|v| v.as_bool()) == Some(true))
        .count();
    let first_kind = episode
        .get("first_kind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let last_kind = episode
        .get("last_kind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let duration_ms = episode
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();

    let primary_kinds = top_kinds(&kinds, 3);
    let abstract_text = build_abstract_text(
        count,
        errors,
        duration_ms,
        &projects,
        &actors,
        &primary_kinds,
    );

    let outline = build_outline(events);
    let outline_text = outline
        .iter()
        .take(3)
        .map(|item| item.as_str().unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" | ");

    let extract = build_extract(events);
    let sources = build_sources(&episode_id, events);

    let stats = CascadeStatsMeta {
        last_event_id,
        count,
        errors,
    };

    let value = json!({
        "episode_id": episode_id,
        "abstract": {
            "text": abstract_text,
            "errors": errors,
            "events": count,
            "duration_ms": duration_ms,
            "primary_kinds": primary_kinds,
        },
        "extract": extract,
        "outline": outline,
        "projects": projects,
        "actors": actors,
        "kinds": kinds,
        "sources": sources.clone(),
        "stats": {
            "last_event_id": last_event_id,
            "events": count,
            "errors": errors,
            "first_kind": first_kind,
            "last_kind": last_kind,
            "duration_ms": duration_ms,
        },
    });

    let projects_list = to_string_vec(value.get("projects"));

    let text = {
        let mut joined = format!("Episode {episode_id}: {abstract_text}");
        if !outline_text.is_empty() {
            joined.push_str(" :: ");
            joined.push_str(&outline_text);
        }
        truncate_text(joined, TEXT_MAX_LEN)
    };

    let score = if count > 0 {
        Some(((count - errors) as f64 / count as f64).clamp(0.0, 1.0))
    } else {
        None
    };
    let prob = score;

    let mut tags = vec!["cascade".to_string(), "episode".to_string()];
    for project in &projects_list {
        tags.push(format!("project:{project}"));
    }

    let project = projects_list.first().cloned();

    let extra = json!({
        "last_event_id": last_event_id,
        "version": SUMMARY_VERSION,
    });

    let record_id = format!("cascade:{episode_id}");
    let key = format!("episode:{episode_id}");

    Some(CascadeSummary {
        episode_id,
        record_id,
        key,
        value,
        text,
        tags,
        score,
        prob,
        project,
        projects: projects_list,
        source: sources.clone(),
        extra,
        stats,
    })
}

fn to_string_vec(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn build_abstract_text(
    count: usize,
    errors: usize,
    duration_ms: u64,
    projects: &[String],
    actors: &[String],
    primary_kinds: &[String],
) -> String {
    let mut parts = Vec::new();
    if count > 0 {
        parts.push(format!(
            "{count} event{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    if let Some(duration) = human_duration(duration_ms) {
        parts.push(format!("over {duration}"));
    }
    if errors > 0 {
        parts.push(format!(
            "{errors} error{}",
            if errors == 1 { "" } else { "s" }
        ));
    }
    if !primary_kinds.is_empty() {
        parts.push(format!("focus on {}", primary_kinds.join(", ")));
    }
    if !projects.is_empty() {
        parts.push(format!("projects {}", projects.join(", ")));
    }
    if !actors.is_empty() {
        parts.push(format!("actors {}", actors.join(", ")));
    }
    if parts.is_empty() {
        "Episode timeline recorded.".to_string()
    } else {
        format!("{}.", parts.join(", "))
    }
}

fn human_duration(duration_ms: u64) -> Option<String> {
    if duration_ms == 0 {
        return None;
    }
    let secs = duration_ms as f64 / 1000.0;
    if secs < 1.0 {
        return Some(format!("{:.0} ms", duration_ms));
    }
    if secs < 60.0 {
        return Some(format!("{:.1}s", secs));
    }
    let minutes = (secs / 60.0).floor();
    let seconds = secs % 60.0;
    if minutes < 60.0 {
        if seconds >= 1.0 {
            return Some(format!("{}m {:.0}s", minutes as i64, seconds));
        }
        return Some(format!("{}m", minutes as i64));
    }
    let hours = minutes / 60.0;
    Some(format!("{:.1}h", hours))
}

fn build_outline(events: &[Value]) -> Vec<Value> {
    let mut outline = Vec::new();
    for event in events.iter().take(OUTLINE_LIMIT) {
        let kind = event
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("event");
        let snippet = event_snippet(event);
        outline.push(json!(format!("{} — {}", short_kind(kind), snippet)));
    }
    outline
}

fn build_extract(events: &[Value]) -> Vec<Value> {
    use std::collections::BTreeSet;
    let mut selected = BTreeSet::new();
    if let Some(first) = events
        .first()
        .and_then(|ev| ev.get("id"))
        .and_then(|v| v.as_i64())
    {
        selected.insert(first);
    }
    if let Some(last) = events
        .iter()
        .rev()
        .find_map(|ev| ev.get("id").and_then(|v| v.as_i64()))
    {
        selected.insert(last);
    }
    for event in events {
        if event.get("error").and_then(|v| v.as_bool()) == Some(true) {
            if let Some(id) = event.get("id").and_then(|v| v.as_i64()) {
                selected.insert(id);
            }
        }
        if selected.len() >= EXTRACT_LIMIT {
            break;
        }
    }
    let mut extract = Vec::new();
    for event in events {
        if let Some(id) = event.get("id").and_then(|v| v.as_i64()) {
            if !selected.contains(&id) {
                continue;
            }
            let snippet = event_snippet(event);
            extract.push(json!({
                "event_id": id,
                "time": event.get("time"),
                "kind": event.get("kind"),
                "summary": snippet,
                "error": event.get("error"),
            }));
        }
    }
    extract
}

fn build_sources(episode_id: &str, events: &[Value]) -> Value {
    let event_ids: Vec<Value> = events
        .iter()
        .filter_map(|event| event.get("id"))
        .cloned()
        .collect();
    json!({
        "episode": episode_id,
        "event_ids": event_ids,
    })
}

fn event_snippet(event: &Value) -> String {
    if let Some(payload) = event.get("payload") {
        for key in ["summary", "message", "detail", "text"] {
            if let Some(snippet) = payload.get(key).and_then(|v| v.as_str()).map(str::trim) {
                if !snippet.is_empty() {
                    return truncate_text(snippet.to_string(), 160);
                }
            }
        }
    }
    event
        .get("kind")
        .and_then(|v| v.as_str())
        .map(short_kind)
        .unwrap_or_else(|| "event".to_string())
}

fn short_kind(kind: &str) -> String {
    kind.rsplit('.').next().unwrap_or(kind).to_string()
}

fn truncate_text(mut text: String, max_len: usize) -> String {
    if text.len() > max_len {
        text.truncate(max_len);
        text.push('…');
    }
    text
}

fn top_kinds(kinds: &[String], limit: usize) -> Vec<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for kind in kinds {
        let entry = counts.entry(kind.as_str()).or_insert(0);
        *entry += 1;
    }
    let mut pairs: Vec<(&str, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    pairs
        .into_iter()
        .take(limit)
        .map(|(kind, _)| kind.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: i64, kind: &str, seconds_ago: i64, _error: bool) -> arw_kernel::EventRow {
        let time = (Utc::now() - Duration::seconds(seconds_ago))
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        arw_kernel::EventRow {
            id,
            time,
            kind: kind.to_string(),
            actor: None,
            proj: Some("demo".to_string()),
            corr_id: Some("ep-123".to_string()),
            payload: json!({ "summary": format!("{kind} summary"), "proj": "demo" }),
        }
    }

    fn episode_value() -> Value {
        json!({
            "id": "ep-123",
            "duration_ms": 4200,
            "projects": ["demo"],
            "actors": ["agent:a"],
            "kinds": ["obs.text", "actions.tool"],
            "first_kind": "obs.text",
            "last_kind": "actions.tool",
            "events": [
                {
                    "id": 1,
                    "time": "2025-01-01T12:00:00Z",
                    "kind": "obs.text",
                    "payload": {"summary": "Observed input"}
                },
                {
                    "id": 2,
                    "time": "2025-01-01T12:00:01Z",
                    "kind": "actions.tool",
                    "payload": {"summary": "Tool output"},
                    "error": true
                },
                {
                    "id": 3,
                    "time": "2025-01-01T12:00:02Z",
                    "kind": "actions.tool",
                    "payload": {"message": "Completed"}
                }
            ]
        })
    }

    #[test]
    fn cascade_summary_builds_text() {
        let value = episode_value();
        let summary = build_cascade_summary(&value, 3).expect("summary");
        assert_eq!(summary.episode_id, "ep-123");
        assert!(summary.text.contains("3 events"));
        assert!(summary.value["outline"].is_array());
        assert_eq!(summary.stats.count, 3);
        assert_eq!(summary.stats.errors, 1);
        assert_eq!(summary.stats.last_event_id, 3);
    }

    #[test]
    fn episode_ready_respects_cooldown() {
        let events = vec![
            event(1, "obs.text", 120, false),
            event(2, "actions.tool", 5, false),
        ];
        assert!(!episode_ready(&events, 30, 2));
        assert!(episode_ready(&events, 2, 2));
    }
}
