use super::{mem_limit, stats};
use crate::AppState;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    OnceLock,
};
use tokio::fs as afs;
use tokio::sync::RwLock;

// Lightweight snapshot of current suggestions (reused by API or UI)
static SNAPSHOT: OnceLock<RwLock<Vec<Value>>> = OnceLock::new();
static VERSION: OnceLock<AtomicU64> = OnceLock::new();
fn snap() -> &'static RwLock<Vec<Value>> {
    SNAPSHOT.get_or_init(|| RwLock::new(Vec::new()))
}
fn ver() -> &'static AtomicU64 {
    VERSION.get_or_init(|| AtomicU64::new(0))
}

pub fn start_feedback_engine(state: AppState) {
    // Spawn a single actor with short cadence; no blocking on request paths
    tokio::spawn(async move {
        // Load persisted snapshot (if any) before starting ticks
        if let Some(list) = load_snapshot().await {
            {
                let mut s = snap().write().await;
                *s = list.clone();
                ver().store(1, Ordering::Relaxed);
            }
            state.bus.publish(
                crate::ext::topics::TOPIC_FEEDBACK_SUGGESTED,
                &json!({"version": 1, "suggestions": list}),
            );
        }
        let tick_ms: u64 = load_cfg_tick_ms().unwrap_or_else(|| {
            std::env::var("ARW_FEEDBACK_TICK_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500)
        });
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // Gather minimal features from stats module (bounded, cheap)
            let routes_map = stats::routes_for_analysis().await;
            let mut f = arw_heuristics::Features::default();
            for (k, (ewma, hits, errs)) in routes_map.into_iter() {
                f.routes.insert(
                    k,
                    arw_heuristics::RouteStat {
                        ewma_ms: ewma,
                        hits,
                        errors: errs,
                    },
                );
            }
            f.mem_applied_count = stats::event_kind_count("memory.applied").await;
            f.cur_mem_limit = Some({ *mem_limit().read().await } as u64);
            let out: Vec<Value> = arw_heuristics::evaluate(&f);

            // Publish deltas if changed
            {
                let mut s = snap().write().await;
                if *s != out {
                    *s = out.clone();
                    let v = ver().fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = persist_snapshot(v, &out).await;
                    state.bus.publish(
                        crate::ext::topics::TOPIC_FEEDBACK_SUGGESTED,
                        &json!({"version": v, "suggestions": out}),
                    );
                    // Also emit a beliefs update for read-model consumers (dot.case)
                    state.bus.publish(
                        crate::ext::topics::TOPIC_BELIEFS_UPDATED,
                        &json!({"version": v, "suggestions": out}),
                    );
                    // Emit Intents for each suggestion (proposed)
                    for s in out.iter() {
                        // Expect shape: {id, action, params, rationale, confidence}
                        let mut intent = json!({
                            "status": "proposed",
                            "suggestion": s,
                        });
                        crate::ext::corr::ensure_corr(&mut intent);
                        state
                            .bus
                            .publish(crate::ext::topics::TOPIC_INTENTS_PROPOSED, &intent);
                    }
                }
            }
        }
    });
}

pub async fn snapshot() -> (u64, Vec<Value>) {
    let v = ver().load(Ordering::Relaxed);
    let s = snap().read().await.clone();
    (v, s)
}

pub async fn updates_since(since: u64) -> Option<(u64, Vec<Value>)> {
    let cur = ver().load(Ordering::Relaxed);
    if cur > since {
        Some((cur, snap().read().await.clone()))
    } else {
        None
    }
}

// --- Optional config loader (configs/feedback.toml) ---
#[derive(Deserialize, Default)]
struct FbCfg {
    tick_ms: Option<u64>,
}
fn load_cfg_tick_ms() -> Option<u64> {
    static CFG: OnceLock<Option<FbCfg>> = OnceLock::new();
    let cfg = CFG.get_or_init(|| {
        if let Some(p) = arw_core::resolve_config_path("configs/feedback.toml") {
            if let Ok(s) = std::fs::read_to_string(p) {
                return toml::from_str::<FbCfg>(&s).ok();
            }
        }
        None
    });
    cfg.as_ref().and_then(|c| c.tick_ms)
}

// ---- Persistence helpers (engine snapshot) ----
async fn snapshot_paths() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let dir = crate::ext::paths::state_dir();
    (
        dir.join("feedback_engine.json"),
        dir.join("feedback_engine.json.bak"),
        dir,
    )
}

fn versioned_path(dir: &std::path::Path, v: u64) -> std::path::PathBuf {
    dir.join(format!("feedback_engine.v{}.json", v))
}

async fn persist_snapshot(version: u64, list: &Vec<Value>) -> std::io::Result<()> {
    let (p, bak, dir) = snapshot_paths().await;
    // rotate current to .bak
    if tokio::fs::try_exists(&p).await.unwrap_or(false) {
        let _ = afs::rename(&p, &bak).await;
    }
    let body = json!({"version": version, "suggestions": list});
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    super::io::save_json_file_async(&p, &body).await?;
    // also write versioned file and prune older ones (keep 3)
    let vp = versioned_path(&dir, version);
    let _ = super::io::save_json_file_async(&vp, &body).await;
    prune_versions(&dir, 3).await;
    Ok(())
}

async fn load_snapshot() -> Option<Vec<Value>> {
    let (p, _bak, _dir) = snapshot_paths().await;
    if let Ok(bytes) = afs::read(&p).await {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            return v.get("suggestions").and_then(|x| x.as_array()).cloned();
        }
    }
    None
}

pub async fn rollback_to(target_ver: Option<u64>) -> Option<(u64, Vec<Value>)> {
    let (p, bak, dir) = snapshot_paths().await;
    let target = if let Some(v) = target_ver {
        versioned_path(&dir, v)
    } else {
        bak
    };
    if !tokio::fs::try_exists(&target).await.ok()? {
        return None;
    }
    // replace current with target content
    let bytes = afs::read(&target).await.ok()?;
    let val: Value = serde_json::from_slice(&bytes).ok()?;
    let list = val
        .get("suggestions")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let _ = super::io::save_bytes_atomic(&p, &bytes).await;
    {
        {
            let mut s = snap().write().await;
            *s = list.clone();
        }
        let vnow = ver().fetch_add(1, Ordering::Relaxed) + 1;
        Some((vnow, list))
    }
}

pub async fn list_versions() -> Vec<u64> {
    let (_p, _bak, dir) = snapshot_paths().await;
    let mut out: Vec<u64> = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                if let Some(rest) = name.strip_prefix("feedback_engine.v") {
                    if let Some(num) = rest.strip_suffix(".json") {
                        if let Ok(v) = num.parse::<u64>() {
                            out.push(v);
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.reverse();
    out
}

async fn prune_versions(dir: &std::path::Path, keep: usize) {
    let mut vers = list_versions().await;
    if vers.len() <= keep {
        return;
    }
    vers.drain(..keep); // remaining are old
    for v in vers {
        let _ = afs::remove_file(versioned_path(dir, v)).await;
    }
}
