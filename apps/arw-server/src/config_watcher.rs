use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::util;
use crate::{config, tasks::TaskHandle, AppState};
use arw_topics as topics;
use chrono::Utc;
use once_cell::sync::Lazy;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tokio::time::{interval, Duration, MissedTickBehavior};
use tracing::{debug, info, warn};

const POLL_INTERVAL: Duration = Duration::from_secs(3);
const HISTORY_LIMIT: usize = 64;

#[derive(Default, Clone)]
struct ReloadState {
    last_ok_ms: Option<i64>,
    last_reason: Option<String>,
    last_error_ms: Option<i64>,
    last_error: Option<String>,
}

static MANIFEST_RELOAD: Lazy<Mutex<ReloadState>> = Lazy::new(|| Mutex::new(ReloadState::default()));
static BUNDLES_RELOAD: Lazy<Mutex<ReloadState>> = Lazy::new(|| Mutex::new(ReloadState::default()));

fn cooldown_ms() -> u64 {
    static COOLDOWN: Lazy<u64> = Lazy::new(|| {
        std::env::var("ARW_RUNTIME_WATCHER_COOLDOWN_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(3 * 60 * 1000)
    });
    *COOLDOWN
}

fn humanize_ms(ms: u64) -> String {
    let mut rem = ms;
    let days = rem / 86_400_000;
    rem %= 86_400_000;
    let hrs = rem / 3_600_000;
    rem %= 3_600_000;
    let mins = rem / 60_000;
    rem %= 60_000;
    let secs = rem / 1_000;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hrs > 0 {
        parts.push(format!("{}h", hrs));
    }
    if mins > 0 {
        parts.push(format!("{}m", mins));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{}s", secs));
    }
    parts.join(" ")
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let mut tasks = Vec::new();
    if let Some(path) = config::runtime_config_path() {
        tasks.push(watch_runtime_config(state.clone(), path));
    }
    if let Some(path) = config::gating_config_path() {
        tasks.push(watch_gating_config(state.clone(), path));
    }
    if let Some(path) = config::cache_policy_manifest_path() {
        tasks.push(watch_cache_policy(state.clone(), path));
    }
    // Managed runtime quick wins: auto-reload supervisor manifests and bundle catalogs.
    tasks.push(watch_runtime_manifests(state.clone()));
    tasks.push(watch_runtime_bundles(state));
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

    crate::config::apply_env_overrides_from(&value);

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

pub(crate) fn manifest_paths() -> Vec<PathBuf> {
    if let Ok(raw) = std::env::var("ARW_RUNTIME_MANIFEST") {
        let paths = raw
            .split(';')
            .map(|part| part.trim())
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            return paths;
        }
    }
    if let Some(path) = arw_core::resolve_config_path("configs/runtime/runtimes.toml") {
        return vec![path];
    }
    Vec::new()
}

pub(crate) fn bundle_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(raw) = std::env::var("ARW_RUNTIME_BUNDLE_DIR") {
        for part in raw.split(';') {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                roots.push(PathBuf::from(trimmed));
            }
        }
    }
    if roots.is_empty() {
        if let Some(config_root) = arw_core::resolve_config_path("configs/runtime") {
            roots.push(config_root);
        }
    }
    let state_root = util::state_dir().join("runtime").join("bundles");
    if !roots.iter().any(|p| p == &state_root) {
        roots.push(state_root);
    }
    roots
}

fn hash_file(path: &Path) -> Option<String> {
    match std::fs::read(path) {
        Ok(bytes) => Some(hash_bytes(&bytes)),
        Err(_) => None,
    }
}

fn hash_dir_snapshot(dir: &Path) -> Option<String> {
    let mut hasher = Sha256::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return None,
    };
    let mut files: Vec<(String, u64, u128)> = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        // Only consider JSON catalogs/manifests under runtime roots.
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(md) = path.metadata() {
                let len = md.len();
                let modified = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                files.push((path.display().to_string(), len, modified));
            }
        }
    }
    if files.is_empty() {
        return Some("empty".into());
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (p, len, ts) in files {
        hasher.update(p.as_bytes());
        hasher.update(len.to_le_bytes());
        hasher.update(ts.to_le_bytes());
    }
    Some(format!("{:x}", hasher.finalize()))
}

fn paths_fingerprint(paths: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    let mut parts = paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>();
    parts.sort();
    for p in parts {
        hasher.update(p.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn read_manifest_hashes(paths: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    let mut parts = paths.to_vec();
    parts.sort();
    for p in parts {
        hasher.update(p.display().to_string().as_bytes());
        if let Some(h) = hash_file(&p) {
            hasher.update(h.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn compute_bundle_roots_fingerprint(roots: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    let mut rs = roots.to_vec();
    rs.sort();
    for r in rs {
        hasher.update(r.display().to_string().as_bytes());
        if let Some(h) = hash_dir_snapshot(&r) {
            hasher.update(h.as_bytes());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn watcher_tick_interval() -> Duration {
    // Reuse the same default as other config watchers.
    POLL_INTERVAL
}

fn manifest_watch_reason(
    prev_paths_fp: &str,
    cur_paths_fp: &str,
    prev_hash: &str,
    cur_hash: &str,
) -> Option<&'static str> {
    if cur_paths_fp != prev_paths_fp {
        Some("paths_changed")
    } else if cur_hash != prev_hash {
        Some("content_changed")
    } else {
        None
    }
}

fn bundle_watch_reason(prev_fp: &str, cur_fp: &str) -> Option<&'static str> {
    if cur_fp != prev_fp {
        Some("dir_changed")
    } else {
        None
    }
}

fn log_reload_error(path: &str, what: &str, err: &str) {
    warn!(path, error = %err, "{} reload failed", what);
}

fn manifest_label(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        "<none>".into()
    } else {
        paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(";")
    }
}

fn roots_label(roots: &[PathBuf]) -> String {
    if roots.is_empty() {
        "<none>".into()
    } else {
        roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(";")
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn bus_emit_reload(state: &AppState, kind: &str, info: serde_json::Value) {
    state.bus().publish(
        arw_topics::TOPIC_CONFIG_RELOADED,
        &json!({
            "kind": kind,
            "info": info,
            "ts_ms": now_ms(),
        }),
    );
}

fn json_paths(paths: &[PathBuf]) -> serde_json::Value {
    serde_json::json!(paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>())
}

fn json_roots(roots: &[PathBuf]) -> serde_json::Value {
    serde_json::json!(roots
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>())
}

fn json_str<S: Into<String>>(s: S) -> serde_json::Value {
    serde_json::json!(s.into())
}

fn emit_manifest_reload(state: &AppState, paths: &[PathBuf], reason: &str) {
    bus_emit_reload(
        state,
        "runtime.manifests",
        json!({
            "paths": json_paths(paths),
            "reason": reason,
        }),
    );
}

fn emit_bundles_reload(state: &AppState, roots: &[PathBuf], reason: &str) {
    bus_emit_reload(
        state,
        "runtime.bundles",
        json!({
            "roots": json_roots(roots),
            "reason": reason,
        }),
    );
}

fn watcher_make_task(
    name: &'static str,
    fut: impl std::future::Future<Output = ()> + Send + 'static,
) -> TaskHandle {
    TaskHandle::new(name, tokio::spawn(fut))
}

fn manifest_paths_and_hash() -> (Vec<PathBuf>, String, String) {
    let paths = manifest_paths();
    let paths_fp = paths_fingerprint(&paths);
    let content_hash = read_manifest_hashes(&paths);
    (paths, paths_fp, content_hash)
}

fn bundle_roots_and_fingerprint() -> (Vec<PathBuf>, String) {
    let roots = bundle_roots();
    let fp = compute_bundle_roots_fingerprint(&roots);
    (roots, fp)
}

fn default_watch_tick() -> Duration {
    watcher_tick_interval()
}

fn set_status(status: &mut Option<&'static str>, value: &'static str) {
    *status = Some(value)
}

fn emit_info(state: &AppState, kind: &str, message: &str, extra: serde_json::Value) {
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_HEALTH,
        &json!({
            "status": "info",
            "component": kind,
            "message": message,
            "extra": extra,
            "ts_ms": now_ms(),
        }),
    );
}

fn emit_warn(state: &AppState, kind: &str, message: &str, extra: serde_json::Value) {
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_HEALTH,
        &json!({
            "status": "warn",
            "component": kind,
            "message": message,
            "extra": extra,
            "ts_ms": now_ms(),
        }),
    );
}

fn emit_error(state: &AppState, kind: &str, message: &str, extra: serde_json::Value) {
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_HEALTH,
        &json!({
            "status": "error",
            "component": kind,
            "message": message,
            "extra": extra,
            "ts_ms": now_ms(),
        }),
    );
}

fn manifest_health_label() -> &'static str {
    "runtime manifests"
}

fn bundles_health_label() -> &'static str {
    "runtime bundles"
}

fn manifest_component() -> String {
    "runtime.manifests".to_string()
}

fn bundles_component() -> String {
    "runtime.bundles".to_string()
}

fn format_paths(paths: &[PathBuf]) -> String {
    manifest_label(paths)
}

fn format_roots(roots: &[PathBuf]) -> String {
    roots_label(roots)
}

fn restart_note() -> &'static str {
    "reload triggered"
}

fn service_ok() -> &'static str {
    "ok"
}

fn service_missing() -> &'static str {
    "missing"
}

fn service_changed() -> &'static str {
    "changed"
}

fn build_extra(label: &str, val: &str, reason: &str) -> serde_json::Value {
    json!({ label: val, "reason": reason })
}

fn should_emit(prev_status: Option<&'static str>, new_status: &'static str) -> bool {
    prev_status != Some(new_status)
}

fn debug_watch_start(label: &str, value: &str) {
    debug!(target: "arw::runtime", %value, "watch started: {}", label);
}

fn debug_watch_tick(label: &str) {
    debug!(target: "arw::runtime", label = %label, "watch tick");
}

fn any_paths_exist(paths: &[PathBuf]) -> bool {
    paths.iter().any(|p| p.exists())
}

fn any_roots_exist(roots: &[PathBuf]) -> bool {
    roots.iter().any(|p| p.exists())
}

pub(crate) fn watch_runtime_manifests(state: AppState) -> TaskHandle {
    watcher_make_task("config.watch.runtime_manifests", async move {
        let mut ticker = tokio::time::interval(default_watch_tick());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let (paths, mut paths_fp, mut content_hash) = manifest_paths_and_hash();
        let mut last_status: Option<&'static str> = None;
        debug_watch_start(manifest_health_label(), &format_paths(&paths));
        loop {
            ticker.tick().await;
            debug_watch_tick("runtime.manifests");
            let (cur_paths, cur_paths_fp, cur_hash) = manifest_paths_and_hash();
            let reason = manifest_watch_reason(&paths_fp, &cur_paths_fp, &content_hash, &cur_hash);
            if let Some(r) = reason {
                let label = format_paths(&cur_paths);
                let info_extra = build_extra("paths", &label, r);
                emit_info(&state, &manifest_component(), restart_note(), info_extra);
                match state.runtime_supervisor().load_manifests_from_disk().await {
                    Ok(_) => {
                        set_status(&mut last_status, service_changed());
                        emit_manifest_reload(&state, &cur_paths, r);
                        record_manifest_ok(r);
                    }
                    Err(err) => {
                        emit_error(
                            &state,
                            &manifest_component(),
                            "reload error",
                            json_str(err.to_string()),
                        );
                        log_reload_error(
                            &manifest_label(&cur_paths),
                            manifest_health_label(),
                            &err.to_string(),
                        );
                        record_manifest_error(&err.to_string());
                    }
                }
                // update fingerprints for next tick
                paths_fp = cur_paths_fp;
                content_hash = cur_hash;
            } else {
                // If previously missing and now reachable, emit a harmless info once.
                let exists_any = any_paths_exist(&cur_paths);
                let status = if exists_any {
                    service_ok()
                } else {
                    service_missing()
                };
                if should_emit(last_status, status) {
                    if exists_any {
                        emit_info(
                            &state,
                            &manifest_component(),
                            "manifests reachable",
                            json_paths(&cur_paths),
                        );
                    } else {
                        emit_warn(
                            &state,
                            &manifest_component(),
                            "manifests missing",
                            json_paths(&cur_paths),
                        );
                    }
                    set_status(&mut last_status, status);
                }
            }
        }
    })
}

pub(crate) fn watch_runtime_bundles(state: AppState) -> TaskHandle {
    watcher_make_task("config.watch.runtime_bundles", async move {
        let mut ticker = tokio::time::interval(default_watch_tick());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let (roots, mut fp) = bundle_roots_and_fingerprint();
        debug_watch_start(bundles_health_label(), &format_roots(&roots));
        let mut last_status: Option<&'static str> = None;
        loop {
            ticker.tick().await;
            debug_watch_tick("runtime.bundles");
            let (cur_roots, cur_fp) = bundle_roots_and_fingerprint();
            if let Some(reason) = bundle_watch_reason(&fp, &cur_fp) {
                let label = format_roots(&cur_roots);
                emit_info(
                    &state,
                    &bundles_component(),
                    restart_note(),
                    build_extra("roots", &label, reason),
                );
                match state.runtime_bundles().reload().await {
                    Ok(_) => {
                        set_status(&mut last_status, service_changed());
                        emit_bundles_reload(&state, &cur_roots, reason);
                        record_bundles_ok(reason);
                    }
                    Err(err) => {
                        emit_error(
                            &state,
                            &bundles_component(),
                            "reload error",
                            json_str(err.to_string()),
                        );
                        log_reload_error(&label, bundles_health_label(), &err.to_string());
                        record_bundles_error(&err.to_string());
                    }
                }
                // update fingerprint for next tick
                fp = cur_fp;
            } else {
                let exists_any = any_roots_exist(&cur_roots);
                let status = if exists_any {
                    service_ok()
                } else {
                    service_missing()
                };
                if should_emit(last_status, status) {
                    if exists_any {
                        emit_info(
                            &state,
                            &bundles_component(),
                            "bundle roots reachable",
                            json_roots(&cur_roots),
                        );
                    } else {
                        emit_warn(
                            &state,
                            &bundles_component(),
                            "bundle roots missing",
                            json_roots(&cur_roots),
                        );
                    }
                    set_status(&mut last_status, status);
                }
            }
        }
    })
}

fn record_manifest_ok(reason: &str) {
    let mut st = MANIFEST_RELOAD.lock().unwrap();
    st.last_ok_ms = Some(now_ms());
    st.last_reason = Some(reason.to_string());
}

fn record_manifest_error(err: &str) {
    let mut st = MANIFEST_RELOAD.lock().unwrap();
    st.last_error_ms = Some(now_ms());
    st.last_error = Some(err.to_string());
}

fn record_bundles_ok(reason: &str) {
    let mut st = BUNDLES_RELOAD.lock().unwrap();
    st.last_ok_ms = Some(now_ms());
    st.last_reason = Some(reason.to_string());
}

fn record_bundles_error(err: &str) {
    let mut st = BUNDLES_RELOAD.lock().unwrap();
    st.last_error_ms = Some(now_ms());
    st.last_error = Some(err.to_string());
}

pub(crate) fn watcher_snapshot() -> serde_json::Value {
    let manifest_paths = manifest_paths()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>();
    let bundle_roots = bundle_roots()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>();
    let m = MANIFEST_RELOAD.lock().unwrap().clone();
    let b = BUNDLES_RELOAD.lock().unwrap().clone();
    let poll_interval_ms = (POLL_INTERVAL.as_millis() as u64).max(1);
    let now = now_ms();

    let (m_ok_age, m_err_age) = (
        m.last_ok_ms
            .and_then(|t| now.checked_sub(t))
            .map(|d| d as u64),
        m.last_error_ms
            .and_then(|t| now.checked_sub(t))
            .map(|d| d as u64),
    );
    let (b_ok_age, b_err_age) = (
        b.last_ok_ms
            .and_then(|t| now.checked_sub(t))
            .map(|d| d as u64),
        b.last_error_ms
            .and_then(|t| now.checked_sub(t))
            .map(|d| d as u64),
    );

    let cd = cooldown_ms();
    let m_status = status_from(&m, now, cd);
    let b_status = status_from(&b, now, cd);
    let overall = if m_status == "ok" && b_status == "ok" {
        "ok"
    } else {
        "degraded"
    };
    json!({
        "manifests": {
            "paths": manifest_paths,
            "last_ok_ms": m.last_ok_ms,
            "last_ok_age_ms": m_ok_age,
            "last_ok_age_human": m_ok_age.map(humanize_ms),
            "last_reason": m.last_reason,
            "last_error_ms": m.last_error_ms,
            "last_error_age_ms": m_err_age,
            "last_error_age_human": m_err_age.map(humanize_ms),
            "last_error": m.last_error,
            "status": m_status,
        },
        "bundles": {
            "roots": bundle_roots,
            "last_ok_ms": b.last_ok_ms,
            "last_ok_age_ms": b_ok_age,
            "last_ok_age_human": b_ok_age.map(humanize_ms),
            "last_reason": b.last_reason,
            "last_error_ms": b.last_error_ms,
            "last_error_age_ms": b_err_age,
            "last_error_age_human": b_err_age.map(humanize_ms),
            "last_error": b.last_error,
            "status": b_status,
        },
        "poll_interval_ms": poll_interval_ms,
        "cooldown_ms": cd,
        "status": overall,
    })
}

fn status_from(st: &ReloadState, now_ms: i64, cooldown_ms: u64) -> &'static str {
    match (st.last_ok_ms, st.last_error_ms) {
        (Some(ok), Some(err)) => {
            if err > ok {
                if (now_ms - err) as u64 <= cooldown_ms {
                    "degraded"
                } else {
                    "ok"
                }
            } else {
                "ok"
            }
        }
        (None, Some(err)) => {
            if (now_ms - err) as u64 <= cooldown_ms {
                "degraded"
            } else {
                "ok"
            }
        }
        _ => "ok",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use tempfile::tempdir;

    #[test]
    fn manifest_paths_prefer_env_list() {
        let mut env = test_support::env::guard();
        env.set("ARW_RUNTIME_MANIFEST", "a.toml; b.toml ; ; ");
        let paths = manifest_paths();
        assert!(paths.len() >= 2, "expected at least two manifest paths");
        assert_eq!(paths[0].to_string_lossy(), "a.toml");
        assert_eq!(paths[1].to_string_lossy(), "b.toml");
    }

    #[test]
    fn bundle_roots_include_state_root() {
        let td = tempdir().expect("tempdir");
        let mut env = test_support::env::guard();
        // Ensure state dir is scoped for this test
        let _state = crate::util::scoped_state_dir_for_tests(td.path(), &mut env);
        env.set("ARW_RUNTIME_BUNDLE_DIR", "X:/custom/root1; Y:/custom/root2");
        let roots = bundle_roots();
        assert!(
            roots.iter().any(|p| p.ends_with("runtime/bundles")),
            "expected state runtime/bundles to be present"
        );
        // Env-provided roots should be present as-is (string match by suffix)
        let has_root1 = roots
            .iter()
            .any(|p| p.to_string_lossy().contains("custom/root1"));
        let has_root2 = roots
            .iter()
            .any(|p| p.to_string_lossy().contains("custom/root2"));
        assert!(
            has_root1 && has_root2,
            "expected both env roots to be present"
        );
    }

    #[test]
    fn hash_dir_snapshot_changes_on_file_update() {
        let td = tempdir().expect("tempdir");
        let dir = td.path();
        // No files yet
        let h0 = hash_dir_snapshot(dir).unwrap_or_default();
        // Add a JSON file
        std::fs::write(dir.join("a.json"), b"{}\n").expect("write a.json");
        let h1 = hash_dir_snapshot(dir).unwrap_or_default();
        assert_ne!(h0, h1, "hash should change after adding a file");
        // Modify contents
        std::fs::write(dir.join("a.json"), b"{\n  \"x\": 1\n}\n").expect("rewrite a.json");
        let h2 = hash_dir_snapshot(dir).unwrap_or_default();
        assert_ne!(h1, h2, "hash should change after modifying file contents");
        // Add another file
        std::fs::write(dir.join("b.json"), b"{}\n").expect("write b.json");
        let h3 = hash_dir_snapshot(dir).unwrap_or_default();
        assert_ne!(h2, h3, "hash should change after adding another file");
    }
}
