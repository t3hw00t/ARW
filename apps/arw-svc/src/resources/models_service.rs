//! Models Service — downloads, CAS, events, and metrics
//!
//! Updated: 2025-09-14
//!
//! Responsibilities
//! - Resumable HTTP downloads with mandatory SHA‑256 integrity.
//! - Content‑addressed storage under `<state>/models/by-hash`.
//! - Event publishing: `models.download.progress`, `models.changed`, `models.manifest.written`.
//! - Read-model patches: `models` (items + default), `models_metrics` (counters + `ewma_mbps`).
//! - Egress ledger entries for download allow/deny.
//!
//! Endpoints
//! - Handlers live in `apps/arw-svc/src/ext/models_api.rs` (list, default, download, cancel,
//!   concurrency, CAS GC, metrics, hashes).
//!
//! Configuration
//! - Concurrency: `ARW_MODELS_MAX_CONC`, `ARW_MODELS_MAX_CONC_HARD`.
//! - Progress details: `ARW_DL_PROGRESS_INCLUDE_BUDGET`, `ARW_DL_PROGRESS_INCLUDE_DISK`,
//!   `ARW_DL_PROGRESS_VALIDATE`.
//! - Timeouts: `ARW_DL_IDLE_TIMEOUT_SECS`.
//! - HTTP pool: `ARW_DL_HTTP_KEEPALIVE_SECS`, `ARW_DL_HTTP_POOL_IDLE_SECS`,
//!   `ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST`.
//! - Metrics cadence: `ARW_MODELS_METRICS_PUBLISH_MS`, `ARW_MODELS_METRICS_COALESCE_MS`.
//! - Disk/quota: `ARW_MODELS_DISK_RESERVE_MB`, `ARW_MODELS_MAX_MB`, `ARW_MODELS_QUOTA_MB`.
//! - Throughput smoothing: `ARW_DL_EWMA_ALPHA`.
//!
//! References
//! - Status/code enums are the source of truth here; `spec/asyncapi.yaml` mirrors them.
//! - See `docs/architecture/events_vocabulary.md` and `docs/reference/topics.md`.
use serde_json::{json, Value};

use crate::app_state::AppState;
use once_cell::sync::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{Notify, RwLock, Semaphore}; // for cancel/active job tracking
                                              // Event topics used by this service
use crate::ext::topics::*;

// ---------------- Models download metrics (process-local counters) ----------------
static DL_STARTED: AtomicU64 = AtomicU64::new(0);
static DL_QUEUED: AtomicU64 = AtomicU64::new(0);
static DL_ADMITTED: AtomicU64 = AtomicU64::new(0);
static DL_RESUMED: AtomicU64 = AtomicU64::new(0);
static DL_CANCELED: AtomicU64 = AtomicU64::new(0);
static DL_COMPLETED: AtomicU64 = AtomicU64::new(0);
static DL_COMPLETED_CACHED: AtomicU64 = AtomicU64::new(0);
static DL_ERRORS: AtomicU64 = AtomicU64::new(0);
static DL_BYTES: AtomicU64 = AtomicU64::new(0);

// Coalescing buffer for metrics patches
static METRICS_DIRTY: AtomicBool = AtomicBool::new(false);
static METRICS_NOTIFY: OnceCell<Notify> = OnceCell::new();
fn metrics_notify() -> &'static Notify {
    METRICS_NOTIFY.get_or_init(Notify::new)
}
fn metrics_mark_dirty() {
    METRICS_DIRTY.store(true, Ordering::Relaxed);
    metrics_notify().notify_one();
}

fn metrics_bump_status(s: &str) {
    match s {
        "started" => {
            DL_STARTED.fetch_add(1, Ordering::Relaxed);
        }
        "queued" => {
            DL_QUEUED.fetch_add(1, Ordering::Relaxed);
        }
        "admitted" => {
            DL_ADMITTED.fetch_add(1, Ordering::Relaxed);
        }
        "resumed" => {
            DL_RESUMED.fetch_add(1, Ordering::Relaxed);
        }
        "canceled" => {
            DL_CANCELED.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

pub fn models_metrics_value() -> Value {
    json!({
        "started": DL_STARTED.load(Ordering::Relaxed),
        "queued": DL_QUEUED.load(Ordering::Relaxed),
        "admitted": DL_ADMITTED.load(Ordering::Relaxed),
        "resumed": DL_RESUMED.load(Ordering::Relaxed),
        "canceled": DL_CANCELED.load(Ordering::Relaxed),
        "completed": DL_COMPLETED.load(Ordering::Relaxed),
        "completed_cached": DL_COMPLETED_CACHED.load(Ordering::Relaxed),
        "errors": DL_ERRORS.load(Ordering::Relaxed),
        "bytes_total": DL_BYTES.load(Ordering::Relaxed),
    })
}

/// Publish a coalesced read-model patch for download metrics.
///
/// Shape matches the HTTP snapshot: counters plus an optional `ewma_mbps`.
fn publish_models_metrics_patch(bus: &arw_events::Bus) {
    // Start with process-local counters
    let mut cur = models_metrics_value();
    // Align shape with HTTP snapshot by including EWMA MB/s when available
    if let Some(obj) = cur.as_object_mut() {
        let ewma = crate::ext::io::load_json_file(&crate::ext::paths::downloads_metrics_path())
            .and_then(|v| v.get("ewma_mbps").and_then(|x| x.as_f64()));
        match ewma {
            Some(v) => {
                obj.insert("ewma_mbps".into(), serde_json::Value::from(v));
            }
            None => {
                obj.insert("ewma_mbps".into(), serde_json::Value::Null);
            }
        }
    }
    // Emit under the unified read‑model topic
    crate::ext::read_model::emit_patch(bus, TOPIC_READMODEL_PATCH, "models_metrics", &cur);
}

// Emit JSON Patch for the models state (list + default) under unified topic.
/// Publish a read-model patch for the models list and default.
async fn publish_models_state_patch(bus: &arw_events::Bus) {
    let list = crate::ext::models().read().await.clone();
    let default = crate::ext::default_model().read().await.clone();
    let cur = json!({ "items": list, "default": default });
    crate::ext::read_model::emit_patch(bus, TOPIC_READMODEL_PATCH, "models", &cur);
}

/// Spawn a task that publishes coalesced `models_metrics` read-model patches.
///
/// Controlled by `ARW_MODELS_METRICS_PUBLISH_MS` and `ARW_MODELS_METRICS_COALESCE_MS`.
pub async fn start_models_metrics_publisher(bus: arw_events::Bus) {
    let idle_ms: u64 = ModelsService::env_u64("ARW_MODELS_METRICS_PUBLISH_MS", 2000).max(200);
    let coalesce_ms: u64 = ModelsService::env_u64("ARW_MODELS_METRICS_COALESCE_MS", 250).max(10);
    let notify = metrics_notify();
    let mut idle = tokio::time::interval(std::time::Duration::from_millis(idle_ms));
    loop {
        tokio::select! {
            _ = notify.notified() => {
                tokio::time::sleep(std::time::Duration::from_millis(coalesce_ms)).await;
                let _ = METRICS_DIRTY.swap(false, Ordering::Relaxed);
                publish_models_metrics_patch(&bus);
            }
            _ = idle.tick() => {
                publish_models_metrics_patch(&bus);
            }
        }
    }
}

#[derive(Default)]
pub struct ModelsService;

#[derive(Clone, Debug)]
pub struct DownloadBudgetOverride {
    pub soft_ms: Option<u64>,
    pub hard_ms: Option<u64>,
    pub class: Option<String>,
}

#[derive(Clone, Debug)]
struct EgressLedgerEntry {
    decision: String,
    reason_code: String,
    dest_host: String,
    dest_port: u16,
    dest_proto: String,
    corr_id: String,
    bytes_in: u64,
    duration_ms: u64,
    extra: Option<Value>,
}

#[derive(Default, Clone, Debug)]
struct EgressLedgerEntryBuilder {
    decision: String,
    reason_code: String,
    dest_host: String,
    dest_port: u16,
    dest_proto: String,
    corr_id: String,
    bytes_in: u64,
    duration_ms: u64,
    extra: Option<Value>,
}

impl EgressLedgerEntryBuilder {
    fn dest(mut self, host: impl Into<String>, port: u16, proto: impl Into<String>) -> Self {
        self.dest_host = host.into();
        self.dest_port = port;
        self.dest_proto = proto.into();
        self
    }
    fn corr_id(mut self, id: impl Into<String>) -> Self {
        self.corr_id = id.into();
        self
    }
    fn bytes_in(mut self, n: u64) -> Self {
        self.bytes_in = n;
        self
    }
    fn duration_ms(mut self, n: u64) -> Self {
        self.duration_ms = n;
        self
    }
    fn extra(mut self, v: Value) -> Self {
        self.extra = Some(v);
        self
    }
    fn build(self) -> EgressLedgerEntry {
        EgressLedgerEntry {
            decision: self.decision,
            reason_code: self.reason_code,
            dest_host: self.dest_host,
            dest_port: self.dest_port,
            dest_proto: self.dest_proto,
            corr_id: self.corr_id,
            bytes_in: self.bytes_in,
            duration_ms: self.duration_ms,
            extra: self.extra,
        }
    }
}

impl EgressLedgerEntry {
    fn deny(reason: impl Into<String>) -> EgressLedgerEntryBuilder {
        EgressLedgerEntryBuilder {
            decision: "deny".into(),
            reason_code: reason.into(),
            ..Default::default()
        }
    }
    fn allow(reason: impl Into<String>) -> EgressLedgerEntryBuilder {
        EgressLedgerEntryBuilder {
            decision: "allow".into(),
            reason_code: reason.into(),
            ..Default::default()
        }
    }
}

// Small helper to emit progress events with consistent shape.
fn emit_progress(
    bus: &arw_events::Bus,
    id: &str,
    status: Option<&str>,
    code: Option<&str>,
    budget: Option<&crate::ext::budget::Budget>,
    extra: Option<Value>,
    corr_id: Option<&str>,
) {
    // Validate status/code against our known vocabulary (warn-only by default)
    #[inline]
    fn validate(kind: &str, val: &str) {
        // Enable strict validation via ARW_DL_PROGRESS_VALIDATE=1
        let strict = matches!(
            std::env::var("ARW_DL_PROGRESS_VALIDATE").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
        );
        if kind == "status" && !ModelsService::PROGRESS_STATUS.contains(&val) && strict {
            tracing::warn!("unknown models.progress status='{}'", val);
        }
        if kind == "code" && !ModelsService::PROGRESS_CODES.contains(&val) && strict {
            tracing::warn!("unknown models.progress code='{}'", val);
        }
    }

    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(id.to_string()));
    if let Some(s) = status {
        validate("status", s);
        obj.insert("status".into(), Value::String(s.to_string()));
    }
    if let Some(c) = code {
        validate("code", c);
        obj.insert("code".into(), Value::String(c.to_string()));
    }
    if let Some(b) = budget {
        obj.insert("budget".into(), b.as_json());
    }
    if let Some(Value::Object(map)) = extra {
        for (k, v) in map {
            obj.insert(k, v);
        }
    }
    if let Some(cid) = corr_id {
        obj.insert("corr_id".into(), Value::String(cid.to_string()));
    }
    let mut payload = Value::Object(obj);
    crate::ext::corr::ensure_corr(&mut payload);
    // Increment metrics by status when present
    if let Some(Value::String(s)) = payload.get("status") {
        metrics_bump_status(s);
    }
    bus.publish(TOPIC_PROGRESS, &payload);
    // Mark for coalesced patch publish
    metrics_mark_dirty();
}

// Small helper to emit standardized error events and audit them.
async fn emit_error(
    bus: &arw_events::Bus,
    id: &str,
    code: &str,
    message: &str,
    budget: Option<&crate::ext::budget::Budget>,
    extra: Option<Value>,
    corr_id: Option<&str>,
) {
    // Validate error code (warn-only unless ARW_DL_PROGRESS_VALIDATE enables strict warnings)
    let strict = matches!(
        std::env::var("ARW_DL_PROGRESS_VALIDATE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
    );
    if strict && !ModelsService::PROGRESS_CODES.contains(&code) {
        tracing::warn!("unknown models.error code='{}'", code);
    }
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(id.to_string()));
    obj.insert("status".into(), Value::String("error".into()));
    obj.insert("error".into(), Value::String(message.to_string()));
    obj.insert("code".into(), Value::String(code.to_string()));
    if let Some(b) = budget {
        obj.insert("budget".into(), b.as_json());
    }
    if let Some(Value::Object(map)) = extra {
        for (k, v) in map {
            obj.insert(k, v);
        }
    }
    if let Some(cid) = corr_id {
        obj.insert("corr_id".into(), Value::String(cid.to_string()));
    }
    let mut payload = Value::Object(obj);
    crate::ext::corr::ensure_corr(&mut payload);
    DL_ERRORS.fetch_add(1, Ordering::Relaxed);
    bus.publish(TOPIC_PROGRESS, &payload);
    metrics_mark_dirty();
    crate::ext::io::audit_event("models.download.error", &payload).await;

    // Reflect error status into models list to avoid "downloading" getting stuck.
    {
        let mut v = crate::ext::models().write().await;
        if let Some(m) = v
            .iter_mut()
            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(id))
        {
            if let Some(obj) = m.as_object_mut() {
                obj.insert("status".into(), Value::String("error".into()));
                obj.insert("error_code".into(), Value::String(code.to_string()));
            }
        }
        // Persist models and notify change
        let _ = crate::ext::io::save_json_file_async(
            &crate::ext::paths::models_path(),
            &Value::Array(v.clone()),
        )
        .await;
    }
    bus.publish(TOPIC_MODELS_CHANGED, &json!({"op":"error","id": id}));
    // Stream a read-model patch for consumers applying deltas
    publish_models_state_patch(bus).await;
}

// tests are placed at the end of file to avoid clippy items-after-test-module

impl ModelsService {
    #[inline]
    fn env_u64(name: &str, default: u64) -> u64 {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(default)
    }
    #[inline]
    fn env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default)
    }
    #[inline]
    fn env_f64(name: &str, default: f64) -> f64 {
        std::env::var(name)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(default)
    }
    // Emit the standardized start event and audit it; returns corr_id.
    async fn emit_download_started(
        bus: &arw_events::Bus,
        id: &str,
        url: &str,
        budget: &crate::ext::budget::Budget,
    ) -> String {
        let mut p = json!({
            "id": id,
            "url": Self::redact_url_for_logs(url),
            "budget": budget.as_json()
        });
        let corr = crate::ext::corr::ensure_corr(&mut p);
        crate::ext::io::audit_event("models.download", &p).await;
        emit_progress(
            bus,
            id,
            Some("started"),
            Some("started"),
            if Self::progress_include_budget() {
                Some(budget)
            } else {
                None
            },
            None,
            Some(&corr),
        );
        corr
    }

    // After acquiring the concurrency permit, emit "admitted" and reflect 'downloading' in models list.
    async fn on_admitted_set_downloading(
        state: &crate::app_state::AppState,
        id: &str,
        provider: &str,
        budget: &crate::ext::budget::Budget,
        corr_id: &str,
    ) {
        emit_progress(
            &state.bus,
            id,
            Some("admitted"),
            Some("admitted"),
            if Self::progress_include_budget() {
                Some(budget)
            } else {
                None
            },
            None,
            Some(corr_id),
        );
        // Now that we've been admitted, reflect 'downloading' in the models list.
        {
            let mut v = crate::ext::models().write().await;
            if let Some(m) = v
                .iter_mut()
                .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(id))
            {
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("status".into(), Value::String("downloading".into()));
                    obj.insert("provider".into(), Value::String(provider.to_string()));
                }
            } else {
                v.push(json!({"id": id, "provider": provider.to_string(), "status":"downloading"}));
            }
        }
        // Publish a patch so UIs can reflect the 'downloading' status
        publish_models_state_patch(&state.bus).await;
    }

    // Extract destination tuple for egress events and ledger (host, port, protocol)
    fn dest_tuple(url: &str) -> (String, u16, String) {
        if let Ok(u) = reqwest::Url::parse(url) {
            let host = u.host_str().unwrap_or("").to_string();
            let port = u.port().unwrap_or_else(|| match u.scheme() {
                "https" => 443,
                "http" => 80,
                _ => 0,
            });
            let proto = u.scheme().to_string();
            (host, port, proto)
        } else {
            (String::new(), 0u16, String::from("http"))
        }
    }

    // Validate that a Content-Range header matches our resume offset
    fn validate_resume_content_range(resume_from: u64, content_range: &str) -> bool {
        let s = content_range.trim();
        if let Some(rest) = s.strip_prefix("bytes ") {
            let parts: Vec<&str> = rest.split('/').collect();
            if let Some(range) = parts.first() {
                let mut it = range.split('-');
                if let (Some(start_s), Some(_end_s)) = (it.next(), it.next()) {
                    if let Ok(start_off) = start_s.parse::<u64>() {
                        return start_off == resume_from;
                    }
                }
            }
        }
        false
    }

    // Finalize a successful download: promote/copy tmp into place (CAS if sha known),
    // fsync, remove resume meta, write manifest, update models list, metrics, events, and ledger.
    #[allow(clippy::too_many_arguments)]
    async fn finalize_and_write_manifest(
        sp: &crate::app_state::AppState,
        id: &str,
        provider: &str,
        corr_id: &str,
        budget: &crate::ext::budget::Budget,
        expect_sha: &Option<String>,
        final_name: &str,
        target_dir: &std::path::Path,
        tmp: &std::path::Path,
        meta_path: &std::path::Path,
        reserve_bytes: u64,
        dest_host: &str,
        dest_port: u16,
        dest_proto: &str,
        t0: std::time::Instant,
        target: &mut std::path::PathBuf,
    ) {
        use tokio::fs as afs;
        // Promote tmp to a content-addressed target path now that verification passed
        // Layout: <state>/models/by-hash/<sha256>[.<ext>]
        let cas_file_name: String;
        if let Some(ref exp) = expect_sha {
            let cas_dir = target_dir.join("by-hash");
            if let Err(e) = afs::create_dir_all(&cas_dir).await {
                emit_error(
                    &sp.bus,
                    id,
                    "mkdir-failed",
                    &format!("Failed to create directory: {}", e),
                    Some(budget),
                    None,
                    Some(corr_id),
                )
                .await;
                return;
            }
            // Derive a canonical filename using the hash and the original extension (if any)
            let ext = final_name.rsplit('.').next().filter(|s| *s != final_name);
            cas_file_name = match ext {
                Some(ex) if !ex.is_empty() => format!("{}.{}", exp, ex),
                _ => exp.clone(),
            };
            let cas_target = cas_dir.join(&cas_file_name);
            if afs::metadata(&cas_target).await.is_ok() {
                // Already have this blob; discard temp
                let _ = afs::remove_file(tmp).await;
            } else if let Err(_e) = afs::rename(tmp, &cas_target).await {
                // If rename fails (Windows lock or cross-device), try remove + rename,
                // and finally fall back to copy + remove.
                let _ = afs::remove_file(&cas_target).await;
                match afs::rename(tmp, &cas_target).await {
                    Ok(_) => {}
                    Err(e2) => match afs::copy(tmp, &cas_target).await {
                        Ok(_) => {
                            let _ = afs::remove_file(tmp).await;
                        }
                        Err(_) => {
                            emit_error(
                                &sp.bus,
                                id,
                                "finalize-failed",
                                &format!("Finalize failed: {}", e2),
                                Some(budget),
                                None,
                                Some(corr_id),
                            )
                            .await;
                            return;
                        }
                    },
                }
            }
            // update target to the content-addressed path
            *target = cas_target.clone();
        } else {
            // Fallback (should not happen since we require sha256): keep original target path
            if let Err(_e) = afs::rename(tmp, &*target).await {
                let _ = afs::remove_file(&*target).await;
                match afs::rename(tmp, &*target).await {
                    Ok(_) => {}
                    Err(e2) => match afs::copy(tmp, &*target).await {
                        Ok(_) => {
                            let _ = afs::remove_file(tmp).await;
                        }
                        Err(_) => {
                            emit_error(
                                &sp.bus,
                                id,
                                "finalize-failed",
                                &format!("Finalize failed: {}", e2),
                                Some(budget),
                                None,
                                Some(corr_id),
                            )
                            .await;
                            return;
                        }
                    },
                }
            }
            // Use the final_name as the canonical file name when sha is unknown
            // (only for completeness; sha is required by API)
            cas_file_name = final_name.to_string();
        }
        // fsync finalized CAS/target file for durability
        if let Ok(f) = afs::File::open(&*target).await {
            let _ = f.sync_all().await;
        }
        // cleanup sidecar meta on success
        let _ = afs::remove_file(meta_path).await;
        // Write a sidecar manifest <id>.json alongside the model
        let manifest_path = target_dir.join(format!("{}.json", Self::sanitize_file_name(id)));
        // Emit manifest written event
        let mut ev = json!({"id": id, "manifest_path": manifest_path.to_string_lossy(), "sha256": expect_sha.clone(), "cas": "sha256", "corr_id": corr_id});
        crate::ext::corr::ensure_corr(&mut ev);
        sp.bus.publish(TOPIC_MODELS_MANIFEST_WRITTEN, &ev);
        let bytes = match afs::metadata(&*target).await {
            Ok(md) => md.len(),
            Err(_) => 0,
        };
        let mut manifest = serde_json::Map::new();
        manifest.insert("id".into(), Value::String(id.to_string()));
        // file: canonical content-addressed filename; name: original display filename (if different)
        manifest.insert("file".into(), Value::String(cas_file_name.clone()));
        if cas_file_name != final_name {
            manifest.insert("name".into(), Value::String(final_name.to_string()));
        }
        manifest.insert(
            "path".into(),
            Value::String(target.to_string_lossy().to_string()),
        );
        if let Some(ref sh) = expect_sha {
            manifest.insert("sha256".into(), Value::String(sh.clone()));
            manifest.insert("cas".into(), Value::String("sha256".into()));
        }
        manifest.insert(
            "bytes".into(),
            Value::Number(serde_json::Number::from(bytes)),
        );
        manifest.insert("provider".into(), Value::String(provider.to_string()));
        manifest.insert("verified".into(), Value::Bool(true));
        let _ =
            crate::ext::io::save_json_file_async(&manifest_path, &Value::Object(manifest)).await;
        // Update EWMA throughput based on observed bytes/time
        let elapsed_ms = t0.elapsed().as_millis() as u64;
        if elapsed_ms > 0 {
            if let Ok(md) = afs::metadata(&*target).await {
                let bytes = md.len() as f64;
                let mbps = (bytes / (1024.0 * 1024.0)) / (elapsed_ms as f64 / 1000.0);
                Self::update_ewma_mbps(mbps).await;
            }
        }
        let mut p = json!({"id": id, "status":"complete", "file": final_name, "provider": provider, "cas_file": cas_file_name});
        if Self::progress_include_disk() {
            if let (Ok(av), Ok(tt)) = (
                fs2::available_space(target_dir),
                fs2::total_space(target_dir),
            ) {
                p["disk"] = json!({"available": av, "total": tt, "reserve": reserve_bytes});
            }
        }
        // Metrics: completed (non-cached); add bytes downloaded
        DL_COMPLETED.fetch_add(1, Ordering::Relaxed);
        DL_BYTES.fetch_add(bytes, Ordering::Relaxed);
        // Emit standardized completion event
        let mut extra = p.clone();
        if let Some(obj) = extra.as_object_mut() {
            obj.remove("id");
            obj.remove("status");
        }
        emit_progress(
            &sp.bus,
            id,
            Some("complete"),
            Some("complete"),
            if Self::progress_include_budget() {
                Some(budget)
            } else {
                None
            },
            Some(extra),
            Some(corr_id),
        );
        // Audit completion with full object
        crate::ext::io::audit_event("models.download.complete", &p).await;
        // Append egress ledger (best-effort)
        Self::append_egress_ledger(
            &sp.bus,
            EgressLedgerEntry::allow("models.download")
                .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                .corr_id(corr_id.to_string())
                .bytes_in(bytes)
                .duration_ms(elapsed_ms)
                .build(),
        )
        .await;
        {
            let mut v = crate::ext::models().write().await;
            if let Some(m) = v
                .iter_mut()
                .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(id))
            {
                let mut obj = serde_json::Map::new();
                obj.insert("id".into(), Value::String(id.to_string()));
                obj.insert("provider".into(), Value::String(provider.to_string()));
                obj.insert("status".into(), Value::String("available".into()));
                obj.insert(
                    "path".into(),
                    Value::String(target.to_string_lossy().to_string()),
                );
                if let Some(ref sh) = expect_sha {
                    obj.insert("sha256".into(), Value::String(sh.clone()));
                    obj.insert("cas".into(), Value::String("sha256".into()));
                    obj.insert("file".into(), Value::String(cas_file_name.clone()));
                }
                if let Ok(md) = afs::metadata(&*target).await {
                    obj.insert(
                        "bytes".into(),
                        Value::Number(serde_json::Number::from(md.len())),
                    );
                }
                *m = Value::Object(obj);
            }
        }
        let _ = crate::ext::io::save_json_file_async(
            &crate::ext::paths::models_path(),
            &Value::Array(crate::ext::models().read().await.clone()),
        )
        .await;
        sp.bus
            .publish(TOPIC_MODELS_CHANGED, &json!({"op":"downloaded","id": id}));
        publish_models_state_patch(&sp.bus).await;
        metrics_mark_dirty();
    }

    // Stream the HTTP response into tmp, supporting resume/validators, budget and disk checks,
    // progress heartbeats, and finally finalize/promote on success.
    #[allow(clippy::too_many_arguments)]
    async fn stream_download_loop(
        sp: &crate::app_state::AppState,
        id: &str,
        provider: &str,
        url: &str,
        dest_host: &str,
        dest_port: u16,
        dest_proto: &str,
        corr_id: &str,
        budget: &crate::ext::budget::Budget,
        client: &reqwest::Client,
        resp: reqwest::Response,
        resume_from: &mut u64,
        target_dir: &std::path::Path,
        final_name: &mut String,
        target: &mut std::path::PathBuf,
        tmp: &std::path::Path,
        meta_path: &std::path::Path,
        expect_sha: &Option<String>,
        reserve_bytes: u64,
        max_bytes: u64,
    ) {
        use futures_util::StreamExt;
        use sha2::Digest;
        use tokio::fs as afs;
        use tokio::io::{AsyncWriteExt, BufWriter};
        let total_rem = resp.content_length().unwrap_or(0);
        let status = resp.status();
        // Validate acceptable HTTP status for initial or ranged request
        let ok_status = status.is_success()
            || (*resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT);
        if !ok_status {
            let extra = json!({"status": status.as_str()});
            emit_error(
                &sp.bus,
                id,
                "downstream-http-status",
                &format!("HTTP status {}", status.as_u16()),
                Some(budget),
                Some(extra),
                Some(corr_id),
            )
            .await;
            return;
        }
        // Capture validators for future resumes and parse Content-Disposition filename
        let t0 = std::time::Instant::now();
        Self::save_resume_validators(meta_path, resp.headers()).await;
        if let Some(cd) = resp
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(fname) = Self::filename_from_content_disposition(cd) {
                let cand = Self::sanitize_file_name(&fname);
                if !cand.is_empty() {
                    *final_name = cand;
                    *target = target_dir.join(&*final_name);
                }
            }
        }
        // If resuming, validate Content-Range consistency with our offset.
        if *resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            if let Some(cr) = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
            {
                let ok_range = Self::validate_resume_content_range(*resume_from, cr);
                if !ok_range {
                    // Upstream changed or server returned an unexpected range: abort safely.
                    let _ = afs::remove_file(tmp).await;
                    let _ = afs::remove_file(meta_path).await;
                    let extra = json!({ "expected_offset": *resume_from, "content_range": cr });
                    emit_error(
                        &sp.bus,
                        id,
                        "upstream-changed",
                        "content-range does not match resume offset",
                        Some(budget),
                        Some(extra),
                        Some(corr_id),
                    )
                    .await;
                    return;
                }
            } else {
                // No Content-Range header on 206 response while resuming. Abort.
                let _ = afs::remove_file(tmp).await;
                let _ = afs::remove_file(meta_path).await;
                emit_error(
                    &sp.bus,
                    id,
                    "resume-no-content-range",
                    "missing Content-Range on partial content",
                    Some(budget),
                    None,
                    Some(corr_id),
                )
                .await;
                return;
            }
        }
        let total_all = if *resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            let mut p = json!({"offset": *resume_from});
            if Self::progress_include_disk() {
                if let (Ok(av), Ok(tt)) = (
                    fs2::available_space(target_dir),
                    fs2::total_space(target_dir),
                ) {
                    p["disk"] = json!({"available": av, "total": tt, "reserve": reserve_bytes});
                }
            }
            emit_progress(
                &sp.bus,
                id,
                Some("resumed"),
                Some("resumed"),
                if Self::progress_include_budget() {
                    Some(budget)
                } else {
                    None
                },
                Some(p),
                Some(corr_id),
            );
            *resume_from + total_rem
        } else {
            if *resume_from > 0 {
                let _ = afs::remove_file(tmp).await;
                *resume_from = 0;
            }
            total_rem
        };
        // Enforce quota if configured (post-GET when total known)
        if total_all > 0 {
            if let Some(quota) = Self::models_quota_bytes() {
                let (cas_total, _files) = Self::cas_usage_totals().await;
                if cas_total.saturating_add(total_all) > quota {
                    let extra = json!({"quota": quota, "cas_total": cas_total, "need": total_all});
                    emit_error(
                        &sp.bus,
                        id,
                        "quota-exceeded",
                        "Models quota exceeded",
                        Some(budget),
                        Some(extra),
                        Some(corr_id),
                    )
                    .await;
                    // Ledger: deny
                    Self::append_egress_ledger(
                        &sp.bus,
                        EgressLedgerEntry::deny("quota-exceeded")
                            .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                            .corr_id(corr_id.to_string())
                            .build(),
                    )
                    .await;
                    return;
                }
            }
        }
        // Hard cap by expected total when known
        if max_bytes > 0 && total_all > 0 && total_all > max_bytes {
            let extra = json!({"total": total_all, "max_bytes": max_bytes});
            emit_error(
                &sp.bus,
                id,
                "size-limit",
                "Size exceeds limit",
                Some(budget),
                Some(extra),
                Some(corr_id),
            )
            .await;
            // Ledger: deny
            Self::append_egress_ledger(
                &sp.bus,
                EgressLedgerEntry::deny("size-limit")
                    .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                    .corr_id(corr_id.to_string())
                    .build(),
            )
            .await;
            return;
        }
        // Open file (append if resuming)
        let mut file: BufWriter<afs::File> = if *resume_from > 0 {
            match afs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(tmp)
                .await
            {
                Ok(f) => BufWriter::with_capacity(1 << 20, f),
                Err(e) => {
                    emit_error(
                        &sp.bus,
                        id,
                        "open-failed",
                        &format!("Open failed: {}", e),
                        Some(budget),
                        None,
                        Some(corr_id),
                    )
                    .await;
                    return;
                }
            }
        } else {
            match afs::File::create(tmp).await {
                Ok(f) => BufWriter::with_capacity(1 << 20, f),
                Err(e) => {
                    emit_error(
                        &sp.bus,
                        id,
                        "create-failed",
                        &format!("Create failed: {}", e),
                        Some(budget),
                        None,
                        Some(corr_id),
                    )
                    .await;
                    return;
                }
            }
        };
        let mut hasher_opt = if expect_sha.is_some() && *resume_from == 0 {
            Some(sha2::Sha256::new())
        } else {
            None
        };
        let mut downloaded: u64 = 0;
        // Idle timeout handling
        let idle_timeout = Self::idle_timeout_duration();
        let mut last_chunk = std::time::Instant::now();
        let mut degraded_sent = false;
        // Soft-budget degrade threshold (percentage of soft budget used)
        let soft_degrade_ms = if budget.soft_ms > 0 {
            let pct = Self::env_u64("ARW_BUDGET_SOFT_DEGRADE_PCT", 80).clamp(1, 99);
            Some(budget.soft_ms.saturating_mul(pct) / 100)
        } else {
            None
        };
        let mut stream = resp.bytes_stream();
        'stream_loop: loop {
            // Idle timeout enforcement when no hard budget
            if budget.hard_ms == 0 {
                if let Some(dur) = idle_timeout {
                    if last_chunk.elapsed() >= dur {
                        let _ = afs::remove_file(tmp).await;
                        let _ = afs::remove_file(meta_path).await;
                        let p = json!({"offset": *resume_from, "reason": "idle-timeout"});
                        emit_error(
                            &sp.bus,
                            id,
                            "idle-timeout",
                            "No progress within idle timeout",
                            Some(budget),
                            Some(p),
                            Some(corr_id),
                        )
                        .await;
                        // Ledger: deny
                        Self::append_egress_ledger(
                            &sp.bus,
                            EgressLedgerEntry::deny("idle-timeout")
                                .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                                .corr_id(corr_id.to_string())
                                .bytes_in(*resume_from + downloaded)
                                .duration_ms(budget.spent_ms())
                                .build(),
                        )
                        .await;
                        return;
                    }
                }
            }
            let next_chunk = match stream.next().await {
                None => None,
                Some(Err(e)) => {
                    // If we’re resuming and server closed stream, try re-issuing range GET once
                    if *resume_from > 0 {
                        // flush file to be safe
                        let _ = file.flush().await;
                        let mut rq2 = client.get(url);
                        rq2 = budget.apply_to_request(rq2);
                        rq2 = rq2.header(
                            reqwest::header::RANGE,
                            format!("bytes={}-", *resume_from + downloaded),
                        );
                        if let Ok(r2) = rq2.send().await {
                            let st = r2.status();
                            if st == reqwest::StatusCode::PARTIAL_CONTENT {
                                // refresh validators and continue
                                Self::save_resume_validators(meta_path, r2.headers()).await;
                                let mut p = json!({
                                    "offset": *resume_from + downloaded,
                                    "reason": "resync",
                                });
                                if Self::progress_include_disk() {
                                    if let (Ok(av), Ok(tt)) = (
                                        fs2::available_space(target_dir),
                                        fs2::total_space(target_dir),
                                    ) {
                                        p["disk"] = json!({"available": av, "total": tt, "reserve": reserve_bytes});
                                    }
                                }
                                emit_progress(
                                    &sp.bus,
                                    id,
                                    Some("resync"),
                                    Some("resync"),
                                    if Self::progress_include_budget() {
                                        Some(budget)
                                    } else {
                                        None
                                    },
                                    Some(p),
                                    Some(corr_id),
                                );
                                stream = r2.bytes_stream();
                                continue 'stream_loop;
                            } else if st == reqwest::StatusCode::OK {
                                // Server ignored range; restart from zero
                                let _ = afs::remove_file(tmp).await;
                                let _ = afs::remove_file(meta_path).await;
                                // Refresh validators
                                let etag_val = r2
                                    .headers()
                                    .get(reqwest::header::ETAG)
                                    .and_then(|v| v.to_str().ok())
                                    .map(|s| s.to_string());
                                let lm_val = r2
                                    .headers()
                                    .get(reqwest::header::LAST_MODIFIED)
                                    .and_then(|v| v.to_str().ok())
                                    .map(|s| s.to_string());
                                if etag_val.is_some() || lm_val.is_some() {
                                    let mut obj = serde_json::Map::new();
                                    if let Some(e) = &etag_val {
                                        obj.insert("etag".into(), Value::String(e.clone()));
                                    }
                                    if let Some(lm) = &lm_val {
                                        obj.insert(
                                            "last_modified".into(),
                                            Value::String(lm.clone()),
                                        );
                                    }
                                    let _ = afs::write(
                                        meta_path,
                                        serde_json::to_vec(&Value::Object(obj)).unwrap_or_default(),
                                    )
                                    .await;
                                }
                                match afs::File::create(tmp).await {
                                    Ok(f) => {
                                        file = BufWriter::with_capacity(1 << 20, f);
                                        *resume_from = 0;
                                        downloaded = 0;
                                        if expect_sha.is_some() {
                                            hasher_opt = Some(sha2::Sha256::new());
                                        }
                                        emit_progress(
                                            &sp.bus,
                                            id,
                                            Some("resync"),
                                            Some("resync"),
                                            if Self::progress_include_budget() {
                                                Some(budget)
                                            } else {
                                                None
                                            },
                                            None,
                                            Some(corr_id),
                                        );
                                        stream = r2.bytes_stream();
                                        continue 'stream_loop;
                                    }
                                    Err(e) => {
                                        emit_error(
                                            &sp.bus,
                                            id,
                                            "create-failed",
                                            &format!("Create failed: {}", e),
                                            Some(budget),
                                            None,
                                            Some(corr_id),
                                        )
                                        .await;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Some(Err(e))
                }
                other => other,
            };
            match next_chunk {
                None => break,
                Some(Ok(bytes)) => {
                    // Enforce hard budget mid-stream
                    if budget.hard_exhausted() {
                        let _ = afs::remove_file(tmp).await;
                        let extra = json!({"spent_ms": budget.spent_ms()});
                        emit_error(
                            &sp.bus,
                            id,
                            "hard-exhausted",
                            "Hard budget exhausted",
                            Some(budget),
                            Some(extra),
                            Some(corr_id),
                        )
                        .await;
                        // Ledger: deny
                        Self::append_egress_ledger(
                            &sp.bus,
                            EgressLedgerEntry::deny("hard-exhausted")
                                .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                                .corr_id(corr_id.to_string())
                                .bytes_in(*resume_from + downloaded)
                                .duration_ms(budget.spent_ms())
                                .build(),
                        )
                        .await;
                        return;
                    }
                    // Fire a one-time degrade notification when soft budget crosses threshold
                    if let Some(th) = soft_degrade_ms {
                        if !degraded_sent && budget.spent_ms() >= th {
                            degraded_sent = true;
                            let p = json!({ "reason": "soft budget threshold", "spent_ms": budget.spent_ms() });
                            emit_progress(
                                &sp.bus,
                                id,
                                Some("degraded"),
                                Some("soft-exhausted"),
                                if Self::progress_include_budget() {
                                    Some(budget)
                                } else {
                                    None
                                },
                                Some(p),
                                Some(corr_id),
                            );
                        }
                    }
                    if Self::is_canceled(
                        &ModelsService::current_job_id(id).await.unwrap_or_default(),
                    )
                    .await
                    {
                        let _ = afs::remove_file(tmp).await;
                        let _ = afs::remove_file(meta_path).await;
                        emit_progress(
                            &sp.bus,
                            id,
                            Some("canceled"),
                            Some("canceled-by-user"),
                            if Self::progress_include_budget() {
                                Some(budget)
                            } else {
                                None
                            },
                            None,
                            Some(corr_id),
                        );
                        let p2 = json!({"id": id, "status": "canceled"});
                        crate::ext::io::audit_event("models.download.canceled", &p2).await;
                        {
                            let mut v = crate::ext::models().write().await;
                            if let Some(m) = v
                                .iter_mut()
                                .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(id))
                            {
                                if let Some(obj) = m.as_object_mut() {
                                    obj.insert("status".into(), Value::String("canceled".into()));
                                }
                            }
                            let _ = crate::ext::io::save_json_file_async(
                                &crate::ext::paths::models_path(),
                                &Value::Array(v.clone()),
                            )
                            .await;
                        }
                        sp.bus
                            .publish(TOPIC_MODELS_CHANGED, &json!({"op":"canceled","id": id}));
                        return;
                    }
                    if let Err(e) = file.write_all(&bytes).await {
                        emit_error(
                            &sp.bus,
                            id,
                            "io-write",
                            &format!("Write failed: {}", e),
                            Some(budget),
                            None,
                            Some(corr_id),
                        )
                        .await;
                        return;
                    }
                    if let Some(ref mut h) = hasher_opt {
                        h.update(&bytes);
                    }
                    downloaded += bytes.len() as u64;
                    // Enforce max size during stream when total unknown
                    if max_bytes > 0 && *resume_from + downloaded > max_bytes {
                        let _ = afs::remove_file(tmp).await;
                        let _ = afs::remove_file(meta_path).await;
                        let extra = json!({"downloaded": *resume_from + downloaded, "max_bytes": max_bytes});
                        emit_error(
                            &sp.bus,
                            id,
                            "size-limit-stream",
                            "Size limit exceeded mid-stream",
                            Some(budget),
                            Some(extra),
                            Some(corr_id),
                        )
                        .await;
                        // Ledger: deny
                        Self::append_egress_ledger(
                            &sp.bus,
                            EgressLedgerEntry::deny("size-limit-stream")
                                .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                                .corr_id(corr_id.to_string())
                                .bytes_in(*resume_from + downloaded)
                                .duration_ms(budget.spent_ms())
                                .build(),
                        )
                        .await;
                        return;
                    }
                    // Disk space safety check
                    if let Ok(avail) = fs2::available_space(target_dir) {
                        if avail < reserve_bytes {
                            // low-disk guard
                            let _ = afs::remove_file(tmp).await;
                            let _ = afs::remove_file(meta_path).await;
                            let extra = json!({"downloaded": *resume_from + downloaded, "available": avail, "reserve": reserve_bytes});
                            emit_error(
                                &sp.bus,
                                id,
                                "disk-insufficient-stream",
                                "Insufficient disk space mid-stream",
                                Some(budget),
                                Some(extra),
                                Some(corr_id),
                            )
                            .await;
                            // Ledger: deny
                            Self::append_egress_ledger(
                                &sp.bus,
                                EgressLedgerEntry::deny("disk-insufficient-stream")
                                    .dest(dest_host.to_string(), dest_port, dest_proto.to_string())
                                    .corr_id(corr_id.to_string())
                                    .bytes_in(*resume_from + downloaded)
                                    .duration_ms(budget.spent_ms())
                                    .build(),
                            )
                            .await;
                            return;
                        }
                    }
                    // Heartbeat progress (percent when total known)
                    if total_all > 0 {
                        let pct = (((*resume_from + downloaded) * 100) / total_all).min(100);
                        let mut p =
                            json!({"progress": pct, "downloaded": *resume_from + downloaded});
                        if Self::progress_include_disk() {
                            if let (Ok(av), Ok(tt)) = (
                                fs2::available_space(target_dir),
                                fs2::total_space(target_dir),
                            ) {
                                p["disk"] =
                                    json!({"available": av, "total": tt, "reserve": reserve_bytes});
                            }
                        }
                        emit_progress(
                            &sp.bus,
                            id,
                            Some("downloading"),
                            Some("progress"),
                            if Self::progress_include_budget() {
                                Some(budget)
                            } else {
                                None
                            },
                            Some(p),
                            Some(corr_id),
                        );
                    }
                    last_chunk = std::time::Instant::now();
                }
                Some(Err(e)) => {
                    // Flush and surface error
                    let _ = file.flush().await;
                    emit_error(
                        &sp.bus,
                        id,
                        "io-read",
                        &format!("Read failed: {}", e),
                        Some(budget),
                        None,
                        Some(corr_id),
                    )
                    .await;
                    return;
                }
            }
        }
        // Stream ended. Flush and verify if needed
        if let Err(e) = file.flush().await {
            emit_error(
                &sp.bus,
                id,
                "flush-failed",
                &format!("Flush failed: {}", e),
                Some(budget),
                None,
                Some(corr_id),
            )
            .await;
            return;
        }
        if let Some(exp) = expect_sha.as_ref() {
            // If we hashed on the fly, finish from hasher; else fully re-read to compute hash
            let actual = match hasher_opt.take() {
                Some(h) => format!("{:x}", h.finalize()),
                None => match afs::File::open(tmp).await {
                    Ok(mut f) => {
                        use tokio::io::AsyncReadExt;
                        let mut h = sha2::Sha256::new();
                        let mut buf = vec![0u8; 1 << 20];
                        loop {
                            match f.read(&mut buf).await {
                                Ok(0) => break,
                                Ok(n) => h.update(&buf[..n]),
                                Err(e) => {
                                    emit_error(
                                        &sp.bus,
                                        id,
                                        "verify-read-failed",
                                        &format!("Verify read failed: {}", e),
                                        Some(budget),
                                        None,
                                        Some(corr_id),
                                    )
                                    .await;
                                    return;
                                }
                            }
                        }
                        format!("{:x}", h.finalize())
                    }
                    Err(e) => {
                        emit_error(
                            &sp.bus,
                            id,
                            "verify-open-failed",
                            &format!("Verify open failed: {}", e),
                            Some(budget),
                            None,
                            Some(corr_id),
                        )
                        .await;
                        return;
                    }
                },
            };
            if actual != *exp {
                let _ = afs::remove_file(tmp).await;
                let _ = afs::remove_file(meta_path).await;
                let extra = json!({"expected": exp.clone(), "actual": actual});
                emit_error(
                    &sp.bus,
                    id,
                    "checksum-mismatch",
                    "checksum mismatch",
                    Some(budget),
                    Some(extra),
                    Some(corr_id),
                )
                .await;
                return;
            }
        }
        // If server reported a total size, ensure the file matches
        if total_all > 0 {
            if let Ok(md) = afs::metadata(tmp).await {
                if md.len() != total_all {
                    let _ = afs::remove_file(tmp).await;
                    let _ = afs::remove_file(meta_path).await;
                    let extra = json!({"expected_bytes": total_all, "actual_bytes": md.len()});
                    emit_error(
                        &sp.bus,
                        id,
                        "size-mismatch",
                        "size mismatch",
                        Some(budget),
                        Some(extra),
                        Some(corr_id),
                    )
                    .await;
                    return;
                }
            }
        }
        // Finalize into place and write manifest, events, ledger
        Self::finalize_and_write_manifest(
            sp,
            id,
            provider,
            corr_id,
            budget,
            expect_sha,
            final_name,
            target_dir,
            tmp,
            meta_path,
            reserve_bytes,
            dest_host,
            dest_port,
            dest_proto,
            t0,
            target,
        )
        .await;
    }
    // ---- Progress/Event Vocabulary (single source of truth) ----
    /// Allowed `status` values for `models.download.progress` events.
    ///
    /// Keep in sync with documentation:
    /// - docs: `docs/architecture/events_vocabulary.md` (statuses list)
    /// - reference: `docs/reference/topics.md`
    ///
    /// See also: `progress_codes()` for allowed `code` values.
    pub const PROGRESS_STATUS: [&'static str; 13] = [
        "started",
        "queued",
        "admitted",
        "downloading",
        "resumed",
        "resync",
        "degraded",
        "canceled",
        "complete",
        "cancel-requested",
        "no-active-job",
        "cache-mismatch",
        "error",
    ];
    /// Allowed `code` values for `models.download.progress` (and error) events.
    ///
    /// Codes are hyphenated, stable machine hints. Keep in sync with docs:
    /// - docs: `docs/architecture/events_vocabulary.md` (common code values)
    /// - reference: `docs/reference/topics.md`
    pub const PROGRESS_CODES: [&'static str; 44] = [
        // lifecycle & progress codes
        "started",
        "queued",
        "admitted",
        "downloading",
        "progress",
        "resumed",
        "resync",
        "degraded",
        "canceled-by-user",
        "complete",
        "cached",
        "already-in-progress",
        "already-in-progress-hash",
        "cache-mismatch",
        "soft-exhausted",
        "cancel-requested",
        "no-active-job",
        // error/guard codes
        "request-failed",
        "concurrency-closed",
        "downstream-http-status",
        "upstream-changed",
        "resume-no-content-range",
        "resume-http-status",
        "resume-failed",
        "resync-failed",
        "quota-exceeded",
        "size-limit",
        "idle-timeout",
        "hard-exhausted",
        "io-read",
        "io-write",
        "flush-failed",
        "mkdir-failed",
        "open-failed",
        "create-failed",
        "verify-open-failed",
        "verify-read-failed",
        "checksum-mismatch",
        "size-mismatch",
        "finalize-failed",
        "admission-denied",
        "disk-insufficient",
        "size-limit-stream",
        "disk-insufficient-stream",
    ];
    /// Borrow the canonical list of allowed status values.
    #[inline]
    pub fn progress_statuses() -> &'static [&'static str] {
        &Self::PROGRESS_STATUS
    }
    /// Borrow the canonical list of allowed code values.
    #[inline]
    pub fn progress_codes() -> &'static [&'static str] {
        &Self::PROGRESS_CODES
    }
    // Track desired concurrency at runtime (defaults from env on first access)
    fn concurrency_cfg_cell() -> &'static RwLock<usize> {
        static CFG: OnceCell<RwLock<usize>> = OnceCell::new();
        CFG.get_or_init(|| RwLock::new(Self::max_concurrency()))
    }
    // Held permits to simulate shrinking concurrency (cannot reduce Semaphore capacity directly)
    fn held_permits_cell() -> &'static RwLock<Vec<tokio::sync::OwnedSemaphorePermit>> {
        static HELD: OnceCell<RwLock<Vec<tokio::sync::OwnedSemaphorePermit>>> = OnceCell::new();
        HELD.get_or_init(|| RwLock::new(Vec::new()))
    }
    // Optional hard upper bound from env (ARW_MODELS_MAX_CONC_HARD)
    fn hard_max_concurrency() -> Option<usize> {
        std::env::var("ARW_MODELS_MAX_CONC_HARD")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v >= 1)
    }
    // Track last known pending_shrink from non-blocking shrink operations
    fn pending_shrink_cell() -> &'static RwLock<usize> {
        static PENDING: OnceCell<RwLock<usize>> = OnceCell::new();
        PENDING.get_or_init(|| RwLock::new(0usize))
    }
    /// Lightweight snapshot of downloader state for admin/ops.
    ///
    /// Includes active `model_id`→`job_id` pairs, in‑flight sha256 hashes, and a
    /// small concurrency summary (configured, available, held permits).
    pub async fn jobs_status(&self) -> Value {
        let active_map = Self::active_jobs_cell().read().await.clone();
        let mut active = Vec::with_capacity(active_map.len());
        for (k, v) in active_map.into_iter() {
            active.push(json!({"model_id": k, "job_id": v}));
        }
        let inflight: Vec<String> = Self::inflight_hash_cell()
            .read()
            .await
            .iter()
            .cloned()
            .collect();
        let desired = *Self::concurrency_cfg_cell().read().await as u64;
        let held_cnt = Self::held_permits_cell().read().await.len() as u64;
        let pend = *Self::pending_shrink_cell().read().await as u64;
        let conc = json!({
            "configured_max": desired,
            "available_permits": Self::concurrency_sem().available_permits() as u64,
            "held_permits": held_cnt,
            "pending_shrink": if pend == 0 { Value::Null } else { Value::from(pend) },
        });
        json!({
            "active": active,
            "inflight_hashes": inflight,
            "concurrency": conc,
        })
    }
    /// Read current concurrency settings and limits.
    ///
    /// Returns `configured_max`, `available_permits`, `held_permits`, and optional
    /// `hard_cap` from `ARW_MODELS_MAX_CONC_HARD`.
    pub async fn concurrency_get(&self) -> Value {
        let desired = *Self::concurrency_cfg_cell().read().await as u64;
        let held_cnt = Self::held_permits_cell().read().await.len() as u64;
        let avail = Self::concurrency_sem().available_permits() as u64;
        let hard = Self::hard_max_concurrency().map(|v| v as u64);
        let pend = *Self::pending_shrink_cell().read().await as u64;
        json!({
            "configured_max": desired,
            "available_permits": avail,
            "held_permits": held_cnt,
            "hard_cap": hard,
            "pending_shrink": if pend == 0 { Value::Null } else { Value::from(pend) },
        })
    }
    /// Change the effective max concurrency at runtime.
    ///
    /// Increasing uses `add_permits` and/or releases held permits. Decreasing
    /// acquires and holds permits to reduce availability. When `block` is
    /// false, shrinking is opportunistic and reports `pending_shrink`.
    pub async fn concurrency_set(
        &self,
        state: &AppState,
        new_max: usize,
        block: bool,
    ) -> Result<serde_json::Value, String> {
        let sem = Self::concurrency_sem().clone();
        let mut cfg = Self::concurrency_cfg_cell().write().await;
        let mut held = Self::held_permits_cell().write().await;
        let old = *cfg;
        let target = new_max.max(1);
        if let Some(hard) = Self::hard_max_concurrency() {
            if target > hard {
                return Err(format!(
                    "requested max {} exceeds hard cap {} (ARW_MODELS_MAX_CONC_HARD)",
                    target, hard
                ));
            }
        }
        if target == old {
            return Ok(json!({"old": old, "new": old, "changed": false}));
        }
        let mut held_released = 0usize;
        let mut held_acquired = 0usize;
        let mut pending_shrink = 0usize;
        if target > old {
            let mut need = target - old;
            // First, release held permits if any (fast path to grow)
            while need > 0 {
                if let Some(_p) = held.pop() {
                    // dropping releases one permit
                    need -= 1;
                    held_released += 1;
                } else {
                    break;
                }
            }
            if need > 0 {
                sem.add_permits(need);
            }
        } else {
            let shrink = old - target;
            if block {
                // Acquire and hold 'shrink' permits (wait until available)
                for _ in 0..shrink {
                    match sem.clone().acquire_owned().await {
                        Ok(p) => {
                            held.push(p);
                            held_acquired += 1;
                        }
                        Err(_) => return Err("concurrency semaphore closed".into()),
                    }
                }
                // no pending shrink under blocking path
                *Self::pending_shrink_cell().write().await = 0;
            } else {
                // Non-blocking shrink: grab as many permits as available and report pending
                for _ in 0..shrink {
                    match sem.clone().try_acquire_owned() {
                        Ok(p) => {
                            held.push(p);
                            held_acquired += 1;
                        }
                        Err(_) => {
                            pending_shrink = shrink - held_acquired;
                            break;
                        }
                    }
                }
                *Self::pending_shrink_cell().write().await = pending_shrink;
            }
        }
        *cfg = target;
        let payload = json!({
            "old": old,
            "new": target,
            "changed": true,
            "block": block,
            "held_released": held_released,
            "held_acquired": held_acquired,
            "pending_shrink": pending_shrink,
            "available_permits": sem.available_permits(),
        });
        // Publish and audit change (best-effort)
        state.bus.publish(TOPIC_CONCURRENCY_CHANGED, &payload);
        crate::ext::io::audit_event("models.concurrency.set", &payload).await;
        Ok(payload)
    }
    pub fn new() -> Self {
        Self
    }

    /// Lightweight download metrics for UI and operations.
    ///
    /// Returns counters merged with an optional `ewma_mbps` throughput estimate.
    pub async fn downloads_metrics(&self) -> Value {
        let ewma = Self::load_ewma_mbps().await;
        let counters = models_metrics_value();
        let mut obj = serde_json::Map::new();
        obj.insert(
            "ewma_mbps".into(),
            ewma.map(Value::from).unwrap_or(Value::Null),
        );
        // Merge counters for convenience (non-breaking: adds fields)
        if let Value::Object(map) = counters {
            for (k, v) in map.into_iter() {
                obj.insert(k, v);
            }
        }
        Value::Object(obj)
    }

    /// Compose a consistent models summary Value used by UI and API.
    /// Shape: { items: [...], default: string, concurrency: {...}, metrics: {...} }
    pub async fn summary_value(&self) -> Value {
        use tokio::join;
        let models_fut = async { crate::ext::models().read().await.clone() };
        let default_fut = async { crate::ext::default_model().read().await.clone() };
        let conc_fut = async { self.concurrency_get().await };
        let metrics_fut = async { self.downloads_metrics().await };
        let (items, default, concurrency, metrics) =
            join!(models_fut, default_fut, conc_fut, metrics_fut);
        json!({
            "items": items,
            "default": default,
            "concurrency": concurrency,
            "metrics": metrics,
        })
    }

    // Redact sensitive parts of a URL for logs/manifests (drop userinfo, query and fragment).
    fn redact_url_for_logs(u: &str) -> String {
        if let Ok(mut url) = reqwest::Url::parse(u) {
            // Strip potential credentials
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        } else {
            let no_frag = u.split('#').next().unwrap_or(u);
            no_frag.split('?').next().unwrap_or(no_frag).to_string()
        }
    }

    // Normalize a path string for cross-OS comparisons by converting through PathBuf
    // and, if possible, also adding its canonical form. Returns primary normalized form
    // and optionally a canonicalized variant when the file exists.
    async fn normalize_path_str(s: &str) -> (String, Option<String>) {
        use tokio::fs as afs;
        let pb = std::path::PathBuf::from(s);
        let primary = pb.to_string_lossy().to_string();
        match afs::canonicalize(&pb).await {
            Ok(c) => (primary, Some(c.to_string_lossy().to_string())),
            Err(_) => (primary, None),
        }
    }

    // Global concurrency limiter for downloads (permits per concurrent job).
    fn max_concurrency() -> usize {
        Self::env_usize("ARW_MODELS_MAX_CONC", 2).max(1)
    }
    fn concurrency_sem() -> &'static std::sync::Arc<Semaphore> {
        static SEM: OnceCell<std::sync::Arc<Semaphore>> = OnceCell::new();
        SEM.get_or_init(|| {
            let cap = Self::max_concurrency();
            std::sync::Arc::new(Semaphore::new(cap))
        })
    }

    // Shared HTTP client with connection pooling and stable UA.
    fn http_client() -> &'static reqwest::Client {
        static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
        CLIENT.get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .tcp_keepalive(Self::http_keepalive())
                .pool_idle_timeout(Self::http_pool_idle_timeout())
                .pool_max_idle_per_host(Self::http_pool_max_idle_per_host())
                .user_agent(format!(
                    "arw-svc/{} (+https://github.com/t3hw00t/ARW)",
                    env!("CARGO_PKG_VERSION")
                ))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
    }

    // HTTP client tuning (env-overridable)
    // ARW_DL_HTTP_KEEPALIVE_SECS: u64 seconds; 0 disables explicit keepalive (use OS default). Default 60.
    fn http_keepalive() -> Option<std::time::Duration> {
        use std::time::Duration;
        static VAL: OnceCell<Option<Duration>> = OnceCell::new();
        *VAL.get_or_init(|| {
            let secs = std::env::var("ARW_DL_HTTP_KEEPALIVE_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            if secs == 0 {
                None
            } else {
                Some(Duration::from_secs(secs))
            }
        })
    }
    // ARW_DL_HTTP_POOL_IDLE_SECS: u64 seconds; 0 disables explicit idle timeout. Default 90.
    fn http_pool_idle_timeout() -> Option<std::time::Duration> {
        use std::time::Duration;
        static VAL: OnceCell<Option<Duration>> = OnceCell::new();
        *VAL.get_or_init(|| {
            let secs = std::env::var("ARW_DL_HTTP_POOL_IDLE_SECS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(90);
            if secs == 0 {
                None
            } else {
                Some(Duration::from_secs(secs))
            }
        })
    }
    // ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST: usize; pool slots per host. Default 8, minimum 1.
    fn http_pool_max_idle_per_host() -> usize {
        static VAL: OnceCell<usize> = OnceCell::new();
        *VAL.get_or_init(|| {
            std::env::var("ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&n| n >= 1)
                .unwrap_or(8)
        })
    }

    // Track in-flight downloads by sha256 to avoid duplicate concurrent fetches
    fn inflight_hash_cell() -> &'static RwLock<HashSet<String>> {
        static INFLIGHT: OnceCell<RwLock<HashSet<String>>> = OnceCell::new();
        INFLIGHT.get_or_init(|| RwLock::new(HashSet::new()))
    }
    // (removed: previously exposed non-atomic contains/add helpers)
    async fn inflight_remove(hash: &str) {
        Self::inflight_hash_cell().write().await.remove(hash);
    }

    // Atomically try to add an in-flight hash; returns true if inserted, false if already present
    async fn inflight_try_add(hash: &str) -> bool {
        let mut w = Self::inflight_hash_cell().write().await;
        if w.contains(hash) {
            false
        } else {
            w.insert(hash.to_string());
            true
        }
    }

    // Whether to include budget snapshot in progress events (opt-in for compatibility).
    fn progress_include_budget() -> bool {
        static FLAG: OnceCell<bool> = OnceCell::new();
        *FLAG.get_or_init(|| {
            matches!(
                std::env::var("ARW_DL_PROGRESS_INCLUDE_BUDGET")
                    .ok()
                    .as_deref(),
                Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
            )
        })
    }

    // Whether to include disk stats in progress events (opt-in for compatibility).
    fn progress_include_disk() -> bool {
        static FLAG: OnceCell<bool> = OnceCell::new();
        *FLAG.get_or_init(|| {
            matches!(
                std::env::var("ARW_DL_PROGRESS_INCLUDE_DISK")
                    .ok()
                    .as_deref(),
                Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
            )
        })
    }

    // Append an egress ledger entry and publish an event. Merges `extra` fields if provided.
    async fn append_egress_ledger(bus: &arw_events::Bus, e: EgressLedgerEntry) {
        let mut entry = json!({
            "decision": e.decision,
            "reason_code": e.reason_code,
            "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
            "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
            "episode_id": null,
            "corr_id": e.corr_id,
            "node_id": null,
            "tool_id": "models.download",
            "dest": {"host": e.dest_host, "port": e.dest_port as u64, "protocol": e.dest_proto},
            "bytes_out": 0u64,
            "bytes_in": e.bytes_in,
            "duration_ms": e.duration_ms,
        });
        if let (Value::Object(ref mut base), Some(Value::Object(extra_map))) = (&mut entry, e.extra)
        {
            for (k, v) in extra_map.into_iter() {
                base.insert(k, v);
            }
        }
        crate::ext::corr::ensure_corr(&mut entry);
        crate::ext::io::egress_ledger_append(&entry).await;
        bus.publish(TOPIC_EGRESS_LEDGER_APPENDED, &entry);
    }

    fn idle_timeout_duration() -> Option<std::time::Duration> {
        // Safety net when hard budget is 0 to avoid hung downloads.
        // Set ARW_DL_IDLE_TIMEOUT_SECS=0 to disable (no idle timeout).
        static DUR: OnceCell<Option<std::time::Duration>> = OnceCell::new();
        *DUR.get_or_init(|| {
            let secs = Self::env_u64("ARW_DL_IDLE_TIMEOUT_SECS", 300);
            if secs == 0 {
                None
            } else {
                Some(std::time::Duration::from_secs(secs))
            }
        })
    }

    fn disk_reserve_bytes() -> u64 {
        static BYTES: OnceCell<u64> = OnceCell::new();
        *BYTES.get_or_init(|| {
            Self::env_u64("ARW_MODELS_DISK_RESERVE_MB", 256).saturating_mul(1024 * 1024)
        })
    }

    fn ewma_alpha() -> f64 {
        static ALPHA: OnceCell<f64> = OnceCell::new();
        *ALPHA.get_or_init(|| Self::env_f64("ARW_DL_EWMA_ALPHA", 0.3).clamp(0.000_001, 0.999_999))
    }

    async fn load_ewma_mbps() -> Option<f64> {
        let p = crate::ext::paths::downloads_metrics_path();
        match crate::ext::io::load_json_file_async(&p).await {
            Some(v) => v.get("ewma_mbps").and_then(|x| x.as_f64()),
            None => None,
        }
    }

    // Write resume validators (ETag/Last-Modified) to sidecar for future resumption.
    async fn save_resume_validators(
        meta_path: &std::path::Path,
        headers: &reqwest::header::HeaderMap,
    ) {
        use tokio::fs as afs;
        let etag_val = headers
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let lm_val = headers
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if etag_val.is_none() && lm_val.is_none() {
            return;
        }
        let mut obj = serde_json::Map::new();
        if let Some(e) = &etag_val {
            obj.insert("etag".into(), Value::String(e.clone()));
        }
        if let Some(lm) = &lm_val {
            obj.insert("last_modified".into(), Value::String(lm.clone()));
        }
        let _ = afs::write(
            meta_path,
            serde_json::to_vec(&Value::Object(obj)).unwrap_or_default(),
        )
        .await;
    }

    // Load resume validators from sidecar, preferring ETag over Last-Modified.
    // Returns a string suitable for the If-Range header when present.
    async fn load_resume_ifrange(meta_path: &std::path::Path) -> Option<String> {
        use tokio::fs as afs;
        if let Ok(bytes) = afs::read(meta_path).await {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(etag) = val.get("etag").and_then(|v| v.as_str()) {
                    return Some(etag.to_string());
                }
                if let Some(lm) = val.get("last_modified").and_then(|v| v.as_str()) {
                    return Some(lm.to_string());
                }
            }
        }
        None
    }

    async fn update_ewma_mbps(sample_mbps: f64) {
        if !sample_mbps.is_finite() || sample_mbps <= 0.0 {
            return;
        }
        let p = crate::ext::paths::downloads_metrics_path();
        let prev = Self::load_ewma_mbps().await.unwrap_or(sample_mbps);
        let a = Self::ewma_alpha();
        let ewma = a * sample_mbps + (1.0 - a) * prev;
        let _ = crate::ext::io::save_json_file_async(&p, &json!({"ewma_mbps": ewma})).await;
    }

    fn max_download_bytes() -> u64 {
        static BYTES: OnceCell<u64> = OnceCell::new();
        *BYTES.get_or_init(|| Self::env_u64("ARW_MODELS_MAX_MB", 4096).saturating_mul(1024 * 1024))
    }

    fn models_quota_bytes() -> Option<u64> {
        std::env::var("ARW_MODELS_QUOTA_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1024 * 1024))
    }

    // Compute current CAS usage (bytes, files) in models/by-hash
    async fn cas_usage_totals() -> (u64, u64) {
        use tokio::fs as afs;
        let dir = crate::ext::paths::state_dir()
            .join("models")
            .join("by-hash");
        let mut bytes: u64 = 0;
        let mut files: u64 = 0;
        if let Ok(mut rd) = afs::read_dir(&dir).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                if let Ok(md) = ent.metadata().await {
                    if md.is_file() {
                        files = files.saturating_add(1);
                        bytes = bytes.saturating_add(md.len());
                    }
                }
            }
        }
        (bytes, files)
    }

    // Public snapshot: CAS quota/usage quick view for UI/ops
    /// Snapshot of CAS usage and quota status.
    ///
    /// Reports directory, file count, used bytes/MB, and whether
    /// `ARW_MODELS_QUOTA_MB` is exceeded.
    pub async fn quota_status(&self) -> Value {
        let dir = crate::ext::paths::state_dir()
            .join("models")
            .join("by-hash");
        let (used_bytes, files) = Self::cas_usage_totals().await;
        let quota = Self::models_quota_bytes();
        let max_bytes = Self::max_download_bytes();
        let reserve = Self::disk_reserve_bytes();
        json!({
            "dir": dir.to_string_lossy(),
            "files": files,
            "used_bytes": used_bytes,
            "used_mb": used_bytes / (1024u64 * 1024u64),
            "quota_bytes": quota,
            "quota_mb": quota.map(|b| b / (1024u64*1024u64)),
            "over_quota": quota.map(|q| used_bytes > q).unwrap_or(false),
            "max_download_bytes": max_bytes,
            "max_download_mb": max_bytes / (1024u64 * 1024u64),
            "disk_reserve_bytes": reserve,
            "disk_reserve_mb": reserve / (1024u64 * 1024u64),
        })
    }

    // Produce a cross-platform safe filename (Windows/macOS/Linux).
    // - Replaces reserved characters with '_'
    // - Trims trailing dots/spaces (Windows quirk)
    // - Avoids reserved device names (CON, PRN, AUX, NUL, COM1..9, LPT1..9)
    // - Caps length to a reasonable limit while preserving extension
    fn sanitize_file_name(input: &str) -> String {
        #[inline]
        fn is_allowed(c: char) -> bool {
            // Allow common safe set; disallow control chars and reserved ones.
            matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ' ')
        }
        // Linear pass to replace disallowed with a single '_' and collapse repeats on the fly.
        let mut out = String::with_capacity(input.len());
        let mut last_was_us = false;
        for ch in input.chars() {
            if is_allowed(ch) {
                out.push(ch);
                last_was_us = false;
            } else if !last_was_us {
                out.push('_');
                last_was_us = true;
            }
        }
        // Trim spaces/dots from ends (Windows doesn't like trailing dot/space in file names).
        let s = out.trim_matches(|c: char| c == ' ' || c == '.').to_string();
        let mut s = if s.is_empty() { "file".to_string() } else { s };
        // Avoid reserved Windows device names (case-insensitive), with or without extensions.
        // Windows forbids names like "con" and also "con.txt". If the base (stem) is reserved,
        // append an underscore before the extension to keep it distinct and safe.
        let reserved = [
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
            "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ];
        let mut needs_suffix = false;
        let lower_full = s.to_ascii_lowercase();
        if reserved.iter().any(|&r| r == lower_full) {
            needs_suffix = true;
        } else if let Some(dot) = s.rfind('.') {
            let base_lower = s[..dot].to_ascii_lowercase();
            if reserved.iter().any(|&r| r == base_lower) {
                // Insert underscore before extension
                s.insert(dot, '_');
            }
        }
        if needs_suffix {
            s.push('_');
        }
        // Enforce a length cap (keep extension when present).
        const MAX_LEN: usize = 120; // conservative to fit various filesystems
        if s.len() > MAX_LEN {
            if let Some(dot) = s.rfind('.') {
                let (base, ext_with_dot) = s.split_at(dot);
                let ext_no_dot = &ext_with_dot[1..];
                // If the extension (without dot) is too long to fit,
                // keep as much base as possible, then '.' and a truncated extension.
                if 1 + ext_no_dot.chars().count() >= MAX_LEN {
                    let base_keep = base.chars().count().min(MAX_LEN.saturating_sub(1));
                    let ext_keep = MAX_LEN.saturating_sub(base_keep + 1);
                    let base_trunc = base.chars().take(base_keep).collect::<String>();
                    let ext_trunc = ext_no_dot.chars().take(ext_keep).collect::<String>();
                    s = format!("{}.{}", base_trunc, ext_trunc);
                } else {
                    let keep_base = MAX_LEN.saturating_sub(ext_with_dot.len());
                    let base_trunc = base.chars().take(keep_base).collect::<String>();
                    s = format!("{}{}", base_trunc, ext_with_dot);
                }
                if s.len() > MAX_LEN {
                    s = s.chars().take(MAX_LEN).collect();
                }
            } else {
                s = s.chars().take(MAX_LEN).collect();
            }
        }
        s
    }

    // Small parser for Content-Disposition filenames.
    // Prefers RFC 5987 filename* when present (percent-decodes), otherwise falls back to filename=.
    fn filename_from_content_disposition(v: &str) -> Option<String> {
        #[inline]
        fn percent_decode(s: &str) -> String {
            let bytes = s.as_bytes();
            let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' && i + 2 < bytes.len() {
                    let h1 = bytes[i + 1];
                    let h2 = bytes[i + 2];
                    let val = |c| match c {
                        b'0'..=b'9' => c - b'0',
                        b'a'..=b'f' => c - b'a' + 10,
                        b'A'..=b'F' => c - b'A' + 10,
                        _ => 255,
                    };
                    let hi = val(h1);
                    let lo = val(h2);
                    if hi != 255 && lo != 255 {
                        out.push((hi << 4) | lo);
                        i += 3;
                        continue;
                    }
                }
                out.push(bytes[i]);
                i += 1;
            }
            String::from_utf8_lossy(&out).into_owned()
        }

        let mut filename_star: Option<String> = None;
        let mut filename_plain: Option<String> = None;
        for part in v.split(';') {
            let p = part.trim();
            let pl = p.to_ascii_lowercase();
            if pl.starts_with("filename*=") {
                // filename*=<charset>'<lang>'<pct-encoded>
                let eq = p.find('=');
                let mut raw = if let Some(i) = eq { &p[i + 1..] } else { "" };
                raw = raw.trim();
                let raw = raw
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(raw);
                // split on single quotes; expect at least two quotes
                let mut iter = raw.splitn(3, '\'');
                let charset = iter.next().unwrap_or("");
                let _lang = iter.next().unwrap_or("");
                let rest = iter.next().unwrap_or("");
                if !rest.is_empty() {
                    let name = percent_decode(rest);
                    // Only honor utf-8 if declared, else still return decoded best-effort.
                    if !charset.is_empty() {
                        if charset.eq_ignore_ascii_case("utf-8") {
                            filename_star = Some(name);
                        } else {
                            // best effort return
                            filename_star = Some(name);
                        }
                    } else {
                        filename_star = Some(name);
                    }
                }
            } else if pl.starts_with("filename=") {
                let eq = p.find('=');
                let raw = if let Some(i) = eq { &p[i + 1..] } else { "" };
                let raw = raw.trim();
                let name = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    raw
                };
                if !name.is_empty() {
                    filename_plain = Some(name.to_string());
                }
            }
        }
        filename_star.or(filename_plain)
    }

    // Locate an existing CAS blob by sha256; returns (path, file_name)
    async fn find_cas_by_hash(sha256: &str) -> Option<(std::path::PathBuf, String)> {
        let dir = crate::ext::paths::state_dir()
            .join("models")
            .join("by-hash");
        // Fast path: exact file name without extension
        let exact = dir.join(sha256);
        if let Ok(md) = tokio::fs::metadata(&exact).await {
            if md.is_file() {
                return Some((exact, sha256.to_string()));
            }
        }
        if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                let name = ent.file_name().to_string_lossy().to_string();
                if name == sha256 || name.starts_with(&format!("{}.", sha256)) {
                    return Some((ent.path(), name));
                }
            }
        }
        None
    }

    /// Return the current models list (in-memory state).
    pub async fn list(&self) -> Vec<Value> {
        crate::ext::models().read().await.clone()
    }

    // Run a single CAS GC sweep. Deletes unreferenced blobs older than ttl_days.
    // Publishes a compact summary event on success.
    /// Run a single CAS GC pass for unreferenced blobs older than `ttl_days`.
    pub async fn cas_gc_once(bus: &arw_events::Bus, ttl_days: u64) {
        use tokio::fs as afs;
        let state_dir = crate::ext::paths::state_dir();
        let models_dir = state_dir.join("models");
        let cas_dir = models_dir.join("by-hash");
        let ttl = std::time::Duration::from_secs(ttl_days.saturating_mul(24 * 3600));
        let now = std::time::SystemTime::now();

        // Collect referenced paths from current models list (normalize for OS differences)
        let mut refs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in crate::ext::models().read().await.iter() {
            if let Some(p) = m.get("path").and_then(|v| v.as_str()) {
                let (norm, canon) = Self::normalize_path_str(p).await;
                refs.insert(norm);
                if let Some(c) = canon {
                    refs.insert(c);
                }
            }
        }
        // Collect referenced paths from manifests under models/*.json (normalize as well)
        if let Ok(mut rd) = afs::read_dir(&models_dir).await {
            while let Ok(Some(ent)) = rd.next_entry().await {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()).unwrap_or("") != "json" {
                    continue;
                }
                if let Ok(bytes) = afs::read(&p).await {
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        if let Some(s) = v.get("path").and_then(|x| x.as_str()) {
                            let (norm, canon) = Self::normalize_path_str(s).await;
                            refs.insert(norm);
                            if let Some(c) = canon {
                                refs.insert(c);
                            }
                        }
                    }
                }
            }
        }
        let mut scanned: u64 = 0;
        let mut kept: u64 = 0;
        let mut deleted: u64 = 0;
        let mut deleted_bytes: u64 = 0;
        if let Ok(mut rd) = afs::read_dir(&cas_dir).await {
            let mut yield_ctr: u64 = 0;
            while let Ok(Some(ent)) = rd.next_entry().await {
                yield_ctr += 1;
                if yield_ctr % 64 == 0 {
                    tokio::task::yield_now().await;
                }
                let path = ent.path();
                let Ok(md) = ent.metadata().await else {
                    continue;
                };
                if !md.is_file() {
                    continue;
                }
                scanned += 1;
                let path_str = path.to_string_lossy().to_string();
                // Also consider canonicalized form in case refs were canonicalized
                let canon_str = match afs::canonicalize(&path).await {
                    Ok(c) => Some(c.to_string_lossy().to_string()),
                    Err(_) => None,
                };
                if refs.contains(&path_str)
                    || canon_str
                        .as_ref()
                        .map(|s| refs.contains(s))
                        .unwrap_or(false)
                {
                    kept += 1;
                    continue;
                }
                // Age check
                let old_enough = match md.modified() {
                    Ok(m) => now.duration_since(m).unwrap_or_default() >= ttl,
                    Err(_) => false,
                };
                if old_enough {
                    deleted_bytes = deleted_bytes.saturating_add(md.len());
                    let _ = afs::remove_file(&path).await;
                    deleted += 1;
                } else {
                    kept += 1;
                }
            }
        }
        let mut payload = json!({
            "scanned": scanned,
            "kept": kept,
            "deleted": deleted,
            "deleted_bytes": deleted_bytes,
            "ttl_days": ttl_days
        });
        crate::ext::corr::ensure_corr(&mut payload);
        bus.publish(TOPIC_MODELS_CAS_GC, &payload);
    }

    /// Reset the models list to defaults (provider-curated) and publish events/patches.
    pub async fn refresh(&self, state: &AppState) -> Vec<Value> {
        let new = crate::ext::default_models();
        {
            let mut m = crate::ext::models().write().await;
            *m = new.clone();
        }
        let _ = crate::ext::io::save_json_file_async(
            &crate::ext::paths::models_path(),
            &Value::Array(new.clone()),
        )
        .await;
        state
            .bus
            .publish(TOPIC_MODELS_REFRESHED, &json!({"count": new.len()}));
        // Emit patch for the full models state
        publish_models_state_patch(&state.bus).await;
        new
    }

    /// Persist the current models array to `<state>/models/models.json`.
    pub async fn save(&self) -> Result<(), String> {
        let v = crate::ext::models().read().await.clone();
        crate::ext::io::save_json_file_async(&crate::ext::paths::models_path(), &Value::Array(v))
            .await
            .map_err(|e| e.to_string())
    }

    /// Load the models array from `<state>/models/models.json`.
    pub async fn load(&self) -> Result<Vec<Value>, String> {
        match crate::ext::io::load_json_file_async(&crate::ext::paths::models_path())
            .await
            .and_then(|v| v.as_array().cloned())
        {
            Some(arr) => {
                {
                    let mut m = crate::ext::models().write().await;
                    *m = arr.clone();
                }
                Ok(arr)
            }
            None => Err("no models.json".into()),
        }
    }

    /// Add a model id (and optional provider); publishes `models.changed`.
    pub async fn add(&self, state: &AppState, id: String, provider: Option<String>) {
        let mut v = crate::ext::models().write().await;
        if !v
            .iter()
            .any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
        {
            v.push(json!({"id": id, "provider": provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
            state.bus.publish(
                TOPIC_MODELS_CHANGED,
                &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}),
            );
            // audit
            crate::ext::io::audit_event(
                "models.add",
                &json!({"id": v.last().and_then(|m| m.get("id")).cloned() }),
            )
            .await;
            // Emit patch
            publish_models_state_patch(&state.bus).await;
        }
    }

    /// Delete a model id; publishes `models.changed`.
    pub async fn delete(&self, state: &AppState, id: String) {
        let mut v = crate::ext::models().write().await;
        let before = v.len();
        v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&id));
        if v.len() != before {
            state
                .bus
                .publish(TOPIC_MODELS_CHANGED, &json!({"op":"delete","id": id}));
            crate::ext::io::audit_event("models.delete", &json!({"id": id})).await;
            publish_models_state_patch(&state.bus).await;
        }
    }

    /// Return the default model id used for inference.
    pub async fn default_get(&self) -> String {
        crate::ext::default_model().read().await.clone()
    }

    /// Set the default model id; publishes `models.changed`.
    pub async fn default_set(&self, state: &AppState, id: String) -> Result<(), String> {
        {
            let mut d = crate::ext::default_model().write().await;
            *d = id.clone();
        }
        state
            .bus
            .publish(TOPIC_MODELS_CHANGED, &json!({"op":"default","id": id}));
        let res = crate::ext::io::save_json_file_async(
            &crate::ext::paths::models_path(),
            &Value::Array(crate::ext::models().read().await.clone()),
        )
        .await
        .map_err(|e| e.to_string());
        if res.is_ok() {
            crate::ext::io::audit_event("models.default", &json!({"id": id})).await;
            publish_models_state_patch(&state.bus).await;
        }
        res
    }

    // ---- Download worker ----
    fn cancel_cell() -> &'static RwLock<HashSet<String>> {
        static DL_CANCEL: OnceCell<RwLock<HashSet<String>>> = OnceCell::new();
        DL_CANCEL.get_or_init(|| RwLock::new(HashSet::new()))
    }
    async fn is_canceled(job_id: &str) -> bool {
        Self::cancel_cell().read().await.contains(job_id)
    }
    async fn clear_cancel(job_id: &str) {
        Self::cancel_cell().write().await.remove(job_id);
    }
    async fn set_cancel(job_id: &str) {
        Self::cancel_cell().write().await.insert(job_id.to_string());
    }

    // Track active download job per model id (model_id -> job_id)
    fn active_jobs_cell() -> &'static RwLock<HashMap<String, String>> {
        static ACTIVE: OnceCell<RwLock<HashMap<String, String>>> = OnceCell::new();
        ACTIVE.get_or_init(|| RwLock::new(HashMap::new()))
    }
    async fn set_active_job(model_id: &str, job_id: &str) {
        Self::active_jobs_cell()
            .write()
            .await
            .insert(model_id.to_string(), job_id.to_string());
    }
    async fn current_job_id(model_id: &str) -> Option<String> {
        Self::active_jobs_cell().read().await.get(model_id).cloned()
    }
    async fn clear_active_job(model_id: &str) {
        Self::active_jobs_cell().write().await.remove(model_id);
    }

    /// Request cancellation by model id; publishes progress and changed events.
    pub async fn cancel_download(&self, state: &AppState, id: String) {
        // Resolve current job for this model id; if present, cancel that job only
        if let Some(job) = Self::current_job_id(&id).await {
            Self::set_cancel(&job).await;
            let p = json!({"id": id, "status":"cancel-requested"});
            emit_progress(
                &state.bus,
                &id,
                Some("cancel-requested"),
                Some("cancel-requested"),
                None,
                None,
                None,
            );
            crate::ext::io::audit_event("models.download.cancel", &p).await;
            return;
        }
        let p = json!({"id": id, "status":"no-active-job"});
        emit_progress(
            &state.bus,
            &id,
            Some("no-active-job"),
            Some("no-active-job"),
            None,
            None,
            None,
        );
        crate::ext::io::audit_event("models.download.cancel", &p).await;
    }

    /// Start a download with optional budget override.
    ///
    /// Requires `sha256`. Publishes progress events and updates the models list.
    pub async fn download_with_budget(
        &self,
        state: &AppState,
        id_in: String,
        url_in: String,
        provider_in: Option<String>,
        sha256_in: Option<String>,
        budget_override: Option<DownloadBudgetOverride>,
    ) -> Result<(), String> {
        // Validate inputs early to avoid leaving partial state behind on error.
        if !(url_in.starts_with("http://") || url_in.starts_with("https://")) {
            return Err("invalid url scheme".into());
        }
        let expect_sha_pre = sha256_in.clone().map(|s| s.to_lowercase());
        if expect_sha_pre.is_none() {
            return Err("sha256 required".into());
        }
        if let Some(ref sh) = expect_sha_pre {
            let valid = sh.len() == 64 && sh.chars().all(|c| c.is_ascii_hexdigit());
            if !valid {
                return Err("invalid sha256".into());
            }
        }
        // If a job is already active for this model id, treat this request as queued.
        if Self::current_job_id(&id_in).await.is_some() {
            emit_progress(
                &state.bus,
                &id_in,
                Some("queued"),
                Some("already-in-progress"),
                None,
                None,
                None,
            );
            return Ok(());
        }
        // Ensure model exists in list (do not flip to "downloading" yet; defer until admitted)
        let mut already_in_progress = false;
        {
            let mut v = crate::ext::models().write().await;
            if let Some(m) = v
                .iter_mut()
                .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id_in))
            {
                let prev = m.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if prev.eq_ignore_ascii_case("downloading") {
                    already_in_progress = true;
                }
            } else {
                v.push(json!({"id": id_in, "provider": provider_in.clone().unwrap_or("local".into()), "status":"available"}));
            }
        }
        if already_in_progress {
            emit_progress(
                &state.bus,
                &id_in,
                Some("queued"),
                Some("already-in-progress"),
                None,
                None,
                None,
            );
            return Ok(());
        }
        // Inputs validated above; proceed.
        // If we already have the CAS blob, verify and short-circuit to completion
        if let Some(ref sh) = expect_sha_pre {
            if let Some((existing_path, cas_file_name)) = Self::find_cas_by_hash(sh).await {
                // Verify on-disk hash matches expectation before trusting cached blob
                use sha2::Digest as _;
                let mut ok_cached = false;
                if let Ok(mut f) = tokio::fs::File::open(&existing_path).await {
                    let mut hasher = sha2::Sha256::new();
                    let mut buf = vec![0u8; 1024 * 1024];
                    loop {
                        match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
                            Ok(0) => {
                                ok_cached = true;
                                break;
                            }
                            Ok(n) => {
                                use sha2::Digest;
                                hasher.update(&buf[..n]);
                            }
                            Err(_) => {
                                ok_cached = false;
                                break;
                            }
                        }
                    }
                    if ok_cached {
                        let actual = format!("{:x}", hasher.finalize());
                        ok_cached = actual == *sh;
                    }
                }
                if !ok_cached {
                    emit_progress(
                        &state.bus,
                        &id_in,
                        Some("cache-mismatch"),
                        Some("cache-mismatch"),
                        None,
                        None,
                        None,
                    );
                } else {
                    let target_dir = crate::ext::paths::state_dir().join("models");
                    let provider = provider_in.clone().unwrap_or("local".into());
                    let bytes = tokio::fs::metadata(&existing_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);
                    // Write manifest
                    let manifest_path =
                        target_dir.join(format!("{}.json", Self::sanitize_file_name(&id_in)));
                    let mut manifest = serde_json::Map::new();
                    manifest.insert("id".into(), Value::String(id_in.clone()));
                    manifest.insert("file".into(), Value::String(cas_file_name.clone()));
                    manifest.insert(
                        "path".into(),
                        Value::String(existing_path.to_string_lossy().to_string()),
                    );
                    manifest.insert(
                        "url".into(),
                        Value::String(Self::redact_url_for_logs(&url_in)),
                    );
                    manifest.insert("sha256".into(), Value::String(sh.clone()));
                    manifest.insert("cas".into(), Value::String("sha256".into()));
                    manifest.insert(
                        "bytes".into(),
                        Value::Number(serde_json::Number::from(bytes)),
                    );
                    manifest.insert("provider".into(), Value::String(provider.clone()));
                    manifest.insert("verified".into(), Value::Bool(true));
                    let _ = crate::ext::io::save_json_file_async(
                        &manifest_path,
                        &Value::Object(manifest),
                    )
                    .await;
                    // Update models list
                    {
                        let mut v = crate::ext::models().write().await;
                        if let Some(m) = v
                            .iter_mut()
                            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id_in))
                        {
                            if let Some(obj) = m.as_object_mut() {
                                obj.insert("status".into(), Value::String("available".into()));
                                obj.insert(
                                    "path".into(),
                                    Value::String(existing_path.to_string_lossy().to_string()),
                                );
                                obj.insert("sha256".into(), Value::String(sh.clone()));
                                obj.insert("cas".into(), Value::String("sha256".into()));
                                obj.insert("file".into(), Value::String(cas_file_name.clone()));
                                obj.insert(
                                    "bytes".into(),
                                    Value::Number(serde_json::Number::from(bytes)),
                                );
                            }
                        }
                    }
                    let _ = crate::ext::io::save_json_file_async(
                        &crate::ext::paths::models_path(),
                        &Value::Array(crate::ext::models().read().await.clone()),
                    )
                    .await;
                    // Publish completion as cached
                    // Metrics: completed via cache
                    DL_COMPLETED.fetch_add(1, Ordering::Relaxed);
                    DL_COMPLETED_CACHED.fetch_add(1, Ordering::Relaxed);
                    let mut p = json!({"id": id_in, "status":"complete", "file": cas_file_name, "provider": provider, "code":"cached"});
                    crate::ext::corr::ensure_corr(&mut p);
                    state.bus.publish(TOPIC_PROGRESS, &p);
                    metrics_mark_dirty();
                    state.bus.publish(
                        TOPIC_MODELS_CHANGED,
                        &json!({"op":"downloaded","id": id_in}),
                    );
                    // Patch read-model for models (cached completion)
                    publish_models_state_patch(&state.bus).await;
                    return Ok(());
                }
            }
        }
        // Publish start (include initial budget snapshot)
        let mut dl_budget = crate::ext::budget::Budget::for_download();
        if let Some(ov) = budget_override.clone() {
            if let Some(s) = ov.soft_ms {
                dl_budget.soft_ms = s;
            }
            if let Some(h) = ov.hard_ms {
                dl_budget.hard_ms = h;
            }
            if let Some(ref c) = ov.class {
                dl_budget.class = match c.to_ascii_lowercase().as_str() {
                    "interactive" => crate::ext::budget::BudgetClass::Interactive,
                    _ => crate::ext::budget::BudgetClass::Batch,
                };
            }
        }
        let corr_id = Self::emit_download_started(&state.bus, &id_in, &url_in, &dl_budget).await;
        // Spawn worker
        let id = id_in.clone();
        let url = url_in.clone();
        let provider = provider_in.clone().unwrap_or("local".into());
        let expect_sha = expect_sha_pre;
        // Require SHA256 to be provided by callers (fail closed)
        // (validated above)
        let job = uuid::Uuid::new_v4().to_string();
        Self::set_active_job(&id, &job).await;
        let reserve_bytes = Self::disk_reserve_bytes();
        let max_bytes = Self::max_download_bytes();
        let sp = state.clone();
        let budget = dl_budget.clone();

        // Guard to ensure bookkeeping cleanup on every exit path
        struct ActiveJobGuard {
            model_id: String,
            job_id: String,
        }
        impl ActiveJobGuard {
            fn new(model_id: &str, job_id: &str) -> Self {
                Self {
                    model_id: model_id.to_string(),
                    job_id: job_id.to_string(),
                }
            }
        }
        impl Drop for ActiveJobGuard {
            fn drop(&mut self) {
                let mid = self.model_id.clone();
                let jid = self.job_id.clone();
                tokio::spawn(async move {
                    ModelsService::clear_active_job(&mid).await;
                    ModelsService::clear_cancel(&jid).await;
                });
            }
        }
        tokio::spawn(async move {
            let _guard = ActiveJobGuard::new(&id, &job);
            // Acquire concurrency permit (min cap=1). Emit queued if needed.
            let sem = Self::concurrency_sem().clone();
            if sem.available_permits() == 0 {
                emit_progress(
                    &sp.bus,
                    &id,
                    Some("queued"),
                    Some("queued"),
                    if Self::progress_include_budget() {
                        Some(&budget)
                    } else {
                        None
                    },
                    None,
                    Some(&corr_id),
                );
            }
            let _permit = match sem.acquire_owned().await {
                Ok(p) => {
                    Self::on_admitted_set_downloading(&sp, &id, &provider, &budget, &corr_id).await;
                    Some(p)
                }
                Err(_) => {
                    emit_error(
                        &sp.bus,
                        &id,
                        "concurrency-closed",
                        "Download concurrency limiter unavailable",
                        Some(&budget),
                        None,
                        Some(&corr_id),
                    )
                    .await;
                    return;
                }
            };
            // Prepare destination tuple for ledger/events
            let (dest_host, dest_port, dest_proto) = Self::dest_tuple(&url);
            // Emit a pre-offload preview event (best-effort)
            {
                let mut pv = json!({
                    "id": id,
                    "url": Self::redact_url_for_logs(&url),
                    "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                    "provider": provider,
                });
                pv["corr_id"] = Value::String(corr_id.clone());
                crate::ext::corr::ensure_corr(&mut pv);
                sp.bus.publish(TOPIC_EGRESS_PREVIEW, &pv);
            }
            // Guard inflight hash entry
            struct HashGuard(Option<String>);
            impl Drop for HashGuard {
                fn drop(&mut self) {
                    if let Some(h) = self.0.take() {
                        tokio::spawn(async move { ModelsService::inflight_remove(&h).await });
                    }
                }
            }
            use tokio::fs as afs;
            // Sanitize filename and compute initial paths (final name may change via Content-Disposition)
            // Strip query/fragment from the last path segment for a more stable default name.
            let seg = url.rsplit('/').next().unwrap_or(&id);
            let base = seg.split(['?', '#']).next().unwrap_or(seg);
            let safe_name = Self::sanitize_file_name(base);
            let target_dir = crate::ext::paths::state_dir().join("models");
            let mut final_name = safe_name.clone();
            // Use a dedicated tmp directory and prefer sha256-based filenames to avoid collisions across jobs.
            // This also enables resumption across restarts by stable tmp path per hash.
            let tmp_dir = target_dir.join("tmp");
            // tmp is primarily keyed by expected sha256 when available (always required), else fall back to job+name.
            let tmp = if let Some(ref exp) = expect_sha {
                tmp_dir.join(format!("{}.part", exp))
            } else {
                tmp_dir.join(format!("{}-{}.part", job, safe_name))
            };
            let mut target = target_dir.join(&final_name);
            // sidecar metadata path for resume validation
            let meta_path = tmp.with_extension("part.meta");
            if let Err(e) = afs::create_dir_all(&target_dir).await {
                emit_error(
                    &sp.bus,
                    &id,
                    "mkdir-failed",
                    &format!("Failed to create directory: {}", e),
                    Some(&budget),
                    None,
                    Some(&corr_id),
                )
                .await;
                return;
            }
            // Ensure tmp directory exists as well
            if let Err(e) = afs::create_dir_all(&tmp_dir).await {
                emit_error(
                    &sp.bus,
                    &id,
                    "mkdir-failed",
                    &format!("Failed to create directory: {}", e),
                    Some(&budget),
                    None,
                    Some(&corr_id),
                )
                .await;
                return;
            }
            let client = Self::http_client().clone();
            let mut resume_from: u64 = 0;
            if let Ok(md) = afs::metadata(&tmp).await {
                resume_from = md.len();
            }
            // Mark inflight by hash (if any)
            let _hash_guard = if let Some(ref sh) = expect_sha {
                if !ModelsService::inflight_try_add(sh).await {
                    // Another job is already fetching this hash; inform and exit
                    emit_progress(
                        &sp.bus,
                        &id,
                        Some("queued"),
                        Some("already-in-progress-hash"),
                        if Self::progress_include_budget() {
                            Some(&budget)
                        } else {
                            None
                        },
                        None,
                        Some(&corr_id),
                    );
                    return;
                }
                HashGuard(Some(sh.clone()))
            } else {
                HashGuard(None)
            };
            // resume_from is set from existing .part size when present
            // Initial send with small, budget-aware retry/backoff
            let max_attempts: u32 = std::env::var("ARW_DL_SEND_RETRIES")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(2);
            let mut attempt: u32 = 0;
            // Optional preflight HEAD to check size and validators
            if std::env::var("ARW_DL_PREFLIGHT").ok().as_deref() == Some("1") {
                let mut rq = client.head(&url);
                rq = budget.apply_to_request(rq);
                if budget.hard_ms == 0 {
                    if let Some(d) = Self::idle_timeout_duration() {
                        rq = rq.timeout(d);
                    }
                }
                if let Ok(head) = rq.send().await {
                    if let Some(total) = head.content_length() {
                        // Quota check: CAS dir size + total must not exceed quota
                        if let Some(quota) = Self::models_quota_bytes() {
                            // compute CAS total
                            let (cas_total, _files) = Self::cas_usage_totals().await;
                            if cas_total.saturating_add(total) > quota {
                                let extra =
                                    json!({"quota": quota, "cas_total": cas_total, "need": total});
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "quota-exceeded",
                                    "Models quota exceeded",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                // Ledger: deny
                                Self::append_egress_ledger(
                                    &sp.bus,
                                    EgressLedgerEntry::deny("quota-exceeded")
                                        .dest(dest_host.clone(), dest_port, dest_proto.clone())
                                        .corr_id(corr_id.clone())
                                        .build(),
                                )
                                .await;
                                return;
                            }
                        }
                        // Also respect max_bytes
                        if Self::max_download_bytes() > 0 && total > Self::max_download_bytes() {
                            let extra =
                                json!({"total": total, "max_bytes": Self::max_download_bytes()});
                            emit_error(
                                &sp.bus,
                                &id,
                                "size-limit",
                                "Size exceeds limit",
                                Some(&budget),
                                Some(extra),
                                Some(&corr_id),
                            )
                            .await;
                            // Ledger: deny
                            Self::append_egress_ledger(
                                &sp.bus,
                                EgressLedgerEntry::deny("size-limit")
                                    .dest(dest_host.clone(), dest_port, dest_proto.clone())
                                    .corr_id(corr_id.clone())
                                    .build(),
                            )
                            .await;
                            return;
                        }
                    }
                    // Save validators if present
                    Self::save_resume_validators(&meta_path, head.headers()).await;
                }
            }

            // Early cancel check before sending the GET request
            if Self::is_canceled(&job).await {
                let _ = afs::remove_file(&tmp).await;
                let _ = afs::remove_file(&meta_path).await;
                emit_progress(
                    &sp.bus,
                    &id,
                    Some("canceled"),
                    Some("canceled-by-user"),
                    if Self::progress_include_budget() {
                        Some(&budget)
                    } else {
                        None
                    },
                    None,
                    Some(&corr_id),
                );
                let p2 = json!({"id": id, "status": "canceled"});
                crate::ext::io::audit_event("models.download.canceled", &p2).await;
                {
                    let mut v = crate::ext::models().write().await;
                    if let Some(m) = v
                        .iter_mut()
                        .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
                    {
                        if let Some(obj) = m.as_object_mut() {
                            obj.insert("status".into(), Value::String("canceled".into()));
                        }
                    }
                    let _ = crate::ext::io::save_json_file_async(
                        &crate::ext::paths::models_path(),
                        &Value::Array(v.clone()),
                    )
                    .await;
                }
                sp.bus
                    .publish(TOPIC_MODELS_CHANGED, &json!({"op":"canceled","id": id}));
                return;
            }

            let resp_result = loop {
                // Build a fresh request each attempt so we don't reuse a moved builder
                let mut rq = client.get(&url);
                // Apply budget headers; do not set whole-request timeout here to avoid killing active streams.
                rq = budget.apply_to_request(rq);
                // Idle timeout is enforced per-chunk below when reading the stream.
                if resume_from > 0 {
                    rq = rq.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
                    // Try If-Range with stored ETag/Last-Modified
                    if let Some(ifr) = Self::load_resume_ifrange(&meta_path).await {
                        rq = rq.header(reqwest::header::IF_RANGE, ifr);
                    }
                }
                match rq.send().await {
                    Ok(r) => break Ok(r),
                    Err(e) => {
                        if budget.hard_exhausted() || attempt >= max_attempts {
                            break Err(e);
                        }
                        // backoff grows with attempts but capped by remaining hard budget
                        let base_ms = 200u64.saturating_mul(1u64 << attempt.min(4));
                        let cap_ms = budget.remaining_hard_ms().saturating_div(4).max(50);
                        let sleep_ms = base_ms.min(cap_ms);
                        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        attempt += 1;
                        continue;
                    }
                }
            };
            let t0 = std::time::Instant::now();
            match resp_result {
                Ok(resp) => {
                    // Delegated streaming path; returns after completion or error
                    Self::stream_download_loop(
                        &sp,
                        &id,
                        &provider,
                        &url,
                        &dest_host,
                        dest_port,
                        &dest_proto,
                        &corr_id,
                        &budget,
                        &client,
                        resp,
                        &mut resume_from,
                        &target_dir,
                        &mut final_name,
                        &mut target,
                        &tmp,
                        &meta_path,
                        &expect_sha,
                        reserve_bytes,
                        max_bytes,
                    )
                    .await;
                    return;
                }

                Err(e) => {
                    emit_error(
                        &sp.bus,
                        &id,
                        "request-failed",
                        &format!("Request failed: {}", e),
                        Some(&budget),
                        None,
                        Some(&corr_id),
                    )
                    .await;
                    // Append failed egress attempt to ledger (best-effort)
                    Self::append_egress_ledger(
                        &sp.bus,
                        EgressLedgerEntry::deny("request-failed")
                            .dest(dest_host.clone(), dest_port, dest_proto.clone())
                            .corr_id(corr_id.clone())
                            .duration_ms(t0.elapsed().as_millis() as u64)
                            .extra(json!({"error": e.to_string()}))
                            .build(),
                    )
                    .await;
                }
            }
        });
        Ok(())
    }
}

impl ModelsService {
    pub async fn download(
        &self,
        state: &AppState,
        id_in: String,
        url_in: String,
        provider_in: Option<String>,
        sha256_in: Option<String>,
    ) -> Result<(), String> {
        self.download_with_budget(state, id_in, url_in, provider_in, sha256_in, None)
            .await
    }
}

// ---- Tests (moved to end to satisfy clippy: items-after-test-module) ----
#[cfg(test)]
mod tests {
    use super::ModelsService;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    #[test]
    fn sanitize_file_name_basic() {
        assert_eq!(ModelsService::sanitize_file_name("a.txt"), "a.txt");
        assert_eq!(ModelsService::sanitize_file_name("..hidden"), "hidden");
        assert_eq!(ModelsService::sanitize_file_name("con"), "con_");
        assert_eq!(ModelsService::sanitize_file_name("AUX"), "AUX_");
        assert_eq!(
            ModelsService::sanitize_file_name("bad:name*?<>|.txt"),
            "bad_name_.txt"
        );
        let long = "x".repeat(300) + ".bin";
        let s = ModelsService::sanitize_file_name(&long);
        assert!(s.len() <= 120);
        assert!(s.ends_with(".bin"));
    }

    #[test]
    fn sanitize_file_name_reserved_with_ext() {
        assert_eq!(ModelsService::sanitize_file_name("con.txt"), "con_.txt");
        assert_eq!(ModelsService::sanitize_file_name("LPT1.md"), "LPT1_.md");
        assert_eq!(ModelsService::sanitize_file_name("aux.JSON"), "aux_.JSON");
        assert_eq!(
            ModelsService::sanitize_file_name("NUL.device"),
            "NUL_.device"
        );
    }

    #[test]
    fn sanitize_file_name_more_cases() {
        assert_eq!(
            ModelsService::sanitize_file_name("bad/../name?.bin"),
            "bad_.._name_.bin"
        );
        assert_eq!(
            ModelsService::sanitize_file_name("a\\b:c*?.txt"),
            "a_b_c_.txt"
        );
        assert_eq!(
            ModelsService::sanitize_file_name(" spaced .txt "),
            "spaced .txt"
        );
        assert_eq!(ModelsService::sanitize_file_name("name."), "name");
        assert_eq!(ModelsService::sanitize_file_name(".."), "file");
    }

    #[test]
    fn sanitize_file_name_long_extension_caps_length() {
        let ext = "a".repeat(300);
        let input = format!("name.{}", ext);
        let s = ModelsService::sanitize_file_name(&input);
        assert!(s.len() <= 120);
        assert!(s.starts_with("name."));
    }

    #[test]
    fn filename_from_content_disposition() {
        assert_eq!(
            ModelsService::filename_from_content_disposition("attachment; filename=foo.bin"),
            Some("foo.bin".into())
        );
        assert_eq!(
            ModelsService::filename_from_content_disposition("inline; filename=\"bar.tar.gz\""),
            Some("bar.tar.gz".into())
        );
        assert_eq!(
            ModelsService::filename_from_content_disposition("attachment; name=data"),
            None
        );
        assert_eq!(
            ModelsService::filename_from_content_disposition(
                "attachment; filename*=UTF-8''na%C3%AFve%20file.txt"
            ),
            Some("naïve file.txt".into())
        );
        assert_eq!(
            ModelsService::filename_from_content_disposition(
                "attachment; filename*=\"UTF-8''foo%20bar.tar.gz\"; filename=ignored.txt"
            ),
            Some("foo bar.tar.gz".into())
        );
    }

    #[test]
    fn redact_url_for_logs_removes_sensitive_parts() {
        // userinfo + query + fragment are removed; scheme/host/port/path kept.
        let u = "https://user:pass@example.com:8443/path/to/file.bin?token=secret#frag";
        let r = ModelsService::redact_url_for_logs(u);
        assert_eq!(r, "https://example.com:8443/path/to/file.bin");

        // Non-parseable URL falls back to stripping query/fragment via string ops
        let u2 = "http://example.com/path?a=1#b";
        let r2 = ModelsService::redact_url_for_logs(u2);
        assert_eq!(r2, "http://example.com/path");
    }

    #[tokio::test]
    async fn normalize_path_str_canonicalizes_existing() {
        use tokio::fs as afs;

        let dir = std::env::temp_dir().join(format!("arw_test_{}", uuid::Uuid::new_v4()));
        let _ = afs::create_dir_all(&dir).await;
        let file = dir.join("sample.txt");
        let _ = afs::write(&file, b"ok").await;
        let (primary, canon) = ModelsService::normalize_path_str(&file.to_string_lossy()).await;
        assert!(!primary.is_empty());
        // Canonicalization should succeed for existing files and be absolute on most platforms
        let canon_path = canon.expect("canonical path present");
        assert!(std::path::Path::new(&canon_path).is_absolute());
        // Cleanup
        let _ = afs::remove_file(&file).await;
        let _ = afs::remove_dir_all(&dir).await;
    }

    #[test]
    fn asyncapi_progress_vocab_matches() {
        use std::collections::BTreeSet;

        // Locate AsyncAPI spec relative to this crate
        let spec_path =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/asyncapi.yaml");
        let data = std::fs::read_to_string(&spec_path).expect("read spec/asyncapi.yaml");
        let yaml: serde_yaml::Value = serde_yaml::from_str(&data).expect("parse asyncapi.yaml");

        let status_yaml = &yaml["components"]["messages"]["ModelsDownloadProgress"]["payload"]
            ["properties"]["status"]["enum"];
        let code_yaml = &yaml["components"]["messages"]["ModelsDownloadProgress"]["payload"]
            ["properties"]["code"]["enum"];
        // If enums are not present in the generated spec, skip this drift check.
        if !status_yaml.is_sequence() || !code_yaml.is_sequence() {
            eprintln!(
                "AsyncAPI enums for ModelsDownloadProgress not present; skipping drift check"
            );
            return;
        }

        let to_set = |node: &serde_yaml::Value| -> BTreeSet<String> {
            if let Some(seq) = node.as_sequence() {
                seq.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                BTreeSet::new()
            }
        };
        let status_set_spec = to_set(status_yaml);
        let code_set_spec = to_set(code_yaml);

        let status_set_code: BTreeSet<String> = super::ModelsService::PROGRESS_STATUS
            .iter()
            .map(|s| s.to_string())
            .collect();
        let code_set_code: BTreeSet<String> = super::ModelsService::PROGRESS_CODES
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Helpful diffs
        let missing_in_spec: Vec<_> = status_set_code
            .difference(&status_set_spec)
            .cloned()
            .collect();
        let extra_in_spec: Vec<_> = status_set_spec
            .difference(&status_set_code)
            .cloned()
            .collect();
        assert!(
            status_set_spec == status_set_code,
            "status enums mismatch: missing_in_spec={:?} extra_in_spec={:?}",
            missing_in_spec,
            extra_in_spec
        );

        let missing_codes_in_spec: Vec<_> =
            code_set_code.difference(&code_set_spec).cloned().collect();
        let extra_codes_in_spec: Vec<_> =
            code_set_spec.difference(&code_set_code).cloned().collect();
        assert!(
            code_set_spec == code_set_code,
            "code enums mismatch: missing_in_spec={:?} extra_in_spec={:?}",
            missing_codes_in_spec,
            extra_codes_in_spec
        );
    }

    #[tokio::test]
    async fn downloads_metrics_shape_includes_counters() {
        let svc = super::ModelsService::new();
        let v = svc.downloads_metrics().await;
        assert!(v.get("ewma_mbps").is_some());
        for k in [
            "started",
            "queued",
            "admitted",
            "resumed",
            "canceled",
            "completed",
            "completed_cached",
            "errors",
            "bytes_total",
        ] {
            assert!(v.get(k).is_some(), "missing key: {}", k);
        }
    }

    #[test]
    fn docs_statuses_match_progress_status_constant() {
        // Locate docs file relative to the crate directory
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../docs/architecture/events_vocabulary.md");
        let ok = std::fs::read_to_string(&p).ok();
        let Some(text) = ok else {
            // Best-effort: if file missing in packaged builds, skip with pass
            eprintln!(
                "warning: docs file not found; skipping status consistency check: {}",
                p.display()
            );
            return;
        };
        // Find the line that lists statuses and extract backtick-enclosed tokens
        let mut doc_vals: BTreeSet<String> = BTreeSet::new();
        for line in text.lines() {
            if line.contains("models.download.progress statuses may include:") {
                let mut s = line;
                while let Some(i) = s.find('`') {
                    let rest = &s[i + 1..];
                    if let Some(end) = rest.find('`') {
                        let token = &rest[..end];
                        if !token.trim().is_empty() {
                            doc_vals.insert(token.trim().to_string());
                        }
                        s = &rest[end + 1..];
                    } else {
                        break;
                    }
                }
                break;
            }
        }
        assert!(
            !doc_vals.is_empty(),
            "failed to parse statuses from docs; check formatting in events_vocabulary.md"
        );
        let code_vals: BTreeSet<String> = ModelsService::progress_statuses()
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Compute differences for clear failures
        let extra_in_docs: Vec<_> = doc_vals.difference(&code_vals).cloned().collect();
        let extra_in_code: Vec<_> = code_vals.difference(&doc_vals).cloned().collect();
        assert!(
            extra_in_docs.is_empty() && extra_in_code.is_empty(),
            "status drift: docs-only={:?}, code-only={:?}",
            extra_in_docs,
            extra_in_code
        );
    }

    #[test]
    fn docs_codes_are_subset_of_progress_codes() {
        // Read docs file with the “Common code values” list
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("../../docs/architecture/events_vocabulary.md");
        let ok = std::fs::read_to_string(&p).ok();
        let Some(text) = ok else {
            eprintln!(
                "warning: docs file not found; skipping code consistency check: {}",
                p.display()
            );
            return;
        };
        // Collect backticked tokens on the "Common `code` values:" line,
        // expanding shorthand like `disk-insufficient(-stream)` into two codes.
        let mut doc_codes: BTreeSet<String> = BTreeSet::new();
        for line in text.lines() {
            if line.contains("Common `code` values:") {
                // Start parsing after the colon to avoid the inline backticked word `code`
                let mut s = match line.find(':') {
                    Some(i) => &line[i + 1..],
                    None => line,
                };
                while let Some(i) = s.find('`') {
                    let rest = &s[i + 1..];
                    if let Some(end) = rest.find('`') {
                        let token = rest[..end].trim();
                        if !token.is_empty() {
                            if let Some(prefix) = token.strip_suffix("(-stream)") {
                                // Expand shorthand “x(-stream)” into x and x-stream
                                doc_codes.insert(prefix.to_string());
                                doc_codes.insert(format!("{}-stream", prefix));
                            } else {
                                doc_codes.insert(token.to_string());
                            }
                        }
                        s = &rest[end + 1..];
                    } else {
                        break;
                    }
                }
                break;
            }
        }
        assert!(
            !doc_codes.is_empty(),
            "failed to parse common code values from docs; check formatting in events_vocabulary.md"
        );
        let code_vals: BTreeSet<String> = ModelsService::progress_codes()
            .iter()
            .map(|s| s.to_string())
            .collect();
        // All documented common codes must be recognized by the service
        let extras: Vec<_> = doc_codes.difference(&code_vals).cloned().collect();
        assert!(
            extras.is_empty(),
            "doc lists unknown codes not in PROGRESS_CODES: {:?}",
            extras
        );
    }
}
// SPDX-License-Identifier: MIT OR Apache-2.0
