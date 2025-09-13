use anyhow::Result;
use directories::ProjectDirs;
use once_cell::sync::OnceCell;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager; // for get_webview_window on AppHandle
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

/// Shared state holder for managing a spawned `arw-svc` child process.
#[derive(Clone)]
pub struct ServiceState {
    inner: Arc<Mutex<Option<Child>>>,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }
}

fn default_port() -> u16 {
    std::env::var("ARW_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8090)
}

fn effective_port(port: Option<u16>) -> u16 {
    if let Some(p) = port {
        return p;
    }
    // Prefer launcher prefs if set
    if let Some(prefs) = prefs_path(Some("launcher"))
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    {
        if let Some(p) = prefs
            .get("port")
            .and_then(|v| v.as_u64())
            .and_then(|n| u16::try_from(n).ok())
        {
            return p;
        }
    }
    default_port()
}

fn service_url(path: &str, port: Option<u16>) -> String {
    format!(
        "http://127.0.0.1:{}/{}",
        effective_port(port),
        path.trim_start_matches('/')
    )
}

fn admin_token() -> Option<String> {
    if let Ok(t) = std::env::var("ARW_ADMIN_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Some(path) = prefs_path(Some("launcher")) {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                if let Some(s) = v.get("adminToken").and_then(|x| x.as_str()) {
                    if !s.is_empty() {
                        return Some(s.to_string());
                    }
                }
            }
        }
    }
    None
}

/// Locate the `arw-svc` binary near the app or in the workspace target dir.
pub fn locate_svc_binary() -> Option<PathBuf> {
    // 1) next to this exe (packaged dist)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("arw-svc");
            let candidate_win = dir.join("arw-svc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
            if candidate_win.exists() {
                return Some(candidate_win);
            }
            // packaged layout may be bin/ alongside
            let bin = dir.join("bin");
            let c2 = bin.join("arw-svc");
            let c2w = bin.join("arw-svc.exe");
            if c2.exists() {
                return Some(c2);
            }
            if c2w.exists() {
                return Some(c2w);
            }
        }
    }
    // 2) workspace target/release
    let mut path = std::env::current_dir().ok()?;
    for _ in 0..3 {
        let p = path.join("target").join("release").join(if cfg!(windows) {
            "arw-svc.exe"
        } else {
            "arw-svc"
        });
        if p.exists() {
            return Some(p);
        }
        path = path.parent()?.to_path_buf();
    }
    None
}

fn prefs_path(namespace: Option<&str>) -> Option<PathBuf> {
    let proj = ProjectDirs::from("org", "arw", "arw")?;
    let dir = proj.config_dir();
    std::fs::create_dir_all(dir).ok()?;
    let file = match namespace {
        Some(ns) if !ns.is_empty() => format!("prefs-{}.json", ns),
        _ => "prefs.json".to_string(),
    };
    Some(dir.join(file))
}

pub fn load_prefs(namespace: Option<&str>) -> Value {
    if let Some(path) = prefs_path(namespace) {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                return v;
            }
        }
    }
    Value::Null
}

pub fn save_prefs(namespace: Option<&str>, value: &Value) -> Result<()> {
    if let Some(path) = prefs_path(namespace) {
        let data = serde_json::to_vec_pretty(value)?;
        std::fs::write(path, data)?;
    }
    Ok(())
}

mod cmds {
    use super::*;

    #[tauri::command]
    pub async fn check_service_health(port: Option<u16>) -> Result<bool, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_millis(1200))
                .build()
                .unwrap()
        });
        let url = service_url("healthz", port);
        match client.get(url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(err) => Err(format!("health request failed: {}", err)),
        }
    }

    #[tauri::command]
    pub fn open_debug_ui(port: Option<u16>) -> Result<(), String> {
        let url = service_url("debug", port);
        open::that(url).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn open_debug_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        port: Option<u16>,
    ) -> Result<(), String> {
        let url = service_url("debug", port);
        let label = "debug";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::External(url.parse().unwrap()),
            )
            .title("Agent Hub (ARW) — Debug UI")
            .inner_size(1000.0, 800.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_events_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "events";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("events.html".into()),
            )
            .title("Agent Hub (ARW) — Events")
            .inner_size(900.0, 700.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_logs_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "logs";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("logs.html".into()),
            )
            .title("Agent Hub (ARW) — Logs")
            .inner_size(900.0, 700.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_models_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "models";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("models.html".into()),
            )
            .title("Agent Hub (ARW) — Model Manager")
            .inner_size(1000.0, 800.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_connections_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<(), String> {
        let label = "connections";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("connections.html".into()),
            )
            .title("Agent Hub (ARW) — Connection Manager")
            .inner_size(1000.0, 800.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn start_service(
        state: tauri::State<'_, ServiceState>,
        port: Option<u16>,
    ) -> Result<(), String> {
        // already running?
        {
            let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
            if let Some(child) = guard.as_mut() {
                if let Ok(None) = child.try_wait() {
                    return Ok(());
                }
            }
        }
        let svc_bin = locate_svc_binary().ok_or_else(|| "arw-svc binary not found".to_string())?;
        let mut cmd = Command::new(svc_bin);
        cmd.env("ARW_DEBUG", "1")
            .env("ARW_PORT", format!("{}", effective_port(port)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd.spawn().map_err(|e| e.to_string())?;
        *state.inner.lock().map_err(|e| e.to_string())? = Some(child);
        Ok(())
    }

    #[tauri::command]
    pub async fn stop_service(
        state: tauri::State<'_, ServiceState>,
        port: Option<u16>,
    ) -> Result<(), String> {
        // try graceful shutdown (async client)
        let url = service_url("shutdown", port);
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_millis(900))
                .build()
                .unwrap()
        });
        let _ = client.get(url).send().await;
        // then kill if needed
        if let Some(mut child) = state.inner.lock().map_err(|e| e.to_string())?.take() {
            let _ = child.kill();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn get_prefs(namespace: Option<String>) -> Result<Value, String> {
        Ok(load_prefs(namespace.as_deref()))
    }

    #[tauri::command]
    pub fn set_prefs(namespace: Option<String>, value: Value) -> Result<(), String> {
        save_prefs(namespace.as_deref(), &value).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn launcher_autostart_status<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<bool, String> {
        let mgr = app.autolaunch();
        mgr.is_enabled().map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set_launcher_autostart<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        enabled: bool,
    ) -> Result<(), String> {
        let mgr = app.autolaunch();
        if enabled {
            mgr.enable().map_err(|e| e.to_string())
        } else {
            mgr.disable().map_err(|e| e.to_string())
        }
    }

    #[tauri::command]
    pub fn open_url(url: String) -> Result<(), String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("invalid url".into());
        }
        open::that(url).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn open_path(path: String) -> Result<(), String> {
        // best-effort guard: reject very long or control characters
        if path.len() > 4096 || path.chars().any(|c| c.is_control()) {
            return Err("invalid path".into());
        }
        open::that(path).map_err(|e| e.to_string())
    }

    // ---- Models (admin) ----
    async fn admin_get(path: &str, port: Option<u16>) -> Result<reqwest::Response, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let mut headers = HeaderMap::new();
        if let Some(tok) = admin_token() {
            if let Ok(h) = HeaderValue::from_str(&tok) {
                headers.insert("X-ARW-Admin", h);
            }
        }
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
        });
        client
            .get(service_url(path, port))
            .headers(headers)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    async fn admin_post_json(
        path: &str,
        body: Value,
        port: Option<u16>,
    ) -> Result<reqwest::Response, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let mut headers = HeaderMap::new();
        if let Some(tok) = admin_token() {
            if let Ok(h) = HeaderValue::from_str(&tok) {
                headers.insert("X-ARW-Admin", h);
            }
        }
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap()
        });
        client
            .post(service_url(path, port))
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn models_list(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_get("admin/models", port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_refresh(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_post_json("admin/models/refresh", Value::Null, port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_save(port: Option<u16>) -> Result<(), String> {
        let _ = admin_post_json("admin/models/save", Value::Null, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn models_load(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_post_json("admin/models/load", Value::Null, port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_add(
        id: String,
        provider: Option<String>,
        port: Option<u16>,
    ) -> Result<(), String> {
        let body = serde_json::json!({"id": id, "provider": provider});
        let _ = admin_post_json("admin/models/add", body, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn models_delete(id: String, port: Option<u16>) -> Result<(), String> {
        let body = serde_json::json!({"id": id});
        let _ = admin_post_json("admin/models/delete", body, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn models_default_get(port: Option<u16>) -> Result<String, String> {
        let resp = admin_get("admin/models/default", port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v.get("default")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string())
    }

    #[tauri::command]
    pub async fn models_default_set(id: String, port: Option<u16>) -> Result<(), String> {
        let body = serde_json::json!({"id": id});
        let _ = admin_post_json("admin/models/default", body, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn models_download(
        id: String,
        url: String,
        provider: Option<String>,
        sha256: String,
        port: Option<u16>,
    ) -> Result<(), String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("invalid url".into());
        }
        let sh = sha256.trim().to_lowercase();
        if sh.len() != 64 || !sh.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("invalid sha256".into());
        }
        let body = serde_json::json!({"id": id, "url": url, "provider": provider, "sha256": sh});
        let _ = admin_post_json("admin/models/download", body, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn models_download_cancel(id: String, port: Option<u16>) -> Result<(), String> {
        let body = serde_json::json!({"id": id});
        let _ = admin_post_json("admin/models/download/cancel", body, port).await?;
        Ok(())
    }

    /// Build and return the Tauri plugin exposing ARW commands.
    pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("arw")
            .invoke_handler(tauri::generate_handler![
                check_service_health,
                open_debug_ui,
                open_debug_window,
                open_events_window,
                open_logs_window,
                open_models_window,
                open_connections_window,
                models_list,
                models_refresh,
                models_save,
                models_load,
                models_add,
                models_delete,
                models_default_get,
                models_default_set,
                models_download,
                models_download_cancel,
                start_service,
                stop_service,
                get_prefs,
                set_prefs,
                launcher_autostart_status,
                set_launcher_autostart,
                open_url,
                open_path
            ])
            .build()
    }
}

// Re-export commands at crate root for existing callers
pub use cmds::*;

/// Build and return the Tauri plugin exposing ARW commands (root wrapper)
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    cmds::plugin()
}
