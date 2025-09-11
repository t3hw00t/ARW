use fs2::FileExt;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::fs as afs;
use tokio::io::AsyncWriteExt;

// In-process per-path async lock registry to serialize writers.
// Prevents concurrent writers from interleaving/truncating files.
static FILE_LOCKS: OnceLock<tokio::sync::RwLock<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();

fn file_locks() -> &'static tokio::sync::RwLock<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>> {
    FILE_LOCKS.get_or_init(|| tokio::sync::RwLock::new(HashMap::new()))
}

async fn lock_for_path(p: &Path) -> Arc<tokio::sync::Mutex<()>> {
    // Fast path: check read map
    {
        let map = file_locks().read().await;
        if let Some(l) = map.get(p) {
            return l.clone();
        }
    }
    // Slow path: insert
    let mut map = file_locks().write().await;
    map.entry(p.to_path_buf())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn tmp_path_for(p: &Path) -> PathBuf {
    // Use stable sibling temp name; unique enough under per-path lock.
    let mut t = p.as_os_str().to_owned();
    let suffix = std::ffi::OsStr::new(".tmp");
    t.push(suffix);
    PathBuf::from(t)
}

pub(crate) fn load_json_file(p: &Path) -> Option<Value> {
    let s = fs::read_to_string(p).ok()?;
    serde_json::from_str(&s).ok()
}

pub(crate) async fn load_json_file_async(p: &Path) -> Option<Value> {
    let s = afs::read_to_string(p).await.ok()?;
    serde_json::from_str(&s).ok()
}

pub(crate) async fn save_json_file_async(p: &Path, v: &Value) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    // cross-process advisory lock (best-effort)
    let _proc_lock = acquire_cross_lock(p).await;
    let bytes = serde_json::to_vec_pretty(v).unwrap_or_else(|_| b"{}".to_vec());
    let tmp = tmp_path_for(p);
    let lk = lock_for_path(p).await;
    let _guard = lk.lock().await;
    // Write to temp file then atomically rename into place (best-effort on Windows).
    {
        let mut f = afs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .await?;
        f.write_all(&bytes).await?;
        // Ensure content hits disk to minimize torn writes on power loss.
        let _ = f.sync_all().await;
    }
    match afs::rename(&tmp, p).await {
        Ok(_) => Ok(()),
        Err(_e) => {
            // Some platforms (Windows) fail if dest exists; try replace
            let _ = afs::remove_file(p).await;
            let r = afs::rename(&tmp, p).await;
            if r.is_err() {
                // Cleanup temp on persistent failure
                let _ = afs::remove_file(&tmp).await;
            }
            r
        }
    }
}

// Atomic write for arbitrary bytes (e.g., project notes)
pub(crate) async fn save_bytes_atomic(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    let _proc_lock = acquire_cross_lock(p).await;
    let tmp = tmp_path_for(p);
    let lk = lock_for_path(p).await;
    let _guard = lk.lock().await;
    {
        let mut f = afs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .await?;
        f.write_all(bytes).await?;
        let _ = f.sync_all().await;
    }
    match afs::rename(&tmp, p).await {
        Ok(_) => Ok(()),
        Err(_e) => {
            let _ = afs::remove_file(p).await;
            let r = afs::rename(&tmp, p).await;
            if r.is_err() {
                let _ = afs::remove_file(&tmp).await;
            }
            r
        }
    }
}

// Serialize audit writes with a process-wide mutex to avoid interleaving lines.
static AUDIT_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
pub(crate) async fn audit_event(action: &str, details: &Value) {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let line = serde_json::json!({"time": ts, "action": action, "details": details});
    let s = serde_json::to_string(&line).unwrap_or_else(|_| "{}".to_string()) + "\n";
    let p = crate::ext::paths::audit_path();
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    let lk = AUDIT_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _g = lk.lock().await;
    rotate_audit_if_needed(&p).await;
    if let Ok(mut f) = afs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .await
    {
        let _ = f.write_all(s.as_bytes()).await;
    }
}

async fn rotate_audit_if_needed(p: &Path) {
    // Max MB from env, default 10MB
    let max_mb: u64 = std::env::var("ARW_AUDIT_MAX_MB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let max_bytes = max_mb.saturating_mul(1024 * 1024);
    if let Ok(md) = afs::metadata(p).await {
        if md.len() >= max_bytes {
            let p1 = p.with_extension("log.1");
            let p2 = p.with_extension("log.2");
            let p3 = p.with_extension("log.3");
            let _ = afs::remove_file(&p3).await;
            if afs::metadata(&p2).await.is_ok() {
                let _ = afs::rename(&p2, &p3).await;
            }
            if afs::metadata(&p1).await.is_ok() {
                let _ = afs::rename(&p1, &p2).await;
            }
            if afs::metadata(p).await.is_ok() {
                let _ = afs::rename(p, &p1).await;
            }
        }
    }
}

async fn acquire_cross_lock(p: &Path) -> Option<std::fs::File> {
    use std::time::Duration;
    let lockp = p.with_extension("lock");
    tokio::task::spawn_blocking(move || {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lockp)
            .ok()?;
        // Try a few times
        for _ in 0..25 {
            if f.try_lock_exclusive().is_ok() {
                return Some(f);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    })
    .await
    .ok()
    .flatten()
}
