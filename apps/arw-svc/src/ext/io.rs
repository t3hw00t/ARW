use serde_json::Value;
use std::fs;
use std::path::Path;
use tokio::fs as afs;

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
    let s = serde_json::to_string_pretty(v).unwrap_or_else(|_| "{}".to_string());
    afs::write(p, s.as_bytes()).await
}

pub(crate) async fn audit_event(action: &str, details: &Value) {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let line = serde_json::json!({"time": ts, "action": action, "details": details});
    let s = serde_json::to_string(&line).unwrap_or_else(|_| "{}".to_string()) + "\n";
    let p = crate::ext::paths::audit_path();
    if let Some(parent) = p.parent() {
        let _ = afs::create_dir_all(parent).await;
    }
    use tokio::io::AsyncWriteExt;
    if let Ok(mut f) = afs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .await
    {
        let _ = f.write_all(s.as_bytes()).await;
    }
}
