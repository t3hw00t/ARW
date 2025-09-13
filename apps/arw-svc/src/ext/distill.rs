use crate::AppState;
use serde_json::{json, Value};

// Nightly distillation: compact logs into beliefs/playbooks/summary and clean indexes

pub async fn distill_once(state: &AppState) -> Value {
    // Summarize recent intents/actions into a compact playbook
    let intents = super::state_api::intents_snapshot().await;
    let actions = super::state_api::actions_snapshot().await;
    let mut playbooks: Vec<Value> = Vec::new();
    for it in intents.iter().rev().take(50) {
        let act = actions.iter().rev().find(|a| {
            a.get("payload").and_then(|p| p.get("corr_id"))
                == it.get("payload").and_then(|p| p.get("corr_id"))
        });
        playbooks.push(json!({
            "intent": it.get("payload"),
            "action": act.and_then(|a| a.get("payload")).cloned(),
        }));
    }
    // Persist to state
    let dir = crate::ext::paths::state_dir();
    let _ = super::io::save_json_file_async(
        &dir.join("distilled.playbooks.json"),
        &json!({"items": playbooks}),
    )
    .await;

    // Beliefs snapshot is materialized via events; leave as future work to copy here if needed.

    // Short world index hygiene: prune old world versions keeping last 5
    prune_world_versions(5).await;

    // Emit a small event
    let mut payload = json!({"playbooks":  playbooks.len()});
    super::corr::ensure_corr(&mut payload);
    state.bus.publish("Distill.Completed", &payload);
    json!({"ok": true, "playbooks": playbooks.len()})
}

async fn prune_world_versions(keep: usize) {
    use tokio::fs as afs;
    let dir = super::paths::world_versions_dir();
    if let Ok(mut rd) = afs::read_dir(&dir).await {
        let mut items: Vec<(std::path::PathBuf, u64)> = Vec::new();
        while let Ok(Some(ent)) = rd.next_entry().await {
            let p = ent.path();
            if let Ok(md) = ent.metadata().await {
                if md.is_file() {
                    let ts = md
                        .modified()
                        .ok()
                        .and_then(|m| m.elapsed().ok())
                        .map(|e| e.as_secs())
                        .unwrap_or(0);
                    items.push((p, ts));
                }
            }
        }
        items.sort_by_key(|x| x.1);
        if items.len() > keep {
            let to_remove = items.len() - keep;
            // Remove the first `to_remove` files (preserves existing semantics)
            let paths: Vec<std::path::PathBuf> =
                items.iter().take(to_remove).map(|x| x.0.clone()).collect();
            for p in paths {
                let _ = afs::remove_file(&p).await;
            }
        }
    }
}

pub fn start_nightly(state: AppState) {
    tokio::spawn(async move {
        let interval_hours: u64 = std::env::var("ARW_DISTILL_EVERY_HOURS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(24);
        let mut intv = tokio::time::interval(std::time::Duration::from_secs(interval_hours * 3600));
        loop {
            intv.tick().await;
            let _ = distill_once(&state).await;
        }
    });
}
