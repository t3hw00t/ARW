use serde_json::{json, Value};

use crate::app_state::AppState;
use futures_util::StreamExt;
use once_cell::sync::OnceCell;
use std::collections::{HashMap, HashSet};
use tokio::sync::{RwLock, Semaphore}; // for cancel/active job tracking

#[derive(Default)]
pub struct ModelsService;

#[derive(Clone, Debug)]
pub struct DownloadBudgetOverride {
    pub soft_ms: Option<u64>,
    pub hard_ms: Option<u64>,
    pub class: Option<String>,
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
    let mut obj = serde_json::Map::new();
    obj.insert("id".into(), Value::String(id.to_string()));
    if let Some(s) = status {
        obj.insert("status".into(), Value::String(s.to_string()));
    }
    if let Some(c) = code {
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
    bus.publish("Models.DownloadProgress", &payload);
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
    bus.publish("Models.DownloadProgress", &payload);
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
    bus.publish("Models.Changed", &json!({"op":"error","id": id}));
}

// tests are placed at the end of file to avoid clippy items-after-test-module
#[cfg(test)]
mod tests_early_placeholder {}

impl ModelsService {
    // Lightweight snapshot of downloader state for admin/ops
    pub async fn jobs_status(&self) -> Value {
        let active_map = Self::active_jobs_cell().read().await.clone();
        let mut active = Vec::with_capacity(active_map.len());
        for (k, v) in active_map.into_iter() {
            active.push(json!({"model_id": k, "job_id": v}));
        }
        let inflight: Vec<String> = Self::inflight_hash_cell().read().await.iter().cloned().collect();
        let conc = json!({
            "configured_max": Self::max_concurrency() as u64,
            "available_permits": Self::concurrency_sem().available_permits() as u64,
        });
        json!({
            "active": active,
            "inflight_hashes": inflight,
            "concurrency": conc,
        })
    }
    pub fn new() -> Self {
        Self
    }

    // Expose lightweight downloads metrics for UI/ops.
    // Returns JSON with optional EWMA throughput estimate.
    pub async fn downloads_metrics(&self) -> Value {
        let ewma = Self::load_ewma_mbps().await;
        json!({"ewma_mbps": ewma})
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
        std::env::var("ARW_MODELS_MAX_CONC")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(2)
            .max(1)
    }
    fn concurrency_sem() -> &'static std::sync::Arc<Semaphore> {
        static SEM: OnceCell<std::sync::Arc<Semaphore>> = OnceCell::new();
        SEM.get_or_init(|| {
            let cap = Self::max_concurrency();
            std::sync::Arc::new(Semaphore::new(cap))
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
        matches!(
            std::env::var("ARW_DL_PROGRESS_INCLUDE_BUDGET")
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
        )
    }

    // Whether to include disk stats in progress events (opt-in for compatibility).
    fn progress_include_disk() -> bool {
        matches!(
            std::env::var("ARW_DL_PROGRESS_INCLUDE_DISK")
                .ok()
                .as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
        )
    }

    fn idle_timeout_duration() -> Option<std::time::Duration> {
        // Safety net when hard budget is 0 to avoid hung downloads.
        // Set ARW_DL_IDLE_TIMEOUT_SECS=0 to disable (no idle timeout).
        let secs = std::env::var("ARW_DL_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300);
        if secs == 0 {
            None
        } else {
            Some(std::time::Duration::from_secs(secs))
        }
    }

    fn disk_reserve_bytes() -> u64 {
        std::env::var("ARW_MODELS_DISK_RESERVE_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(256)
            .saturating_mul(1024 * 1024)
    }

    fn ewma_alpha() -> f64 {
        std::env::var("ARW_DL_EWMA_ALPHA")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|a| *a > 0.0 && *a < 1.0)
            .unwrap_or(0.3)
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
        std::env::var("ARW_MODELS_MAX_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(4096)
            .saturating_mul(1024 * 1024)
    }

    fn models_quota_bytes() -> Option<u64> {
        std::env::var("ARW_MODELS_QUOTA_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1024 * 1024))
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

    pub async fn list(&self) -> Vec<Value> {
        crate::ext::models().read().await.clone()
    }

    // Run a single CAS GC sweep. Deletes unreferenced blobs older than ttl_days.
    // Publishes a compact summary event on success.
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
            while let Ok(Some(ent)) = rd.next_entry().await {
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
        bus.publish("Models.CasGc", &payload);
    }

    // Best-effort migration: move legacy model files to CAS layout if sha256 is present.
    pub async fn migrate_legacy_to_cas() {
        use tokio::fs as afs;
        let dir = crate::ext::paths::state_dir().join("models");
        let cas_dir = dir.join("by-hash");
        let _ = afs::create_dir_all(&cas_dir).await;
        let mut changed = false;
        let mut list = crate::ext::models().read().await.clone();
        for m in list.iter_mut() {
            let sh = match m.get("sha256").and_then(|v| v.as_str()) {
                Some(s) if s.len() == 64 => s.to_string(),
                _ => continue,
            };
            let path = match m.get("path").and_then(|v| v.as_str()) {
                Some(p) => std::path::PathBuf::from(p),
                None => continue,
            };
            // Skip if already under by-hash
            if let Some(parent) = path.parent() {
                if parent.ends_with("by-hash") {
                    continue;
                }
            }
            // Move into CAS layout if file exists
            if afs::metadata(&path).await.is_ok() {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());
                let cas_file = match ext {
                    Some(ex) if !ex.is_empty() => format!("{}.{}", sh, ex),
                    _ => sh.clone(),
                };
                let cas_path = cas_dir.join(&cas_file);
                if afs::metadata(&cas_path).await.is_err() {
                    let _ = afs::rename(&path, &cas_path).await;
                }
                // Update model entry
                if let Some(obj) = m.as_object_mut() {
                    obj.insert(
                        "path".into(),
                        Value::String(cas_path.to_string_lossy().to_string()),
                    );
                    obj.insert("cas".into(), Value::String("sha256".into()));
                    obj.insert("file".into(), Value::String(cas_file.clone()));
                }
                // Update manifest if present
                if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
                    let manifest_path = dir.join(format!("{}.json", Self::sanitize_file_name(id)));
                    if let Some(mut man) = crate::ext::io::load_json_file_async(&manifest_path)
                        .await
                        .and_then(|v| v.as_object().cloned())
                    {
                        man.insert(
                            "path".into(),
                            Value::String(cas_path.to_string_lossy().to_string()),
                        );
                        man.insert("file".into(), Value::String(cas_file));
                        man.insert("cas".into(), Value::String("sha256".into()));
                        let _ = afs::write(
                            &manifest_path,
                            serde_json::to_vec(&Value::Object(man)).unwrap_or_default(),
                        )
                        .await;
                    }
                }
                changed = true;
            }
        }
        if changed {
            let _ = crate::ext::io::save_json_file_async(
                &crate::ext::paths::models_path(),
                &Value::Array(list),
            )
            .await;
        }
    }

    pub async fn refresh(&self, state: &AppState) -> Vec<Value> {
        let new = super::super::ext::default_models();
        {
            let mut m = crate::ext::models().write().await;
            *m = new.clone();
        }
        let _ = super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(new.clone()),
        )
        .await;
        state
            .bus
            .publish("Models.Refreshed", &json!({"count": new.len()}));
        new
    }

    pub async fn save(&self) -> Result<(), String> {
        let v = crate::ext::models().read().await.clone();
        super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(v),
        )
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn load(&self) -> Result<Vec<Value>, String> {
        match super::super::ext::io::load_json_file_async(&super::super::ext::paths::models_path())
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

    pub async fn add(&self, state: &AppState, id: String, provider: Option<String>) {
        let mut v = crate::ext::models().write().await;
        if !v
            .iter()
            .any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
        {
            v.push(json!({"id": id, "provider": provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
            state.bus.publish(
                "Models.Changed",
                &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}),
            );
            // audit
            super::super::ext::io::audit_event(
                "models.add",
                &json!({"id": v.last().and_then(|m| m.get("id")).cloned() }),
            )
            .await;
        }
    }

    pub async fn delete(&self, state: &AppState, id: String) {
        let mut v = crate::ext::models().write().await;
        let before = v.len();
        v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&id));
        if v.len() != before {
            state
                .bus
                .publish("Models.Changed", &json!({"op":"delete","id": id}));
            super::super::ext::io::audit_event("models.delete", &json!({"id": id})).await;
        }
    }

    pub async fn default_get(&self) -> String {
        crate::ext::default_model().read().await.clone()
    }

    pub async fn default_set(&self, state: &AppState, id: String) -> Result<(), String> {
        {
            let mut d = crate::ext::default_model().write().await;
            *d = id.clone();
        }
        state
            .bus
            .publish("Models.Changed", &json!({"op":"default","id": id}));
        let res = super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(crate::ext::models().read().await.clone()),
        )
        .await
        .map_err(|e| e.to_string());
        if res.is_ok() {
            super::super::ext::io::audit_event("models.default", &json!({"id": id})).await;
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
            super::super::ext::io::audit_event("models.download.cancel", &p).await;
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
        super::super::ext::io::audit_event("models.download.cancel", &p).await;
    }

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
                        Some("cache_mismatch"),
                        Some("cache_mismatch"),
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
                    let mut p = json!({"id": id_in, "status":"complete", "file": cas_file_name, "provider": provider, "code":"cached"});
                    crate::ext::corr::ensure_corr(&mut p);
                    state.bus.publish("Models.DownloadProgress", &p);
                    state
                        .bus
                        .publish("Models.Changed", &json!({"op":"downloaded","id": id_in}));
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
        let corr_id = {
            // Start event (separate topic) still published as-is for compatibility
            let mut p = json!({"id": id_in, "url": Self::redact_url_for_logs(&url_in), "budget": dl_budget.as_json()});
            let corr = crate::ext::corr::ensure_corr(&mut p);
            state.bus.publish("Models.Download", &p);
            super::super::ext::io::audit_event("models.download", &p).await;
            // Also emit a standardized progress event for downstream listeners
            emit_progress(
                &state.bus,
                &id_in,
                Some("started"),
                Some("started"),
                if Self::progress_include_budget() {
                    Some(&dl_budget)
                } else {
                    None
                },
                None,
                Some(&corr),
            );
            corr
        };
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
        // Always use the enhanced downloader path; legacy flag removed.
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
                    emit_progress(
                        &sp.bus,
                        &id,
                        Some("admitted"),
                        Some("admitted"),
                        if Self::progress_include_budget() {
                            Some(&budget)
                        } else {
                            None
                        },
                        None,
                        Some(&corr_id),
                    );
                    // Now that we've been admitted, reflect 'downloading' in the models list.
                    {
                        let mut v = crate::ext::models().write().await;
                        if let Some(m) = v
                            .iter_mut()
                            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
                        {
                            if let Some(obj) = m.as_object_mut() {
                                obj.insert("status".into(), Value::String("downloading".into()));
                                obj.insert("provider".into(), Value::String(provider.clone()));
                            }
                        } else {
                            v.push(json!({"id": id, "provider": provider.clone(), "status":"downloading"}));
                        }
                    }
                    Some(p)
                }
                Err(_) => {
                    emit_error(
                        &sp.bus,
                        &id,
                        "concurrency_closed",
                        "download concurrency limiter unavailable",
                        Some(&budget),
                        None,
                        Some(&corr_id),
                    )
                    .await;
                    return;
                }
            };
            // Prepare destination tuple for ledger/events
            let (dest_host, dest_port, dest_proto) = {
                if let Ok(u) = reqwest::Url::parse(&url) {
                    let host = u.host_str().unwrap_or("").to_string();
                    let port = u.port().unwrap_or_else(|| match u.scheme() {
                        "https" => 443,
                        "http" => 80,
                        _ => 0,
                    });
                    let proto = u.scheme();
                    (host, port, proto.to_string())
                } else {
                    (String::new(), 0u16, String::from("http"))
                }
            };
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
                sp.bus.publish("Egress.Preview", &pv);
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
            use sha2::Digest;
            use tokio::fs as afs;
            use tokio::io::{AsyncWriteExt, BufWriter};
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
                    "mkdir_failed",
                    &format!("mkdir failed: {}", e),
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
                    "mkdir_failed",
                    &format!("mkdir failed: {}", e),
                    Some(&budget),
                    None,
                    Some(&corr_id),
                )
                .await;
                return;
            }
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .user_agent(format!(
                    "arw-svc/{} (+https://github.com/t3hw00t/arw)",
                    env!("CARGO_PKG_VERSION")
                ))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
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
                            let cas_dir = crate::ext::paths::state_dir()
                                .join("models")
                                .join("by-hash");
                            let mut cas_total: u64 = 0;
                            if let Ok(mut rd) = afs::read_dir(&cas_dir).await {
                                while let Ok(Some(ent)) = rd.next_entry().await {
                                    if let Ok(md) = ent.metadata().await {
                                        if md.is_file() {
                                            cas_total = cas_total.saturating_add(md.len());
                                        }
                                    }
                                }
                            }
                            if cas_total.saturating_add(total) > quota {
                                let extra =
                                    json!({"quota": quota, "cas_total": cas_total, "need": total});
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "quota_exceeded",
                                    "models quota exceeded",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                // Ledger: deny
                                let mut entry = json!({
                                    "decision": "deny",
                                    "reason_code": "quota_exceeded",
                                    "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                    "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                    "episode_id": null,
                                    "corr_id": corr_id,
                                    "node_id": null,
                                    "tool_id": "models.download",
                                    "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                    "bytes_out": 0u64,
                                    "bytes_in": 0u64,
                                    "duration_ms": 0u64,
                                });
                                crate::ext::corr::ensure_corr(&mut entry);
                                super::super::ext::io::egress_ledger_append(&entry).await;
                                sp.bus.publish("Egress.Ledger.Appended", &entry);
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
                                "size_limit",
                                "size exceeds limit",
                                Some(&budget),
                                Some(extra),
                                Some(&corr_id),
                            )
                            .await;
                            // Ledger: deny
                            let mut entry = json!({
                                "decision": "deny",
                                "reason_code": "size_limit",
                                "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                "episode_id": null,
                                "corr_id": corr_id,
                                "node_id": null,
                                "tool_id": "models.download",
                                "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                "bytes_out": 0u64,
                                "bytes_in": 0u64,
                                "duration_ms": 0u64,
                            });
                            crate::ext::corr::ensure_corr(&mut entry);
                            super::super::ext::io::egress_ledger_append(&entry).await;
                            sp.bus.publish("Egress.Ledger.Appended", &entry);
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
                    Some("canceled_by_user"),
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
                    .publish("Models.Changed", &json!({"op":"canceled","id": id}));
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
                    if let Ok(bytes) = afs::read(&meta_path).await {
                        if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                            if let Some(etag) = val.get("etag").and_then(|v| v.as_str()) {
                                rq = rq.header(reqwest::header::IF_RANGE, etag.to_string());
                            } else if let Some(lm) =
                                val.get("last_modified").and_then(|v| v.as_str())
                            {
                                rq = rq.header(reqwest::header::IF_RANGE, lm.to_string());
                            }
                        }
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
                    let total_rem = resp.content_length().unwrap_or(0);
                    let status = resp.status();
                    // Validate acceptable HTTP status for initial or ranged request
                    let ok_status = status.is_success()
                        || (resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT);
                    if !ok_status {
                        let extra = json!({"status": status.as_str()});
                        emit_error(
                            &sp.bus,
                            &id,
                            "downstream_http_status",
                            &format!("http status {}", status.as_u16()),
                            Some(&budget),
                            Some(extra),
                            Some(&corr_id),
                        )
                        .await;
                        return;
                    }
                    // capture validators for future resumes and parse Content-Disposition filename
                    Self::save_resume_validators(&meta_path, resp.headers()).await;
                    if let Some(cd) = resp
                        .headers()
                        .get(reqwest::header::CONTENT_DISPOSITION)
                        .and_then(|v| v.to_str().ok())
                    {
                        if let Some(fname) = Self::filename_from_content_disposition(cd) {
                            let cand = Self::sanitize_file_name(&fname);
                            if !cand.is_empty() {
                                final_name = cand;
                                target = target_dir.join(&final_name);
                            }
                        }
                    }
                    // If resuming, validate Content-Range consistency with our offset.
                    if resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
                        if let Some(cr) = resp
                            .headers()
                            .get(reqwest::header::CONTENT_RANGE)
                            .and_then(|v| v.to_str().ok())
                        {
                            // Expect formats like: bytes <start>-<end>/<total> or bytes <start>-<end>/*
                            let mut ok_range = false;
                            let s = cr.trim();
                            if let Some(rest) = s.strip_prefix("bytes ") {
                                let parts: Vec<&str> = rest.split('/').collect();
                                if let Some(range) = parts.get(0) {
                                    let mut it = range.split('-');
                                    if let (Some(start_s), Some(_end_s)) = (it.next(), it.next()) {
                                        if let Ok(start_off) = start_s.parse::<u64>() {
                                            ok_range = start_off == resume_from;
                                        }
                                    }
                                }
                            }
                            if !ok_range {
                                // Upstream changed or server returned an unexpected range: abort safely.
                                let _ = afs::remove_file(&tmp).await;
                                let _ = afs::remove_file(&meta_path).await;
                                let extra = json!({
                                    "expected_offset": resume_from,
                                    "content_range": s,
                                });
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "upstream_changed",
                                    "content-range does not match resume offset",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                return;
                            }
                        } else {
                            // No Content-Range header on 206 response while resuming. Abort.
                            let _ = afs::remove_file(&tmp).await;
                            let _ = afs::remove_file(&meta_path).await;
                            emit_error(
                                &sp.bus,
                                &id,
                                "resume_no_content_range",
                                "missing Content-Range on partial content",
                                Some(&budget),
                                None,
                                Some(&corr_id),
                            )
                            .await;
                            return;
                        }
                    }
                    let total_all = if resume_from > 0
                        && status == reqwest::StatusCode::PARTIAL_CONTENT
                    {
                        let mut p = json!({"offset": resume_from});
                        if Self::progress_include_disk() {
                            if let (Ok(av), Ok(tt)) = (
                                fs2::available_space(&target_dir),
                                fs2::total_space(&target_dir),
                            ) {
                                p["disk"] =
                                    json!({"available": av, "total": tt, "reserve": reserve_bytes});
                            }
                        }
                        emit_progress(
                            &sp.bus,
                            &id,
                            Some("resumed"),
                            Some("resumed"),
                            if Self::progress_include_budget() {
                                Some(&budget)
                            } else {
                                None
                            },
                            Some(p),
                            Some(&corr_id),
                        );
                        resume_from + total_rem
                    } else {
                        if resume_from > 0 {
                            let _ = afs::remove_file(&tmp).await;
                            resume_from = 0;
                        }
                        total_rem
                    };
                    // Enforce quota if configured (post-GET when total known)
                    if total_all > 0 {
                        if let Some(quota) = Self::models_quota_bytes() {
                            let cas_dir = crate::ext::paths::state_dir()
                                .join("models")
                                .join("by-hash");
                            let mut cas_total: u64 = 0;
                            if let Ok(mut rd) = afs::read_dir(&cas_dir).await {
                                while let Ok(Some(ent)) = rd.next_entry().await {
                                    if let Ok(md) = ent.metadata().await {
                                        if md.is_file() {
                                            cas_total = cas_total.saturating_add(md.len());
                                        }
                                    }
                                }
                            }
                            if cas_total.saturating_add(total_all) > quota {
                                let extra = json!({"quota": quota, "cas_total": cas_total, "need": total_all});
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "quota_exceeded",
                                    "models quota exceeded",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                // Ledger: deny
                                let mut entry = json!({
                                    "decision": "deny",
                                    "reason_code": "quota_exceeded",
                                    "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                    "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                    "episode_id": null,
                                    "corr_id": corr_id,
                                    "node_id": null,
                                    "tool_id": "models.download",
                                    "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                    "bytes_out": 0u64,
                                    "bytes_in": 0u64,
                                    "duration_ms": 0u64,
                                });
                                crate::ext::corr::ensure_corr(&mut entry);
                                super::super::ext::io::egress_ledger_append(&entry).await;
                                sp.bus.publish("Egress.Ledger.Appended", &entry);
                                return;
                            }
                        }
                    }
                    // Hard cap by expected total when known
                    if max_bytes > 0 && total_all > 0 && total_all > max_bytes {
                        let extra = json!({"total": total_all, "max_bytes": max_bytes});
                        emit_error(
                            &sp.bus,
                            &id,
                            "size_limit",
                            "size exceeds limit",
                            Some(&budget),
                            Some(extra),
                            Some(&corr_id),
                        )
                        .await;
                        // Ledger: deny
                        let mut entry = json!({
                            "decision": "deny",
                            "reason_code": "size_limit",
                            "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                            "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                            "episode_id": null,
                            "corr_id": corr_id,
                            "node_id": null,
                            "tool_id": "models.download",
                            "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                            "bytes_out": 0u64,
                            "bytes_in": 0u64,
                            "duration_ms": 0u64,
                        });
                        crate::ext::corr::ensure_corr(&mut entry);
                        super::super::ext::io::egress_ledger_append(&entry).await;
                        sp.bus.publish("Egress.Ledger.Appended", &entry);
                        return;
                    }
                    // Admission: if hard budget configured and we know total bytes, ensure we can plausibly finish
                    if budget.hard_ms > 0 && total_all > 0 {
                        // Minimum expected throughput (MB/s) with EWMA fallback
                        let floor_mbps: f64 = std::env::var("ARW_DL_MIN_MBPS")
                            .ok()
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(2.0);
                        let hist_mbps = Self::load_ewma_mbps().await.unwrap_or(floor_mbps);
                        let mbps: f64 = hist_mbps.max(floor_mbps);
                        let need_bytes = total_all.saturating_sub(resume_from) as f64;
                        let bytes_per_ms = (mbps.max(0.1) * 1024.0 * 1024.0) / 1000.0;
                        let need_ms = (need_bytes / bytes_per_ms).ceil() as u64;
                        let remaining_hard = budget.remaining_hard_ms();
                        if need_ms > remaining_hard.saturating_sub(500) {
                            let extra = json!({"need_ms": need_ms, "remaining_hard_ms": remaining_hard, "mbps": mbps});
                            emit_error(
                                &sp.bus,
                                &id,
                                "admission_denied",
                                "admission_denied: insufficient hard budget",
                                Some(&budget),
                                Some(extra),
                                Some(&corr_id),
                            )
                            .await;
                            // Ledger: deny
                            let mut entry = json!({
                                "decision": "deny",
                                "reason_code": "admission_denied",
                                "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                "episode_id": null,
                                "corr_id": corr_id,
                                "node_id": null,
                                "tool_id": "models.download",
                                "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                "bytes_out": 0u64,
                                "bytes_in": 0u64,
                                "duration_ms": 0u64,
                            });
                            crate::ext::corr::ensure_corr(&mut entry);
                            super::super::ext::io::egress_ledger_append(&entry).await;
                            sp.bus.publish("Egress.Ledger.Appended", &entry);
                            return;
                        }
                    }
                    // Pre-check free space if total size known
                    if total_all > 0 {
                        if let Ok(avail) = fs2::available_space(&target_dir) {
                            let need = total_all.saturating_sub(resume_from);
                            if avail <= reserve_bytes.saturating_add(need) {
                                let extra = json!({"need": need, "available": avail, "reserve": reserve_bytes});
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "disk_insufficient",
                                    "insufficient disk space",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                // Ledger: deny
                                let mut entry = json!({
                                    "decision": "deny",
                                    "reason_code": "disk_insufficient",
                                    "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                    "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                    "episode_id": null,
                                    "corr_id": corr_id,
                                    "node_id": null,
                                    "tool_id": "models.download",
                                    "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                    "bytes_out": 0u64,
                                    "bytes_in": 0u64,
                                    "duration_ms": 0u64,
                                });
                                crate::ext::corr::ensure_corr(&mut entry);
                                super::super::ext::io::egress_ledger_append(&entry).await;
                                sp.bus.publish("Egress.Ledger.Appended", &entry);
                                return;
                            }
                        }
                    }
                    let mut file = if resume_from > 0 {
                        match afs::OpenOptions::new().append(true).open(&tmp).await {
                            Ok(f) => BufWriter::with_capacity(1 << 20, f),
                            Err(e) => {
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "open_failed",
                                    &format!("open failed: {}", e),
                                    Some(&budget),
                                    None,
                                    Some(&corr_id),
                                )
                                .await;
                                return;
                            }
                        }
                    } else {
                        match afs::File::create(&tmp).await {
                            Ok(f) => BufWriter::with_capacity(1 << 20, f),
                            Err(e) => {
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "create_failed",
                                    &format!("create failed: {}", e),
                                    Some(&budget),
                                    None,
                                    Some(&corr_id),
                                )
                                .await;
                                return;
                            }
                        }
                    };
                    // Hash on-the-fly when not resuming (avoids extra disk pass)
                    let mut hasher_opt = if expect_sha.is_some() && resume_from == 0 {
                        Some(sha2::Sha256::new())
                    } else {
                        None
                    };
                    let mut downloaded: u64 = 0;
                    let mut since_check: u64 = 0;
                    // stream-level retries for transient errors (resume with Range)
                    let mut stream_retries_left: u32 = std::env::var("ARW_DL_STREAM_RETRIES")
                        .ok()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(2);
                    // Soft-budget degrade threshold (percentage of soft budget used)
                    let soft_total = budget.soft_ms;
                    let degrade_pct: u64 = std::env::var("ARW_BUDGET_SOFT_DEGRADE_PCT")
                        .ok()
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|v| *v > 0 && *v < 100)
                        .unwrap_or(80);
                    let soft_degrade_ms = if soft_total > 0 {
                        Some(soft_total.saturating_mul(degrade_pct) / 100)
                    } else {
                        None
                    };
                    let mut degraded_sent = false;
                    let mut stream = resp.bytes_stream();
                    let idle_dur = if budget.hard_ms == 0 {
                        Self::idle_timeout_duration()
                    } else {
                        None
                    };
                    'stream_loop: loop {
                        let next_chunk = match idle_dur {
                            Some(d) => {
                                match tokio::time::timeout(d, stream.next()).await {
                                    Ok(v) => v,
                                    Err(_elapsed) => {
                                        // Idle read timeout: attempt resume if possible, else error out
                                        if stream_retries_left > 0 && !budget.hard_exhausted() {
                                            stream_retries_left -= 1;
                                            // Advance resume offset to include what we wrote this attempt
                                            resume_from = resume_from.saturating_add(downloaded);
                                            downloaded = 0;
                                            // Build a new ranged request from current offset
                                            let mut rq = client.get(&url);
                                            rq = budget.apply_to_request(rq);
                                            rq = rq.header(
                                                reqwest::header::RANGE,
                                                format!("bytes={}-", resume_from),
                                            );
                                            if let Ok(bytes) = afs::read(&meta_path).await {
                                                if let Ok(val) =
                                                    serde_json::from_slice::<serde_json::Value>(
                                                        &bytes,
                                                    )
                                                {
                                                    if let Some(etag) =
                                                        val.get("etag").and_then(|v| v.as_str())
                                                    {
                                                        rq = rq.header(
                                                            reqwest::header::IF_RANGE,
                                                            etag.to_string(),
                                                        );
                                                    } else if let Some(lm) = val
                                                        .get("last_modified")
                                                        .and_then(|v| v.as_str())
                                                    {
                                                        rq = rq.header(
                                                            reqwest::header::IF_RANGE,
                                                            lm.to_string(),
                                                        );
                                                    }
                                                }
                                            }
                                            match rq.send().await {
                                                Ok(r2) => {
                                                    let st = r2.status();
                                                    if st == reqwest::StatusCode::PARTIAL_CONTENT {
                                                        // Update resume validators from new response
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
                                                                obj.insert(
                                                                    "etag".into(),
                                                                    Value::String(e.clone()),
                                                                );
                                                            }
                                                            if let Some(lm) = &lm_val {
                                                                obj.insert(
                                                                    "last_modified".into(),
                                                                    Value::String(lm.clone()),
                                                                );
                                                            }
                                                            let _ = afs::write(
                                                                &meta_path,
                                                                serde_json::to_vec(&Value::Object(
                                                                    obj,
                                                                ))
                                                                .unwrap_or_default(),
                                                            )
                                                            .await;
                                                        }
                                                        let p = json!({"offset": resume_from, "reason": "idle_timeout"});
                                                        emit_progress(
                                                            &sp.bus,
                                                            &id,
                                                            Some("resumed"),
                                                            Some("resumed"),
                                                            if Self::progress_include_budget() {
                                                                Some(&budget)
                                                            } else {
                                                                None
                                                            },
                                                            Some(p),
                                                            Some(&corr_id),
                                                        );
                                                        stream = r2.bytes_stream();
                                                        continue 'stream_loop;
                                                    } else if st == reqwest::StatusCode::OK {
                                                        let _ = afs::remove_file(&tmp).await;
                                                        let _ = afs::remove_file(&meta_path).await; // meta no longer valid
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
                                                                obj.insert(
                                                                    "etag".into(),
                                                                    Value::String(e.clone()),
                                                                );
                                                            }
                                                            if let Some(lm) = &lm_val {
                                                                obj.insert(
                                                                    "last_modified".into(),
                                                                    Value::String(lm.clone()),
                                                                );
                                                            }
                                                            let _ = afs::write(
                                                                &meta_path,
                                                                serde_json::to_vec(&Value::Object(
                                                                    obj,
                                                                ))
                                                                .unwrap_or_default(),
                                                            )
                                                            .await;
                                                        }
                                                        match afs::File::create(&tmp).await {
                                                            Ok(f) => {
                                                                file = BufWriter::with_capacity(
                                                                    1 << 20,
                                                                    f,
                                                                );
                                                                resume_from = 0;
                                                                downloaded = 0;
                                                                if expect_sha.is_some() {
                                                                    hasher_opt =
                                                                        Some(sha2::Sha256::new());
                                                                }
                                                                emit_progress(
                                                                    &sp.bus,
                                                                    &id,
                                                                    Some("resync"),
                                                                    Some("resync"),
                                                                    if Self::progress_include_budget(
                                                                    ) {
                                                                        Some(&budget)
                                                                    } else {
                                                                        None
                                                                    },
                                                                    None,
                                                                    Some(&corr_id),
                                                                );
                                                                stream = r2.bytes_stream();
                                                                continue 'stream_loop;
                                                            }
                                                            Err(e2) => {
                                                                emit_error(
                                                                    &sp.bus,
                                                                    &id,
                                                                    "resync_failed",
                                                                    &format!(
                                                                        "resync failed: {}",
                                                                        e2
                                                                    ),
                                                                    Some(&budget),
                                                                    None,
                                                                    Some(&corr_id),
                                                                )
                                                                .await;
                                                                return;
                                                            }
                                                        }
                                                    } else {
                                                        emit_error(
                                                            &sp.bus,
                                                            &id,
                                                            "resume_http_status",
                                                            &format!(
                                                                "resume http status {}",
                                                                st.as_u16()
                                                            ),
                                                            Some(&budget),
                                                            None,
                                                            Some(&corr_id),
                                                        )
                                                        .await;
                                                        return;
                                                    }
                                                }
                                                Err(e2) => {
                                                    emit_error(
                                                        &sp.bus,
                                                        &id,
                                                        "resume_failed",
                                                        &format!("resume failed: {} (prior: idle timeout)", e2),
                                                        Some(&budget),
                                                        None,
                                                        Some(&corr_id),
                                                    ).await;
                                                    return;
                                                }
                                            }
                                        } else {
                                            let extra = json!({"offset": resume_from + downloaded});
                                            emit_error(
                                                &sp.bus,
                                                &id,
                                                "idle_timeout",
                                                "idle read timeout",
                                                Some(&budget),
                                                Some(extra),
                                                Some(&corr_id),
                                            )
                                            .await;
                                            let mut entry = json!({
                                                "decision": "deny",
                                                "reason_code": "idle_timeout",
                                                "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                                "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                                "episode_id": null,
                                                "corr_id": corr_id,
                                                "node_id": null,
                                                "tool_id": "models.download",
                                                "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                                "bytes_out": 0u64,
                                                "bytes_in": resume_from + downloaded,
                                                "duration_ms": t0.elapsed().as_millis() as u64,
                                            });
                                            crate::ext::corr::ensure_corr(&mut entry);
                                            super::super::ext::io::egress_ledger_append(&entry)
                                                .await;
                                            sp.bus.publish("Egress.Ledger.Appended", &entry);
                                            return;
                                        }
                                    }
                                }
                            }
                            None => stream.next().await,
                        };

                        match next_chunk {
                            None => break,
                            Some(Ok(bytes)) => {
                                // Enforce hard budget mid-stream
                                if budget.hard_exhausted() {
                                    let _ = afs::remove_file(&tmp).await;
                                    let extra = json!({"spent_ms": budget.spent_ms()});
                                    emit_error(
                                        &sp.bus,
                                        &id,
                                        "hard_exhausted",
                                        "hard budget exhausted",
                                        Some(&budget),
                                        Some(extra),
                                        Some(&corr_id),
                                    )
                                    .await;
                                    // Ledger: deny (budget hard exhausted)
                                    let mut entry = json!({
                                        "decision": "deny",
                                        "reason_code": "hard_exhausted",
                                        "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                        "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                        "episode_id": null,
                                        "corr_id": corr_id,
                                        "node_id": null,
                                        "tool_id": "models.download",
                                        "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                        "bytes_out": 0u64,
                                        "bytes_in": resume_from + downloaded,
                                        "duration_ms": t0.elapsed().as_millis() as u64,
                                    });
                                    crate::ext::corr::ensure_corr(&mut entry);
                                    super::super::ext::io::egress_ledger_append(&entry).await;
                                    sp.bus.publish("Egress.Ledger.Appended", &entry);
                                    return;
                                }
                                // Fire a one-time degrade notification when soft budget crosses threshold
                                if let Some(th) = soft_degrade_ms {
                                    if !degraded_sent && budget.spent_ms() >= th {
                                        degraded_sent = true;
                                        let p = json!({
                                            "reason": "soft budget threshold",
                                            "spent_ms": budget.spent_ms()
                                        });
                                        emit_progress(
                                            &sp.bus,
                                            &id,
                                            Some("degraded"),
                                            Some("soft_exhausted"),
                                            if Self::progress_include_budget() {
                                                Some(&budget)
                                            } else {
                                                None
                                            },
                                            Some(p),
                                            Some(&corr_id),
                                        );
                                    }
                                }
                                if Self::is_canceled(&job).await {
                                    let _ = afs::remove_file(&tmp).await;
                                    let _ = afs::remove_file(&meta_path).await;
                                    emit_progress(
                                        &sp.bus,
                                        &id,
                                        Some("canceled"),
                                        Some("canceled_by_user"),
                                        if Self::progress_include_budget() {
                                            Some(&budget)
                                        } else {
                                            None
                                        },
                                        None,
                                        Some(&corr_id),
                                    );
                                    let p2 = json!({"id": id, "status": "canceled"});
                                    crate::ext::io::audit_event("models.download.canceled", &p2)
                                        .await;
                                    // Update models list to reflect cancellation
                                    {
                                        let mut v = crate::ext::models().write().await;
                                        if let Some(m) = v.iter_mut().find(|m| {
                                            m.get("id").and_then(|s| s.as_str()) == Some(&id)
                                        }) {
                                            if let Some(obj) = m.as_object_mut() {
                                                obj.insert(
                                                    "status".into(),
                                                    Value::String("canceled".into()),
                                                );
                                            }
                                        }
                                        let _ = crate::ext::io::save_json_file_async(
                                            &crate::ext::paths::models_path(),
                                            &Value::Array(v.clone()),
                                        )
                                        .await;
                                    }
                                    sp.bus.publish(
                                        "Models.Changed",
                                        &json!({"op":"canceled","id": id}),
                                    );
                                    return;
                                }
                                if let Err(e) = file.write_all(&bytes).await {
                                    emit_error(
                                        &sp.bus,
                                        &id,
                                        "io_write",
                                        &format!("write failed: {}", e),
                                        Some(&budget),
                                        None,
                                        Some(&corr_id),
                                    )
                                    .await;
                                    return;
                                }
                                if let Some(ref mut h) = hasher_opt {
                                    h.update(&bytes);
                                }
                                downloaded += bytes.len() as u64;
                                // Enforce max size during stream when total unknown
                                if max_bytes > 0 && resume_from + downloaded > max_bytes {
                                    let _ = afs::remove_file(&tmp).await;
                                    let _ = afs::remove_file(&meta_path).await;
                                    let extra = json!({"downloaded": resume_from + downloaded, "max_bytes": max_bytes});
                                    emit_error(
                                        &sp.bus,
                                        &id,
                                        "size_limit_stream",
                                        "size exceeds limit (stream)",
                                        Some(&budget),
                                        Some(extra),
                                        Some(&corr_id),
                                    )
                                    .await;
                                    // Ledger: deny
                                    let mut entry = json!({
                                        "decision": "deny",
                                        "reason_code": "size_limit_stream",
                                        "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                        "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                        "episode_id": null,
                                        "corr_id": corr_id,
                                        "node_id": null,
                                        "tool_id": "models.download",
                                        "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                        "bytes_out": 0u64,
                                        "bytes_in": resume_from + downloaded,
                                        "duration_ms": t0.elapsed().as_millis() as u64,
                                    });
                                    crate::ext::corr::ensure_corr(&mut entry);
                                    super::super::ext::io::egress_ledger_append(&entry).await;
                                    sp.bus.publish("Egress.Ledger.Appended", &entry);
                                    return;
                                }
                                // For unknown total, periodically ensure we keep reserve free space
                                since_check = since_check.saturating_add(bytes.len() as u64);
                                if total_all == 0 && since_check >= 8 * 1024 * 1024 {
                                    since_check = 0;
                                    if let Ok(avail) = fs2::available_space(&target_dir) {
                                        if avail <= reserve_bytes.saturating_add(8 * 1024 * 1024) {
                                            let _ = afs::remove_file(&tmp).await;
                                            let _ = afs::remove_file(&meta_path).await;
                                            let extra = json!({"downloaded": resume_from + downloaded, "available": avail, "reserve": reserve_bytes});
                                            emit_error(
                                                &sp.bus,
                                                &id,
                                                "disk_insufficient_stream",
                                                "insufficient disk space (stream)",
                                                Some(&budget),
                                                Some(extra),
                                                Some(&corr_id),
                                            )
                                            .await;
                                            // Ledger: deny
                                            let mut entry = json!({
                                                "decision": "deny",
                                                "reason_code": "disk_insufficient_stream",
                                                "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                                                "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                                                "episode_id": null,
                                                "corr_id": corr_id,
                                                "node_id": null,
                                                "tool_id": "models.download",
                                                "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                                                "bytes_out": 0u64,
                                                "bytes_in": resume_from + downloaded,
                                                "duration_ms": t0.elapsed().as_millis() as u64,
                                            });
                                            crate::ext::corr::ensure_corr(&mut entry);
                                            super::super::ext::io::egress_ledger_append(&entry)
                                                .await;
                                            sp.bus.publish("Egress.Ledger.Appended", &entry);
                                            return;
                                        }
                                        // Emit standardized heartbeat when total unknown
                                        let mut extra =
                                            json!({"downloaded": resume_from + downloaded});
                                        if Self::progress_include_disk() {
                                            if let Ok(total) = fs2::total_space(&target_dir) {
                                                extra["disk"] = json!({"available": avail, "total": total, "reserve": reserve_bytes});
                                            }
                                        }
                                        emit_progress(
                                            &sp.bus,
                                            &id,
                                            Some("downloading"),
                                            Some("downloading"),
                                            if Self::progress_include_budget() {
                                                Some(&budget)
                                            } else {
                                                None
                                            },
                                            Some(extra),
                                            Some(&corr_id),
                                        );
                                    }
                                }
                                if total_all > 0 {
                                    let pct =
                                        (((resume_from + downloaded) * 100) / total_all).min(100);
                                    let mut extra = json!({
                                        "progress": pct,
                                        "downloaded": resume_from + downloaded,
                                        "total": total_all
                                    });
                                    if Self::progress_include_disk() {
                                        if let (Ok(av), Ok(tt)) = (
                                            fs2::available_space(&target_dir),
                                            fs2::total_space(&target_dir),
                                        ) {
                                            extra["disk"] = json!({"available": av, "total": tt, "reserve": reserve_bytes});
                                        }
                                    }
                                    emit_progress(
                                        &sp.bus,
                                        &id,
                                        Some("downloading"),
                                        Some("progress"),
                                        if Self::progress_include_budget() {
                                            Some(&budget)
                                        } else {
                                            None
                                        },
                                        Some(extra),
                                        Some(&corr_id),
                                    );
                                }
                            }
                            Some(Err(e)) => {
                                // Try to resume if we have budget and retries left
                                if stream_retries_left > 0 && !budget.hard_exhausted() {
                                    stream_retries_left -= 1;
                                    // Advance resume offset to include what we wrote this attempt
                                    resume_from = resume_from.saturating_add(downloaded);
                                    downloaded = 0;
                                    // Build a new ranged request from current offset
                                    let mut rq = client.get(&url);
                                    rq = budget.apply_to_request(rq);
                                    // No whole-request timeout; idle is enforced per-chunk below.
                                    rq = rq.header(
                                        reqwest::header::RANGE,
                                        format!("bytes={}-", resume_from),
                                    );
                                    if let Ok(bytes) = afs::read(&meta_path).await {
                                        if let Ok(val) =
                                            serde_json::from_slice::<serde_json::Value>(&bytes)
                                        {
                                            if let Some(etag) =
                                                val.get("etag").and_then(|v| v.as_str())
                                            {
                                                rq = rq.header(
                                                    reqwest::header::IF_RANGE,
                                                    etag.to_string(),
                                                );
                                            } else if let Some(lm) =
                                                val.get("last_modified").and_then(|v| v.as_str())
                                            {
                                                rq = rq.header(
                                                    reqwest::header::IF_RANGE,
                                                    lm.to_string(),
                                                );
                                            }
                                        }
                                    }
                                    match rq.send().await {
                                        Ok(r2) => {
                                            let st = r2.status();
                                            if st == reqwest::StatusCode::PARTIAL_CONTENT {
                                                // Update resume validators from new response
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
                                                        obj.insert(
                                                            "etag".into(),
                                                            Value::String(e.clone()),
                                                        );
                                                    }
                                                    if let Some(lm) = &lm_val {
                                                        obj.insert(
                                                            "last_modified".into(),
                                                            Value::String(lm.clone()),
                                                        );
                                                    }
                                                    let _ = afs::write(
                                                        &meta_path,
                                                        serde_json::to_vec(&Value::Object(obj))
                                                            .unwrap_or_default(),
                                                    )
                                                    .await;
                                                }
                                                let p = json!({"offset": resume_from});
                                                emit_progress(
                                                    &sp.bus,
                                                    &id,
                                                    Some("resumed"),
                                                    Some("resumed"),
                                                    if Self::progress_include_budget() {
                                                        Some(&budget)
                                                    } else {
                                                        None
                                                    },
                                                    Some(p),
                                                    Some(&corr_id),
                                                );
                                                stream = r2.bytes_stream();
                                                continue 'stream_loop;
                                            } else if st == reqwest::StatusCode::OK {
                                                // Server ignored range; safest is to restart from zero
                                                // Remove tmp and start fresh (but only if allowed by budget)
                                                let _ = afs::remove_file(&tmp).await;
                                                let _ = afs::remove_file(&meta_path).await; // meta no longer valid
                                                                                            // Refresh validators from this new full response
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
                                                        obj.insert(
                                                            "etag".into(),
                                                            Value::String(e.clone()),
                                                        );
                                                    }
                                                    if let Some(lm) = &lm_val {
                                                        obj.insert(
                                                            "last_modified".into(),
                                                            Value::String(lm.clone()),
                                                        );
                                                    }
                                                    let _ = afs::write(
                                                        &meta_path,
                                                        serde_json::to_vec(&Value::Object(obj))
                                                            .unwrap_or_default(),
                                                    )
                                                    .await;
                                                }
                                                match afs::File::create(&tmp).await {
                                                    Ok(f) => {
                                                        file = BufWriter::with_capacity(1 << 20, f);
                                                        resume_from = 0;
                                                        downloaded = 0;
                                                        // Since we're starting from zero again, hash on the fly.
                                                        if expect_sha.is_some() {
                                                            hasher_opt = Some(sha2::Sha256::new());
                                                        }
                                                        emit_progress(
                                                            &sp.bus,
                                                            &id,
                                                            Some("resync"),
                                                            Some("resync"),
                                                            if Self::progress_include_budget() {
                                                                Some(&budget)
                                                            } else {
                                                                None
                                                            },
                                                            None,
                                                            Some(&corr_id),
                                                        );
                                                        stream = r2.bytes_stream();
                                                        continue 'stream_loop;
                                                    }
                                                    Err(e2) => {
                                                        emit_error(
                                                            &sp.bus,
                                                            &id,
                                                            "resync_failed",
                                                            &format!("resync failed: {}", e2),
                                                            Some(&budget),
                                                            None,
                                                            Some(&corr_id),
                                                        )
                                                        .await;
                                                        return;
                                                    }
                                                }
                                            } else {
                                                emit_error(
                                                    &sp.bus,
                                                    &id,
                                                    "resume_http_status",
                                                    &format!("resume http status {}", st.as_u16()),
                                                    Some(&budget),
                                                    None,
                                                    Some(&corr_id),
                                                )
                                                .await;
                                                return;
                                            }
                                        }
                                        Err(e2) => {
                                            emit_error(
                                                &sp.bus,
                                                &id,
                                                "resume_failed",
                                                &format!("resume failed: {} (prior: {})", e2, e),
                                                Some(&budget),
                                                None,
                                                Some(&corr_id),
                                            )
                                            .await;
                                            return;
                                        }
                                    }
                                } else {
                                    emit_error(
                                        &sp.bus,
                                        &id,
                                        "io_read",
                                        &format!("read failed: {}", e),
                                        Some(&budget),
                                        None,
                                        Some(&corr_id),
                                    )
                                    .await;
                                    return;
                                }
                            }
                        }
                    }
                    if let Err(e) = file.flush().await {
                        // Cleanup temp artifacts on flush failure
                        let _ = afs::remove_file(&tmp).await;
                        let _ = afs::remove_file(&meta_path).await;
                        emit_error(
                            &sp.bus,
                            &id,
                            "flush_failed",
                            &format!("flush failed: {}", e),
                            Some(&budget),
                            None,
                            Some(&corr_id),
                        )
                        .await;
                        return;
                    }
                    // Ensure handle closed before rename on platforms with exclusive locks
                    drop(file);
                    // Verify checksum BEFORE promoting tmp -> final target
                    if let Some(ref exp) = expect_sha {
                        let actual = if let Some(h) = hasher_opt.take() {
                            format!("{:x}", h.finalize())
                        } else {
                            // resumed: compute from file on disk
                            let mut f = match afs::File::open(&tmp).await {
                                Ok(f) => f,
                                Err(e) => {
                                    // Could not open temp file to verify checksum
                                    let _ = afs::remove_file(&tmp).await;
                                    let _ = afs::remove_file(&meta_path).await;
                                    emit_error(
                                        &sp.bus,
                                        &id,
                                        "verify_open_failed",
                                        &format!("verify open failed: {}", e),
                                        Some(&budget),
                                        None,
                                        Some(&corr_id),
                                    )
                                    .await;
                                    return;
                                }
                            };
                            let mut h = sha2::Sha256::new();
                            let mut buf = vec![0u8; 1024 * 1024];
                            loop {
                                match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
                                    Ok(0) => break,
                                    Ok(n) => h.update(&buf[..n]),
                                    Err(e) => {
                                        // Read failed during verification; abort and clean up
                                        let _ = afs::remove_file(&tmp).await;
                                        let _ = afs::remove_file(&meta_path).await;
                                        emit_error(
                                            &sp.bus,
                                            &id,
                                            "verify_read_failed",
                                            &format!("verify read failed: {}", e),
                                            Some(&budget),
                                            None,
                                            Some(&corr_id),
                                        )
                                        .await;
                                        return;
                                    }
                                }
                            }
                            format!("{:x}", h.finalize())
                        };
                        if actual != *exp {
                            let _ = afs::remove_file(&tmp).await;
                            let _ = afs::remove_file(&meta_path).await;
                            let extra = json!({"expected": exp.clone(), "actual": actual});
                            emit_error(
                                &sp.bus,
                                &id,
                                "checksum_mismatch",
                                "checksum mismatch",
                                Some(&budget),
                                Some(extra),
                                Some(&corr_id),
                            )
                            .await;
                            return;
                        }
                    }
                    // If server reported a total size, ensure the file matches
                    if total_all > 0 {
                        if let Ok(md) = afs::metadata(&tmp).await {
                            if md.len() != total_all {
                                let _ = afs::remove_file(&tmp).await;
                                let _ = afs::remove_file(&meta_path).await;
                                let extra =
                                    json!({"expected_bytes": total_all, "actual_bytes": md.len()});
                                emit_error(
                                    &sp.bus,
                                    &id,
                                    "size_mismatch",
                                    "size mismatch",
                                    Some(&budget),
                                    Some(extra),
                                    Some(&corr_id),
                                )
                                .await;
                                return;
                            }
                        }
                    }
                    // Promote tmp to a content-addressed target path now that verification passed
                    // Layout: <state>/models/by-hash/<sha256>[.<ext>]
                    let cas_file_name: String;
                    if let Some(ref exp) = expect_sha {
                        let cas_dir = target_dir.join("by-hash");
                        if let Err(e) = afs::create_dir_all(&cas_dir).await {
                            emit_error(
                                &sp.bus,
                                &id,
                                "mkdir_failed",
                                &format!("mkdir failed: {}", e),
                                Some(&budget),
                                None,
                                Some(&corr_id),
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
                            let _ = afs::remove_file(&tmp).await;
                        } else if let Err(_e) = afs::rename(&tmp, &cas_target).await {
                            // If rename fails (Windows lock or cross-device), try remove + rename,
                            // and finally fall back to copy + remove.
                            let _ = afs::remove_file(&cas_target).await;
                            match afs::rename(&tmp, &cas_target).await {
                                Ok(_) => {}
                                Err(e2) => match afs::copy(&tmp, &cas_target).await {
                                    Ok(_) => {
                                        let _ = afs::remove_file(&tmp).await;
                                    }
                                    Err(_) => {
                                        emit_error(
                                            &sp.bus,
                                            &id,
                                            "finalize_failed",
                                            &format!("finalize failed: {}", e2),
                                            Some(&budget),
                                            None,
                                            Some(&corr_id),
                                        )
                                        .await;
                                        return;
                                    }
                                },
                            }
                        }
                        // update target to the content-addressed path
                        target = cas_target.clone();
                    } else {
                        // Fallback (should not happen since we require sha256): keep original target path
                        if let Err(_e) = afs::rename(&tmp, &target).await {
                            let _ = afs::remove_file(&target).await;
                            match afs::rename(&tmp, &target).await {
                                Ok(_) => {}
                                Err(e2) => match afs::copy(&tmp, &target).await {
                                    Ok(_) => {
                                        let _ = afs::remove_file(&tmp).await;
                                    }
                                    Err(_) => {
                                        emit_error(
                                            &sp.bus,
                                            &id,
                                            "finalize_failed",
                                            &format!("finalize failed: {}", e2),
                                            Some(&budget),
                                            None,
                                            Some(&corr_id),
                                        )
                                        .await;
                                        return;
                                    }
                                },
                            }
                        }
                        cas_file_name = final_name.clone();
                    }
                    // fsync finalized CAS/target file for durability
                    if let Ok(f) = afs::File::open(&target).await {
                        let _ = f.sync_all().await;
                    }
                    // cleanup sidecar meta on success
                    let _ = afs::remove_file(&meta_path).await;
                    // Write a sidecar manifest <id>.json alongside the model
                    let manifest_path =
                        target_dir.join(format!("{}.json", Self::sanitize_file_name(&id)));
                    // Emit manifest written event
                    let mut ev = json!({"id": id, "manifest_path": manifest_path.to_string_lossy(), "sha256": expect_sha.clone(), "cas": "sha256", "corr_id": corr_id});
                    crate::ext::corr::ensure_corr(&mut ev);
                    sp.bus.publish("Models.ManifestWritten", &ev);
                    let bytes = match afs::metadata(&target).await {
                        Ok(md) => md.len(),
                        Err(_) => 0,
                    };
                    let mut manifest = serde_json::Map::new();
                    manifest.insert("id".into(), Value::String(id.clone()));
                    // file: canonical content-addressed filename; name: original display filename (if different)
                    manifest.insert("file".into(), Value::String(cas_file_name.clone()));
                    if cas_file_name != final_name {
                        manifest.insert("name".into(), Value::String(final_name.clone()));
                    }
                    manifest.insert(
                        "path".into(),
                        Value::String(target.to_string_lossy().to_string()),
                    );
                    manifest.insert("url".into(), Value::String(Self::redact_url_for_logs(&url)));
                    if let Some(exp) = expect_sha.clone() {
                        manifest.insert("sha256".into(), Value::String(exp));
                        manifest.insert("cas".into(), Value::String("sha256".into()));
                    }
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
                    // Update EWMA throughput based on observed bytes/time
                    let elapsed_ms = t0.elapsed().as_millis() as u64;
                    if elapsed_ms > 0 {
                        if let Ok(md) = afs::metadata(&target).await {
                            let bytes = md.len() as f64;
                            let mbps = (bytes / (1024.0 * 1024.0)) / (elapsed_ms as f64 / 1000.0);
                            Self::update_ewma_mbps(mbps).await;
                        }
                    }
                    let mut p = json!({"id": id, "status":"complete", "file": final_name, "provider": provider, "cas_file": cas_file_name});
                    if Self::progress_include_disk() {
                        if let (Ok(av), Ok(tt)) = (
                            fs2::available_space(&target_dir),
                            fs2::total_space(&target_dir),
                        ) {
                            p["disk"] =
                                json!({"available": av, "total": tt, "reserve": reserve_bytes});
                        }
                    }
                    // Emit standardized completion event
                    let mut extra = p.clone();
                    // extra already contains id/status/file/provider and maybe disk; remove id/status to avoid duplication in payload
                    if let Some(obj) = extra.as_object_mut() {
                        obj.remove("id");
                        obj.remove("status");
                    }
                    emit_progress(
                        &sp.bus,
                        &id,
                        Some("complete"),
                        Some("complete"),
                        if Self::progress_include_budget() {
                            Some(&budget)
                        } else {
                            None
                        },
                        Some(extra),
                        Some(&corr_id),
                    );
                    // Audit completion with full object
                    crate::ext::io::audit_event("models.download.complete", &p).await;
                    // Append egress ledger (best-effort)
                    let mut entry = json!({
                        "decision": "allow",
                        "reason_code": "models.download",
                        "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                        "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                        "episode_id": null,
                        "corr_id": corr_id,
                        "node_id": null,
                        "tool_id": "models.download",
                        "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                        "bytes_out": 0u64,
                        "bytes_in": bytes,
                        "duration_ms": elapsed_ms,
                    });
                    crate::ext::corr::ensure_corr(&mut entry);
                    super::super::ext::io::egress_ledger_append(&entry).await;
                    sp.bus.publish("Egress.Ledger.Appended", &entry);
                    {
                        let mut v = crate::ext::models().write().await;
                        if let Some(m) = v
                            .iter_mut()
                            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
                        {
                            let mut obj = serde_json::Map::new();
                            obj.insert("id".into(), Value::String(id.clone()));
                            obj.insert("provider".into(), Value::String(provider.clone()));
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
                            if let Ok(md) = afs::metadata(&target).await {
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
                        .publish("Models.Changed", &json!({"op":"downloaded","id": id}));
                }
                Err(e) => {
                    emit_error(
                        &sp.bus,
                        &id,
                        "request_failed",
                        &format!("request failed: {}", e),
                        Some(&budget),
                        None,
                        Some(&corr_id),
                    )
                    .await;
                    // Append failed egress attempt to ledger (best-effort)
                    let mut entry = json!({
                        "decision": "deny",
                        "reason_code": "request_failed",
                        "posture": std::env::var("ARW_NET_POSTURE").unwrap_or_else(|_| "off".into()),
                        "project_id": std::env::var("ARW_PROJECT_ID").unwrap_or_else(|_| "default".into()),
                        "episode_id": null,
                        "corr_id": corr_id,
                        "node_id": null,
                        "tool_id": "models.download",
                        "dest": {"host": dest_host, "port": dest_port as u64, "protocol": dest_proto},
                        "bytes_out": 0u64,
                        "bytes_in": 0u64,
                        "duration_ms": t0.elapsed().as_millis() as u64,
                        "error": e.to_string(),
                    });
                    crate::ext::corr::ensure_corr(&mut entry);
                    super::super::ext::io::egress_ledger_append(&entry).await;
                    sp.bus.publish("Egress.Ledger.Appended", &entry);
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
}
