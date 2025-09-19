use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use arw_events::{Bus, Envelope};
use arw_topics as topics;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs as afs;
use tokio::sync::RwLock;
use utoipa::ToSchema;

use crate::{governor::GovernorState, metrics::Metrics, responses, util};

#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct FeedbackSignal {
    pub id: String,
    pub ts: String,
    pub kind: String,
    pub target: String,
    pub confidence: f64,
    pub severity: u8,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct Suggestion {
    pub id: String,
    pub action: String,
    pub params: Value,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct FeedbackState {
    #[serde(default)]
    pub auto_apply: bool,
    #[serde(default)]
    pub signals: Vec<FeedbackSignal>,
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
}

#[derive(Debug)]
pub enum FeedbackError {
    NotFound,
    PolicyDenied(String),
    Invalid(String),
}

pub struct FeedbackHub {
    state: RwLock<FeedbackState>,
    applied: RwLock<HashSet<String>>,
    engine_snapshot: RwLock<Vec<Value>>,
    engine_version: AtomicU64,
    engine_loaded: AtomicBool,
    signal_seq: AtomicU64,
    bus: Bus,
    metrics: Arc<Metrics>,
    governor: Arc<GovernorState>,
    state_path: PathBuf,
    engine_path: PathBuf,
    engine_backup_path: PathBuf,
    engine_versions_dir: PathBuf,
}

impl FeedbackHub {
    pub async fn new(bus: Bus, metrics: Arc<Metrics>, governor: Arc<GovernorState>) -> Arc<Self> {
        let state_dir = util::state_dir();
        let state_path = state_dir.join("feedback.json");
        let engine_path = state_dir.join("feedback_engine.json");
        let engine_backup_path = state_dir.join("feedback_engine.json.bak");
        let hub = Arc::new(Self {
            state: RwLock::new(load_state(&state_path).await),
            applied: RwLock::new(HashSet::new()),
            engine_snapshot: RwLock::new(Vec::new()),
            engine_version: AtomicU64::new(0),
            engine_loaded: AtomicBool::new(false),
            signal_seq: AtomicU64::new(1),
            bus,
            metrics,
            governor,
            state_path,
            engine_path,
            engine_backup_path,
            engine_versions_dir: state_dir,
        });
        hub.load_engine_snapshot().await;
        hub.spawn_engine_loop();
        hub.spawn_auto_apply_listener();
        hub
    }

    pub async fn snapshot(&self) -> FeedbackState {
        self.state.read().await.clone()
    }

    pub async fn submit_signal(
        &self,
        kind: String,
        target: String,
        confidence: f64,
        severity: u8,
        note: Option<String>,
    ) -> FeedbackState {
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let id = format!("sig-{}", self.signal_seq.fetch_add(1, Ordering::Relaxed));
        let sig = FeedbackSignal {
            id,
            ts,
            kind,
            target,
            confidence: confidence.clamp(0.0, 1.0),
            severity: severity.clamp(1, 5),
            note,
        };
        {
            let mut guard = self.state.write().await;
            guard.signals.push(sig);
            if guard.signals.len() > 200 {
                let overflow = guard.signals.len() - 200;
                guard.signals.drain(0..overflow);
            }
        }
        self.persist_state().await;
        let _ = self.refresh_suggestions().await;
        self.state.read().await.clone()
    }

    pub async fn analyze(&self) -> FeedbackState {
        let _ = self.refresh_suggestions().await;
        self.state.read().await.clone()
    }

    pub async fn set_auto_apply(&self, enabled: bool) -> FeedbackState {
        {
            let mut guard = self.state.write().await;
            guard.auto_apply = enabled;
        }
        self.persist_state().await;
        self.state.read().await.clone()
    }

    pub async fn reset(&self) -> FeedbackState {
        {
            let mut guard = self.state.write().await;
            guard.signals.clear();
            guard.suggestions.clear();
        }
        self.persist_state().await;
        self.state.read().await.clone()
    }

    pub async fn apply(&self, id: &str, source: &str) -> Result<(), FeedbackError> {
        let suggestion = self
            .lookup_suggestion(id)
            .await
            .ok_or(FeedbackError::NotFound)?;
        match policy::allow_apply(&suggestion.action, &suggestion.params).await {
            Ok(()) => {}
            Err(FeedbackError::PolicyDenied(reason)) => {
                let mut intent = json!({
                    "status": "rejected",
                    "reason": reason.clone(),
                    "suggestion": {
                        "id": suggestion.id.clone(),
                        "action": suggestion.action.clone(),
                        "params": suggestion.params.clone(),
                    }
                });
                responses::attach_corr(&mut intent);
                self.bus.publish(topics::TOPIC_INTENTS_REJECTED, &intent);
                return Err(FeedbackError::PolicyDenied(reason));
            }
            Err(other) => return Err(other),
        }
        if !self.apply_inner(&suggestion, source).await? {
            return Err(FeedbackError::Invalid("no-op".into()));
        }
        {
            let mut applied = self.applied.write().await;
            applied.insert(suggestion.id.clone());
        }
        Ok(())
    }

    pub async fn suggestions_snapshot(&self) -> (u64, Vec<Value>) {
        let version = self.engine_version.load(Ordering::Relaxed);
        let list = self.engine_snapshot.read().await.clone();
        (version, list)
    }

    pub async fn updates_since(&self, since: u64) -> Option<(u64, Vec<Value>)> {
        let cur = self.engine_version.load(Ordering::Relaxed);
        if cur > since {
            Some((cur, self.engine_snapshot.read().await.clone()))
        } else {
            None
        }
    }

    pub async fn list_versions(&self) -> Vec<u64> {
        list_engine_versions(&self.engine_versions_dir).await
    }

    pub async fn rollback(&self, target: Option<u64>) -> Option<(u64, Vec<Value>)> {
        let (bytes, version) = load_version_bytes(
            &self.engine_path,
            &self.engine_backup_path,
            &self.engine_versions_dir,
            target,
        )
        .await?;
        let payload: Value = serde_json::from_slice(&bytes).ok()?;
        let list = payload
            .get("suggestions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        save_bytes_atomic(&self.engine_path, &bytes).await.ok()?;
        if let Some(parent) = self.engine_backup_path.parent() {
            let _ = afs::create_dir_all(parent).await;
        }
        let _ = save_bytes_atomic(&self.engine_backup_path, &bytes).await;
        {
            let mut guard = self.engine_snapshot.write().await;
            *guard = list.clone();
        }
        self.engine_version.store(version, Ordering::Relaxed);
        self.update_state_suggestions(&list).await;
        self.persist_state().await;
        self.publish_suggestions(version, &list).await;
        Some((version, list))
    }

    pub fn effective_policy(&self) -> Value {
        policy::effective_policy()
    }

    fn spawn_engine_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_millis(load_tick_ms()));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let _ = this.refresh_suggestions().await;
            }
        });
    }

    fn spawn_auto_apply_listener(self: &Arc<Self>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut rx = this.bus.subscribe();
            while let Ok(env) = rx.recv().await {
                this.auto_apply_from_event(&env).await;
            }
        });
    }

    async fn refresh_suggestions(&self) -> Option<(u64, Vec<Value>)> {
        let features = self.collect_features().await;
        let new_list = arw_heuristics::evaluate(&features);
        let mut changed = false;
        {
            let current = self.engine_snapshot.read().await;
            if *current != new_list {
                changed = true;
            }
        }
        if !changed {
            self.update_state_suggestions(&new_list).await;
            return None;
        }
        {
            let mut guard = self.engine_snapshot.write().await;
            *guard = new_list.clone();
        }
        let version = self.engine_version.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = persist_engine(
            &self.engine_path,
            &self.engine_backup_path,
            &self.engine_versions_dir,
            version,
            &new_list,
        )
        .await;
        self.update_state_suggestions(&new_list).await;
        self.persist_state().await;
        self.publish_suggestions(version, &new_list).await;
        Some((version, new_list))
    }

    async fn update_state_suggestions(&self, list: &[Value]) {
        let suggestions = list
            .iter()
            .filter_map(json_to_suggestion)
            .collect::<Vec<Suggestion>>();
        let mut guard = self.state.write().await;
        guard.suggestions = suggestions;
    }

    async fn persist_state(&self) {
        let snapshot = self.state.read().await.clone();
        if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
            let _ = save_bytes_atomic(&self.state_path, &bytes).await;
        }
    }

    async fn collect_features(&self) -> arw_heuristics::Features {
        let mut features = arw_heuristics::Features::default();
        let routes = self.metrics.routes_for_analysis();
        for (path, (ewma, hits, errors)) in routes.into_iter() {
            features.routes.insert(
                path,
                arw_heuristics::RouteStat {
                    ewma_ms: ewma,
                    hits,
                    errors,
                },
            );
        }
        features.mem_applied_count = self.metrics.event_kind_count("memory.applied");
        features.cur_mem_limit = self.governor.memory_limit().await;
        features
    }

    async fn load_engine_snapshot(&self) {
        if let Ok(bytes) = afs::read(&self.engine_path).await {
            if let Ok(val) = serde_json::from_slice::<Value>(&bytes) {
                let version = val.get("version").and_then(|v| v.as_u64()).unwrap_or(1);
                let list = val
                    .get("suggestions")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                {
                    let mut guard = self.engine_snapshot.write().await;
                    *guard = list.clone();
                }
                self.engine_version.store(version, Ordering::Relaxed);
                self.update_state_suggestions(&list).await;
                self.persist_state().await;
                if !self.engine_loaded.swap(true, Ordering::Relaxed) {
                    self.publish_suggestions(version, &list).await;
                }
            }
        }
    }

    async fn publish_suggestions(&self, version: u64, list: &[Value]) {
        let mut payload = json!({"version": version, "suggestions": list});
        responses::attach_corr(&mut payload);
        self.bus.publish(topics::TOPIC_FEEDBACK_SUGGESTED, &payload);
        self.bus.publish(topics::TOPIC_BELIEFS_UPDATED, &payload);
        for item in list.iter() {
            let mut intent = json!({"status": "proposed", "suggestion": item});
            responses::attach_corr(&mut intent);
            self.bus.publish(topics::TOPIC_INTENTS_PROPOSED, &intent);
        }
    }

    async fn auto_apply_from_event(&self, env: &Envelope) {
        if env.kind != topics::TOPIC_FEEDBACK_SUGGESTED {
            return;
        }
        let auto = { self.state.read().await.auto_apply };
        if !auto {
            return;
        }
        let Some(list) = env.payload.get("suggestions").and_then(|v| v.as_array()) else {
            return;
        };
        for item in list.iter() {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                if self.applied.read().await.contains(id) {
                    continue;
                }
                if let Some(suggestion) = json_to_suggestion(item) {
                    if policy::allow_apply(&suggestion.action, &suggestion.params)
                        .await
                        .is_ok()
                    {
                        if let Ok(true) = self.apply_inner(&suggestion, "feedback.auto_apply").await
                        {
                            self.applied.write().await.insert(suggestion.id.clone());
                        }
                    }
                }
            }
        }
    }

    async fn apply_inner(
        &self,
        suggestion: &Suggestion,
        source: &str,
    ) -> Result<bool, FeedbackError> {
        let id = suggestion.id.clone();
        let action = suggestion.action.clone();
        let params = suggestion.params.clone();

        match action.as_str() {
            "hint" => {
                let secs = params
                    .get("http_timeout_secs")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| FeedbackError::Invalid("missing http_timeout_secs".into()))?;
                self.governor
                    .apply_hints(
                        &self.bus,
                        None,
                        None,
                        Some(secs),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await;
            }
            "profile" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| FeedbackError::Invalid("missing profile name".into()))?;
                self.governor.set_profile(&self.bus, name.to_string()).await;
            }
            "mem_limit" => {
                let limit = params
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| FeedbackError::Invalid("missing mem limit".into()))?;
                self.governor.set_memory_limit(Some(limit)).await;
            }
            _ => return Ok(false),
        }

        let mut applied_payload = json!({
            "id": id.clone(),
            "action": action.clone(),
            "params": params.clone(),
        });
        responses::attach_corr(&mut applied_payload);
        self.bus
            .publish(topics::TOPIC_FEEDBACK_APPLIED, &applied_payload);

        let mut actions_payload = json!({
            "ok": true,
            "source": source,
            "suggestion": {
                "id": id.clone(),
                "action": action.clone(),
                "params": params.clone(),
            }
        });
        responses::attach_corr(&mut actions_payload);
        self.bus
            .publish(topics::TOPIC_ACTIONS_APPLIED, &actions_payload);

        let mut intent_payload = json!({
            "status": "approved",
            "suggestion": {
                "id": id,
                "action": action,
                "params": params,
            }
        });
        responses::attach_corr(&mut intent_payload);
        self.bus
            .publish(topics::TOPIC_INTENTS_APPROVED, &intent_payload);
        Ok(true)
    }

    async fn lookup_suggestion(&self, id: &str) -> Option<Suggestion> {
        {
            let guard = self.state.read().await;
            if let Some(s) = guard.suggestions.iter().find(|s| s.id == id) {
                return Some(s.clone());
            }
        }
        let list = self.engine_snapshot.read().await.clone();
        for item in list.iter() {
            if let Some(s) = json_to_suggestion(item) {
                if s.id == id {
                    return Some(s);
                }
            }
        }
        None
    }
}

async fn load_state(path: &PathBuf) -> FeedbackState {
    if let Ok(bytes) = afs::read(path).await {
        if let Ok(state) = serde_json::from_slice::<FeedbackState>(&bytes) {
            return state;
        }
    }
    FeedbackState::default()
}

fn json_to_suggestion(value: &Value) -> Option<Suggestion> {
    let id = value.get("id").and_then(|v| v.as_str())?.to_string();
    let action = value.get("action").and_then(|v| v.as_str())?.to_string();
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
    let rationale = value
        .get("rationale")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let confidence = value
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    Some(Suggestion {
        id,
        action,
        params,
        rationale,
        confidence,
    })
}

pub(crate) async fn save_bytes_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        afs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    afs::write(&tmp, bytes).await?;
    match afs::rename(&tmp, path).await {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = afs::remove_file(path).await;
            let res = afs::rename(&tmp, path).await;
            if res.is_err() {
                let _ = afs::remove_file(&tmp).await;
            }
            res
        }
    }
}

async fn persist_engine(
    path: &PathBuf,
    backup: &PathBuf,
    versions_dir: &PathBuf,
    version: u64,
    list: &Vec<Value>,
) -> std::io::Result<()> {
    if afs::try_exists(path).await.unwrap_or(false) {
        let _ = afs::rename(path, backup).await;
    }
    let body = json!({"version": version, "suggestions": list});
    let bytes = serde_json::to_vec_pretty(&body).unwrap_or_else(|_| b"{}".to_vec());
    save_bytes_atomic(path, &bytes).await?;
    let version_path = versions_dir.join(format!("feedback_engine.v{}.json", version));
    let _ = save_bytes_atomic(&version_path, &bytes).await;
    prune_versions(versions_dir, 3).await;
    Ok(())
}

async fn prune_versions(dir: &PathBuf, keep: usize) {
    let mut versions = list_engine_versions(dir).await;
    if versions.len() <= keep {
        return;
    }
    versions.drain(..keep);
    for v in versions {
        let _ = afs::remove_file(dir.join(format!("feedback_engine.v{}.json", v))).await;
    }
}

async fn list_engine_versions(dir: &PathBuf) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    if let Ok(mut rd) = afs::read_dir(dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(rest) = name.strip_prefix("feedback_engine.v") {
                    if let Some(num) = rest.strip_suffix(".json") {
                        if let Ok(ver) = num.parse::<u64>() {
                            out.push(ver);
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

async fn load_version_bytes(
    current: &Path,
    backup: &Path,
    dir: &Path,
    target: Option<u64>,
) -> Option<(Vec<u8>, u64)> {
    match target {
        Some(v) => {
            let path = dir.join(format!("feedback_engine.v{}.json", v));
            let bytes = afs::read(&path).await.ok()?;
            Some((bytes, v))
        }
        None => {
            if let Ok(bytes) = afs::read(backup).await {
                let val: Value = serde_json::from_slice(&bytes).ok()?;
                let version = val.get("version").and_then(|x| x.as_u64()).unwrap_or(1);
                Some((bytes, version))
            } else {
                let bytes = afs::read(current).await.ok()?;
                let val: Value = serde_json::from_slice(&bytes).ok()?;
                let version = val.get("version").and_then(|x| x.as_u64()).unwrap_or(1);
                Some((bytes, version))
            }
        }
    }
}

fn load_tick_ms() -> u64 {
    static CACHE: OnceCell<u64> = OnceCell::new();
    *CACHE.get_or_init(|| {
        policy::config()
            .and_then(|c| c.tick_ms)
            .or_else(|| {
                std::env::var("ARW_FEEDBACK_TICK_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
            })
            .unwrap_or(500)
    })
}

mod policy {
    use super::*;
    use tokio::sync::RwLock;

    #[derive(Default, Clone, Deserialize)]
    pub struct FbPolicyCfg {
        pub tick_ms: Option<u64>,
        pub apply_per_hour: Option<u32>,
        pub http_timeout_min: Option<u64>,
        pub http_timeout_max: Option<u64>,
        pub mem_limit_min: Option<u64>,
        pub mem_limit_max: Option<u64>,
    }

    static CONFIG: OnceCell<Option<FbPolicyCfg>> = OnceCell::new();
    static WINDOW: OnceCell<RwLock<(i64, u32)>> = OnceCell::new();

    pub fn config() -> Option<FbPolicyCfg> {
        CONFIG
            .get_or_init(|| {
                if let Some(path) = arw_core::resolve_config_path("configs/feedback.toml") {
                    if let Ok(body) = std::fs::read_to_string(path) {
                        return toml::from_str::<FbPolicyCfg>(&body).ok();
                    }
                }
                None
            })
            .clone()
    }

    fn window_bucket() -> &'static RwLock<(i64, u32)> {
        WINDOW.get_or_init(|| RwLock::new((0, 0)))
    }

    pub async fn allow_apply(action: &str, params: &Value) -> Result<(), super::FeedbackError> {
        let mut guard = window_bucket().write().await;
        let now = now_secs();
        if now - guard.0 >= 3600 {
            guard.0 = now;
            guard.1 = 0;
        }
        let cap = apply_per_hour();
        if guard.1 >= cap {
            return Err(super::FeedbackError::PolicyDenied("rate limit".into()));
        }
        drop(guard);

        if !within_bounds(action, params) {
            return Err(super::FeedbackError::PolicyDenied("out of bounds".into()));
        }

        let mut guard = window_bucket().write().await;
        let now = now_secs();
        if now - guard.0 >= 3600 {
            guard.0 = now;
            guard.1 = 0;
        }
        if guard.1 >= cap {
            return Err(super::FeedbackError::PolicyDenied("rate limit".into()));
        }
        guard.1 += 1;
        Ok(())
    }

    pub fn effective_policy() -> Value {
        json!({
            "http_timeout_min": http_timeout_min(),
            "http_timeout_max": http_timeout_max(),
            "mem_limit_min": mem_limit_min(),
            "mem_limit_max": mem_limit_max(),
            "apply_per_hour": apply_per_hour(),
        })
    }

    fn within_bounds(action: &str, params: &Value) -> bool {
        match action {
            "hint" => params
                .get("http_timeout_secs")
                .and_then(|v| v.as_u64())
                .map(|n| {
                    let min = http_timeout_min();
                    let max = http_timeout_max();
                    (min..=max).contains(&n)
                })
                .unwrap_or(false),
            "mem_limit" => params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| {
                    let min = mem_limit_min();
                    let max = mem_limit_max();
                    (min..=max).contains(&n)
                })
                .unwrap_or(false),
            "profile" => params
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| matches!(s, "performance" | "balanced" | "power-saver"))
                .unwrap_or(false),
            _ => false,
        }
    }

    fn now_secs() -> i64 {
        (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()) as i64
    }

    fn http_timeout_min() -> u64 {
        std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MIN")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| config().and_then(|c| c.http_timeout_min))
            .unwrap_or(5)
    }

    fn http_timeout_max() -> u64 {
        std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| config().and_then(|c| c.http_timeout_max))
            .unwrap_or(300)
    }

    fn mem_limit_min() -> u64 {
        std::env::var("ARW_FEEDBACK_MEM_LIMIT_MIN")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| config().and_then(|c| c.mem_limit_min))
            .unwrap_or(50)
    }

    fn mem_limit_max() -> u64 {
        std::env::var("ARW_FEEDBACK_MEM_LIMIT_MAX")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| config().and_then(|c| c.mem_limit_max))
            .unwrap_or(2000)
    }

    fn apply_per_hour() -> u32 {
        std::env::var("ARW_FEEDBACK_APPLY_PER_HOUR")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| config().and_then(|c| c.apply_per_hour))
            .unwrap_or(3)
    }
}
