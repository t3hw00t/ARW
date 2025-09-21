use anyhow::Result;
use directories::ProjectDirs;
use once_cell::sync::OnceCell;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager; // for get_webview_window on AppHandle
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

/// Shared state holder for managing a spawned service child process.
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
        .unwrap_or(8091)
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

/// Locate the unified service binary (`arw-server`).
pub fn locate_service_binary() -> Option<PathBuf> {
    // 1) packaged layout (next to launcher or in ./bin)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let name = if cfg!(windows) {
                "arw-server.exe"
            } else {
                "arw-server"
            };
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
            let bin = dir.join("bin");
            let candidate = bin.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 2) workspace builds (target/release)
    let mut path = std::env::current_dir().ok()?;
    for _ in 0..3 {
        let name = if cfg!(windows) {
            "arw-server.exe"
        } else {
            "arw-server"
        };
        let candidate = path.join("target").join("release").join(name);
        if candidate.exists() {
            return Some(candidate);
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
        // Align with service route mounted under /admin
        let url = service_url("admin/debug", port);
        open::that(url).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn open_debug_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        port: Option<u16>,
    ) -> Result<(), String> {
        // Align with service route mounted under /admin
        let url = service_url("admin/debug", port);
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
    pub fn active_window_bounds<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
    ) -> Result<Value, String> {
        // Prefer provided label, else focus main/any
        let win = if let Some(l) = label.as_deref() {
            app.get_webview_window(l)
        } else if let Some(w) = app.get_webview_window("main") {
            Some(w)
        } else {
            app.webview_windows().values().next().cloned()
        };
        let Some(w) = win else {
            return Err("no window".into());
        };
        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "x": pos.x,
            "y": pos.y,
            "w": size.width,
            "h": size.height
        }))
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
    pub fn open_events_window_base<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        base: String,
        label_suffix: Option<String>,
    ) -> Result<(), String> {
        let suffix = label_suffix
            .unwrap_or_else(|| "remote".into())
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let label = format!("events-{}", suffix);
        let url = format!("events.html?base={}", urlencoding::encode(&base));
        if app.get_webview_window(&label).is_none() {
            tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
                .title(format!("ARW — Events ({})", suffix))
                .inner_size(900.0, 700.0)
                .build()
                .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(&label) {
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
    pub fn open_logs_window_base<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        base: String,
        label_suffix: Option<String>,
    ) -> Result<(), String> {
        let suffix = label_suffix
            .unwrap_or_else(|| "remote".into())
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let label = format!("logs-{}", suffix);
        let url = format!("logs.html?base={}", urlencoding::encode(&base));
        if app.get_webview_window(&label).is_none() {
            tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
                .title(format!("ARW — Logs ({})", suffix))
                .inner_size(900.0, 700.0)
                .build()
                .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(&label) {
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
    pub fn open_models_window_base<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        base: String,
        label_suffix: Option<String>,
    ) -> Result<(), String> {
        let suffix = label_suffix
            .unwrap_or_else(|| "remote".into())
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>();
        let label = format!("models-{}", suffix);
        let url = format!("models.html?base={}", urlencoding::encode(&base));
        if app.get_webview_window(&label).is_none() {
            tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url.into()))
                .title(format!("ARW — Model Manager ({})", suffix))
                .inner_size(1000.0, 800.0)
                .build()
                .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(&label) {
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
    pub fn open_hub_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "hub";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("hub.html".into()),
            )
            .title("Agent Hub (ARW) — Project Hub")
            .inner_size(1100.0, 820.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_chat_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "chat";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("chat.html".into()),
            )
            .title("Agent Hub (ARW) — Chat")
            .inner_size(1000.0, 800.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub fn open_training_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "training";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("training.html".into()),
            )
            .title("Agent Hub (ARW) — Training Park")
            .inner_size(1100.0, 820.0)
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
        let svc_bin =
            locate_service_binary().ok_or_else(|| "service binary not found".to_string())?;
        let mut cmd = Command::new(svc_bin);
        cmd.env("ARW_PORT", format!("{}", effective_port(port)))
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
        _port: Option<u16>,
    ) -> Result<(), String> {
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

    fn split_cmdline(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_q: Option<char> = None;
        let mut esc = false;
        for ch in s.chars() {
            if esc {
                cur.push(ch);
                esc = false;
                continue;
            }
            if ch == '\\' {
                esc = true;
                continue;
            }
            match in_q {
                Some(q) if ch == q => {
                    in_q = None;
                }
                None if ch == '"' || ch == '\'' => {
                    in_q = Some(ch);
                }
                None if ch.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(cur.clone());
                        cur.clear();
                    }
                }
                _ => cur.push(ch),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    #[tauri::command]
    pub fn open_in_editor(path: String, editor_cmd: Option<String>) -> Result<(), String> {
        if path.len() > 4096 || path.chars().any(|c| c.is_control()) {
            return Err("invalid path".into());
        }
        // Prefer caller-provided editor command, then launcher prefs
        let provided = editor_cmd.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        });
        let from_prefs = if provided.is_none() {
            let prefs = load_prefs(Some("launcher"));
            prefs
                .get("editorCmd")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };
        if let Some(cmd) = provided.or(from_prefs) {
            // Tokenize with minimal quote support
            let mut parts = split_cmdline(&cmd);
            if parts.is_empty() {
                return open_path(path);
            }
            let prog = parts.remove(0);
            // Replace {path} placeholder or append path
            if parts.iter().any(|a| a.contains("{path}")) {
                for a in parts.iter_mut() {
                    *a = a.replace("{path}", &path);
                }
            } else {
                parts.push(path.clone());
            }
            let mut c = std::process::Command::new(prog);
            c.args(parts)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            c.spawn().map_err(|e| e.to_string())?;
            Ok(())
        } else {
            // Fallback to OS default handler
            open_path(path)
        }
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

    async fn admin_put_json(
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
            .put(service_url(path, port))
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())
    }

    // ---- Generic admin fetchers with explicit base+token (for remote connections) ----
    #[tauri::command]
    pub async fn admin_get_json_base(
        base: String,
        path: String,
        token: Option<String>,
    ) -> Result<Value, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap()
        });
        let mut headers = HeaderMap::new();
        if let Some(tok) = token.or_else(admin_token) {
            if let Ok(h) = HeaderValue::from_str(&tok) {
                headers.insert("X-ARW-Admin", h);
            }
        }
        let url = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let resp = client
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn admin_post_json_base(
        base: String,
        path: String,
        body: Value,
        token: Option<String>,
    ) -> Result<Value, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap()
        });
        let mut headers = HeaderMap::new();
        if let Some(tok) = token.or_else(admin_token) {
            if let Ok(h) = HeaderValue::from_str(&tok) {
                headers.insert("X-ARW-Admin", h);
            }
        }
        let url = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let resp = client
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn run_tool_admin(
        id: String,
        input: Value,
        port: Option<u16>,
    ) -> Result<Value, String> {
        let body = serde_json::json!({ "id": id, "input": input });
        let resp = admin_post_json("admin/tools/run", body, port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn projects_file_get(
        proj: String,
        path: String,
        port: Option<u16>,
    ) -> Result<Value, String> {
        let url = format!(
            "state/projects/{}/file?path={}",
            urlencoding::encode(&proj),
            urlencoding::encode(&path)
        );
        let resp = admin_get(&url, port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn projects_file_set(
        proj: String,
        path: String,
        content: String,
        prev_sha256: Option<String>,
        port: Option<u16>,
    ) -> Result<(), String> {
        let url = format!(
            "projects/{}/file?path={}",
            urlencoding::encode(&proj),
            urlencoding::encode(&path)
        );
        let body = serde_json::json!({ "content": content, "prev_sha256": prev_sha256 });
        let _ = admin_put_json(&url, body, port).await?;
        Ok(())
    }

    #[tauri::command]
    pub async fn projects_import(
        proj: String,
        dest: String,
        src_path: String,
        mode: Option<String>,
        port: Option<u16>,
    ) -> Result<Value, String> {
        let body = serde_json::json!({ "dest": dest, "src_path": src_path, "mode": mode });
        let path = format!("projects/{}/import", urlencoding::encode(&proj));
        let resp = admin_post_json(&path, body, port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_list(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_get("admin/models", port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_summary(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_get("admin/models/summary", port).await?;
        let env = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        let summary_raw = env.get("data").cloned().unwrap_or(env);
        let summary: ModelsSummary =
            serde_json::from_value(summary_raw).map_err(|e| e.to_string())?;
        serde_json::to_value(summary).map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn models_concurrency_get(
        port: Option<u16>,
    ) -> Result<ModelsConcurrencySnapshot, String> {
        let resp = admin_get("admin/models/concurrency", port).await?;
        let v = resp
            .json::<ModelsConcurrencySnapshot>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_concurrency_set(
        max: usize,
        block: Option<bool>,
        port: Option<u16>,
    ) -> Result<ModelsConcurrencySnapshot, String> {
        let body = serde_json::json!({"max": max, "block": block});
        let resp = admin_post_json("admin/models/concurrency", body, port).await?;
        let v = resp
            .json::<ModelsConcurrencySnapshot>()
            .await
            .map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn models_jobs(port: Option<u16>) -> Result<Value, String> {
        let resp = admin_get("admin/models/jobs", port).await?;
        let v = resp.json::<Value>().await.map_err(|e| e.to_string())?;
        Ok(v)
    }

    #[tauri::command]
    pub async fn state_models_hashes(
        limit: Option<usize>,
        offset: Option<usize>,
        provider: Option<String>,
        sort: Option<String>,
        order: Option<String>,
        port: Option<u16>,
    ) -> Result<Value, String> {
        let mut url = format!(
            "state/models_hashes?limit={}&offset={}",
            limit.unwrap_or(100),
            offset.unwrap_or(0)
        );
        if let Some(p) = provider.as_deref() {
            if !p.is_empty() {
                url.push_str(&format!("&provider={}", urlencoding::encode(p)));
            }
        }
        if let Some(s) = sort.as_deref() {
            if !s.is_empty() {
                url.push_str(&format!("&sort={}", urlencoding::encode(s)));
            }
        }
        if let Some(o) = order.as_deref() {
            if !o.is_empty() {
                url.push_str(&format!("&order={}", urlencoding::encode(o)));
            }
        }
        // public endpoint (no admin header)
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
        });
        let resp = client
            .get(service_url(&url, port))
            .send()
            .await
            .map_err(|e| e.to_string())?;
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
                active_window_bounds,
                open_debug_ui,
                open_debug_window,
                open_events_window,
                open_events_window_base,
                open_logs_window_base,
                open_models_window_base,
                admin_get_json_base,
                admin_post_json_base,
                open_logs_window,
                open_models_window,
                open_connections_window,
                open_hub_window,
                open_chat_window,
                open_training_window,
                models_summary,
                models_concurrency_get,
                models_concurrency_set,
                models_jobs,
                state_models_hashes,
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
                run_tool_admin,
                projects_import,
                projects_file_get,
                projects_file_set,
                start_service,
                stop_service,
                get_prefs,
                set_prefs,
                launcher_autostart_status,
                set_launcher_autostart,
                open_url,
                open_path,
                open_in_editor
            ])
            .build()
    }
}

// Re-export commands at crate root for existing callers
pub use cmds::*;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsConcurrencySnapshot {
    pub configured_max: u64,
    pub available_permits: u64,
    pub held_permits: u64,
    #[serde(default)]
    pub hard_cap: Option<u64>,
    #[serde(default)]
    pub pending_shrink: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsJobDestination {
    pub host: String,
    pub port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsJobSnapshot {
    pub model_id: String,
    pub job_id: String,
    pub url: String,
    pub corr_id: String,
    pub dest: ModelsJobDestination,
    pub started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsInflightEntry {
    pub sha256: String,
    pub primary: String,
    #[serde(default)]
    pub followers: Vec<String>,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsMetricsResponse {
    pub started: u64,
    pub queued: u64,
    pub admitted: u64,
    pub resumed: u64,
    pub canceled: u64,
    pub completed: u64,
    pub completed_cached: u64,
    pub errors: u64,
    pub bytes_total: u64,
    #[serde(default)]
    pub ewma_mbps: Option<f64>,
    pub preflight_ok: u64,
    pub preflight_denied: u64,
    pub preflight_skipped: u64,
    pub coalesced: u64,
    #[serde(default)]
    pub inflight: Vec<ModelsInflightEntry>,
    pub concurrency: ModelsConcurrencySnapshot,
    #[serde(default)]
    pub jobs: Vec<ModelsJobSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelsSummary {
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub concurrency: ModelsConcurrencySnapshot,
    #[serde(default)]
    pub metrics: ModelsMetricsResponse,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}
