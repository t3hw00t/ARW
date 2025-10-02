use serde_json::{json, Value};
use std::path::Path;
use tokio::fs as afs;
use tracing::warn;

use crate::{responses, state_observer, tasks::TaskHandle, util, AppState};
use arw_topics as topics;

/// Run a single distillation pass: stitch recent intents/actions into compact playbooks,
/// persist belief snapshots for ops/debug, prune stale world versions, and emit an event.
pub(crate) async fn run_once(state: &AppState) -> Value {
    let (intents_version, intents) = state_observer::intents_snapshot().await;
    let (actions_version, actions) = state_observer::actions_snapshot().await;
    let (beliefs_version, beliefs_items) = state_observer::beliefs_snapshot().await;
    let playbooks_version = intents_version.max(actions_version);

    let mut playbooks: Vec<Value> = Vec::new();
    for intent in intents.iter().rev().take(50) {
        let intent_payload = intent.get("payload").cloned().unwrap_or_else(|| json!({}));
        let corr = intent_payload
            .get("corr_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let matched_action = corr.as_deref().and_then(|cid| {
            actions
                .iter()
                .rev()
                .find_map(|act| match act.get("payload") {
                    Some(payload)
                        if payload.get("corr_id").and_then(|v| v.as_str()) == Some(cid) =>
                    {
                        Some(payload.clone())
                    }
                    _ => None,
                })
        });
        playbooks.push(json!({
            "intent": intent_payload,
            "action": matched_action,
        }));
    }

    let playbooks_count = playbooks.len();
    let beliefs_count = beliefs_items.len();

    let state_dir = util::state_dir();
    let playbooks_path = state_dir.join("distilled.playbooks.json");
    let beliefs_path = state_dir.join("distilled.beliefs.json");
    if let Err(err) = write_json_pretty(
        &playbooks_path,
        &json!({ "version": playbooks_version, "items": playbooks.clone() }),
    )
    .await
    {
        warn!("distill failed to persist playbooks: {}", err);
    }
    if let Err(err) = write_json_pretty(
        &beliefs_path,
        &json!({ "version": beliefs_version, "items": beliefs_items.clone() }),
    )
    .await
    {
        warn!("distill failed to persist beliefs: {}", err);
    }

    let removed_versions = prune_world_versions(5).await;

    let mut payload = json!({
        "playbooks": playbooks_count,
        "beliefs": beliefs_count,
        "removed_world_versions": removed_versions,
    });
    responses::attach_corr(&mut payload);
    state
        .bus()
        .publish(topics::TOPIC_DISTILL_COMPLETED, &payload);

    json!({
        "ok": true,
        "playbooks": playbooks_count,
        "beliefs": beliefs_count,
        "beliefs_version": beliefs_version,
        "intents_version": intents_version,
        "actions_version": actions_version,
        "files": {
            "playbooks": playbooks_path.to_string_lossy(),
            "beliefs": beliefs_path.to_string_lossy(),
        },
        "removed_world_versions": removed_versions,
    })
}

/// Spawn the periodic distillation loop. Defaults to 24h cadence and can be tuned via
/// `ARW_DISTILL_EVERY_HOURS`.
pub(crate) fn start(state: AppState) -> TaskHandle {
    TaskHandle::new(
        "distill.loop",
        tokio::spawn(async move {
            let hours: u64 = std::env::var("ARW_DISTILL_EVERY_HOURS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24);
            if hours == 0 {
                return;
            }
            let period = std::time::Duration::from_secs(hours.saturating_mul(3600));
            let mut ticker = tokio::time::interval(period);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Run once immediately so operators have fresh artifacts without waiting a full interval
            log_if_unexpected(run_once(&state).await);
            // Consume the initial instant tick so the next awaits the full period
            ticker.tick().await;
            loop {
                ticker.tick().await;
                log_if_unexpected(run_once(&state).await);
            }
        }),
    )
}

fn log_if_unexpected(result: Value) {
    if result.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        warn!(target: "distill", ?result, "distill run returned non-ok status");
    }
}

async fn write_json_pretty(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        afs::create_dir_all(parent).await?;
    }
    let body = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    afs::write(path, body).await
}

async fn prune_world_versions(keep: usize) -> usize {
    if keep == 0 {
        return 0;
    }
    let dir = util::state_dir().join("world").join("versions");
    let mut removed = 0usize;
    match afs::read_dir(&dir).await {
        Ok(mut rd) => {
            let mut entries: Vec<(std::path::PathBuf, u64)> = Vec::new();
            while let Ok(Some(entry)) = rd.next_entry().await {
                if let Ok(md) = entry.metadata().await {
                    if md.is_file() {
                        let age = md
                            .modified()
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(u64::MAX);
                        entries.push((entry.path(), age));
                    }
                }
            }
            entries.sort_by_key(|(_, age)| *age);
            if entries.len() > keep {
                let to_remove = entries.len() - keep;
                for (path, _) in entries.iter().take(to_remove) {
                    let path = path.clone();
                    match afs::remove_file(&path).await {
                        Ok(_) => removed += 1,
                        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                            warn!("distill failed to remove world version {:?}: {}", path, err);
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("distill failed to scan world versions: {}", err);
            }
        }
    }
    removed
}
