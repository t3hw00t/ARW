use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{config, tasks::TaskHandle, AppState};
use arw_topics as topics;
use chrono::Utc;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const HISTORY_LIMIT: usize = 64;

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let mut tasks = Vec::new();
    if let Some(path) = config::runtime_config_path() {
        tasks.push(watch_runtime_config(state.clone(), path));
    }
    if let Some(path) = config::gating_config_path() {
        tasks.push(watch_gating_config(state.clone(), path));
    }
    if let Some(path) = config::cache_policy_manifest_path() {
        tasks.push(watch_cache_policy(state, path));
    }
    tasks
}

fn watch_runtime_config(state: AppState, path: PathBuf) -> TaskHandle {
    TaskHandle::new(
        "config.watch.runtime",
        tokio::spawn(async move {
            let mut ticker = interval(POLL_INTERVAL);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut last_hash: Option<String> = None;
            let mut last_status: Option<&'static str> = None;
            loop {
                ticker.tick().await;
                match tokio::fs::read(&path).await {
                    Ok(bytes) => {
                        if last_status != Some("ok") {
                            debug!(path = %path.display(), "runtime config reachable");
                        }
                        last_status = Some("ok");
                        let digest = hash_bytes(&bytes);
                        if last_hash.as_ref() == Some(&digest) {
                            continue;
                        }
                        match reload_runtime_config(&state, &path, &digest).await {
                            Ok(_) => {
                                last_hash = Some(digest);
                            }
                            Err(err) => {
                                warn!(path = %path.display(), %err, "runtime config reload failed");
                            }
                        }
                    }
                    Err(err) => {
                        if last_status != Some("missing") {
                            warn!(path = %path.display(), error = %err, "runtime config missing or unreadable");
                        }
                        last_status = Some("missing");
                        last_hash = None;
                    }
                }
            }
        }),
    )
}

fn watch_gating_config(state: AppState, path: PathBuf) -> TaskHandle {
    TaskHandle::new(
        "config.watch.gating",
        tokio::spawn(async move {
            let mut ticker = interval(POLL_INTERVAL);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut last_modified: Option<SystemTime> = None;
            loop {
                ticker.tick().await;
                match tokio::fs::metadata(&path).await {
                    Ok(md) => {
                        let modified = md.modified().ok();
                        if modified == last_modified {
                            continue;
                        }
                        last_modified = modified;
                        if let Err(err) = reload_gating().await {
                            warn!(path = %path.display(), %err, "gating policy reload failed");
                            continue;
                        }
                        state.bus().publish(
                            topics::TOPIC_GATING_RELOADED,
                            &json!({
                                "path": path.display().to_string(),
                                "ts_ms": Utc::now().timestamp_millis(),
                            }),
                        );
                        info!(path = %path.display(), "gating policy reloaded");
                    }
                    Err(err) => {
                        if last_modified.is_some() {
                            warn!(path = %path.display(), error = %err, "gating policy metadata unavailable");
                        }
                        last_modified = None;
                    }
                }
            }
        }),
    )
}

fn watch_cache_policy(state: AppState, path: PathBuf) -> TaskHandle {
    TaskHandle::new(
        "config.watch.cache_policy",
        tokio::spawn(async move {
            let mut ticker = interval(POLL_INTERVAL);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            let mut last_modified: Option<SystemTime> = None;
            loop {
                ticker.tick().await;
                match tokio::fs::metadata(&path).await {
                    Ok(md) => {
                        let modified = md.modified().ok();
                        if modified == last_modified {
                            continue;
                        }
                        last_modified = modified;
                        if let Err(err) = reload_cache_policy().await {
                            warn!(path = %path.display(), %err, "cache policy reload failed");
                            continue;
                        }
                        state.bus().publish(
                            topics::TOPIC_CACHE_POLICY_RELOADED,
                            &json!({
                                "path": path.display().to_string(),
                                "ts_ms": Utc::now().timestamp_millis(),
                            }),
                        );
                        info!(path = %path.display(), "cache policy manifest applied");
                    }
                    Err(err) => {
                        if last_modified.is_some() {
                            warn!(path = %path.display(), error = %err, "cache policy metadata unavailable");
                        }
                        last_modified = None;
                    }
                }
            }
        }),
    )
}

async fn reload_runtime_config(state: &AppState, path: &Path, digest: &str) -> Result<(), String> {
    let path_string = path.to_string_lossy().to_string();
    let path_clone = path_string.clone();
    let cfg = tokio::task::spawn_blocking(move || arw_core::load_config(&path_clone))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&cfg).map_err(|e| e.to_string())?;

    let config_state = state.config_state();
    {
        let mut guard = config_state.lock().await;
        *guard = value.clone();
    }
    let config_history = state.config_history();
    {
        let mut history = config_history.lock().await;
        history.push((format!("watch:{}", path_string), value.clone()));
        if history.len() > HISTORY_LIMIT {
            let excess = history.len() - HISTORY_LIMIT;
            history.drain(0..excess);
        }
    }

    state.bus().publish(
        topics::TOPIC_CONFIG_RELOADED,
        &json!({
            "path": path_string,
            "hash": digest,
            "ts_ms": Utc::now().timestamp_millis(),
        }),
    );
    tokio::task::spawn_blocking(config::apply_effective_paths)
        .await
        .map_err(|e| e.to_string())?;
    info!(path = %path.display(), hash = %digest, "runtime config reloaded");
    Ok(())
}

async fn reload_gating() -> Result<(), String> {
    tokio::task::spawn_blocking(config::init_gating_from_configs)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn reload_cache_policy() -> Result<(), String> {
    tokio::task::spawn_blocking(config::init_cache_policy_from_manifest)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{:x}", digest)
}
