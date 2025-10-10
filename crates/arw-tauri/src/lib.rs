use anyhow::Result;
use directories::ProjectDirs;
use once_cell::sync::OnceCell;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager}; // for get_webview_window on AppHandle
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

/// Shared state holder for managing a spawned service child process.
#[derive(Clone)]
pub struct ServiceState {
    inner: Arc<Mutex<Option<ServiceProcess>>>,
    recent: Arc<Mutex<VecDeque<LogRecord>>>,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            recent: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

const MAX_SERVICE_LOG_LINES: usize = 400;

type SharedLogWriter = Arc<Mutex<File>>;

struct ServiceProcess {
    child: Child,
    threads: Vec<std::thread::JoinHandle<()>>,
    log_path: Option<PathBuf>,
    writer: Option<SharedLogWriter>,
}

#[derive(Clone)]
struct LogRecord {
    stream: &'static str,
    line: String,
    timestamp: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LauncherSettings {
    pub default_port: u16,
    pub autostart_service: bool,
    pub notify_on_status: bool,
    pub launch_at_login: bool,
    pub base_override: Option<String>,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            default_port: default_port(),
            autostart_service: false,
            notify_on_status: true,
            launch_at_login: false,
            base_override: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LauncherWebView2Status {
    pub supported: bool,
    pub installed: bool,
    pub channel: Option<String>,
    pub detail: Option<String>,
}

impl Default for LauncherWebView2Status {
    fn default() -> Self {
        Self {
            supported: false,
            installed: true,
            channel: None,
            detail: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct LauncherSettingsBundle {
    pub settings: LauncherSettings,
    pub webview2: LauncherWebView2Status,
    pub logs_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherSettingsPayload {
    pub settings: LauncherSettings,
}

fn launcher_logs_dir(create_dirs: bool) -> Option<PathBuf> {
    let proj = ProjectDirs::from("org", "arw", "arw")?;
    let dir = proj.data_dir().join("logs");
    if create_dirs {
        std::fs::create_dir_all(&dir).ok()?;
    }
    Some(dir)
}

fn service_log_path(create_dirs: bool) -> Option<PathBuf> {
    let dir = launcher_logs_dir(create_dirs)?;
    Some(dir.join("launcher-service.log"))
}

fn push_recent(recent: &Arc<Mutex<VecDeque<LogRecord>>>, record: LogRecord) {
    let mut guard = recent.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.push_back(record);
    if guard.len() > MAX_SERVICE_LOG_LINES {
        guard.pop_front();
    }
}

fn log_record_to_json(record: &LogRecord) -> serde_json::Value {
    let ts = record
        .timestamp
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    json!({
        "stream": record.stream,
        "line": record.line,
        "timestamp": ts
    })
}

fn capture_line<R: tauri::Runtime + 'static>(
    app: &tauri::AppHandle<R>,
    stream: &'static str,
    line: &str,
    writer: Option<&SharedLogWriter>,
    recent: &Arc<Mutex<VecDeque<LogRecord>>>,
    log_path: Option<&Path>,
) {
    if let Some(writer) = writer {
        if let Ok(mut file) = writer.lock() {
            let _ = writeln!(file, "{line}");
        }
    }
    let timestamp = SystemTime::now();
    let record = LogRecord {
        stream,
        line: line.to_string(),
        timestamp,
    };
    push_recent(recent, record);
    let ts = timestamp
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let payload = json!({
        "stream": stream,
        "line": line,
        "timestamp": ts,
        "path": log_path.map(|p| p.display().to_string()),
    });
    let _ = app.emit("launcher://service-log", payload);
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

fn candidate_trial_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if seen.insert(path.clone()) {
            roots.push(path);
        }
    };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent().map(|p| p.to_path_buf()) {
            for _ in 0..6 {
                push(dir.clone());
                if !dir.pop() {
                    break;
                }
            }
        }
    }

    if let Ok(mut dir) = std::env::current_dir() {
        for _ in 0..6 {
            push(dir.clone());
            if !dir.pop() {
                break;
            }
        }
    }

    roots
}

fn load_launcher_settings_from_prefs() -> Map<String, Value> {
    match load_prefs(Some("launcher")) {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn persist_launcher_prefs(mut map: Map<String, Value>) -> Result<()> {
    // Remove nullish keys to keep the file tidy.
    map.retain(|_, value| !matches!(value, Value::Null));
    save_prefs(Some("launcher"), &Value::Object(map))
}

fn normalize_base_override(raw: Option<&str>) -> Option<String> {
    let trimmed = raw.unwrap_or_default().trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn load_launcher_settings<R: tauri::Runtime>(
    app: Option<&tauri::AppHandle<R>>,
) -> LauncherSettings {
    let mut out = LauncherSettings::default();
    let map = load_launcher_settings_from_prefs();
    if let Some(port) = map
        .get("port")
        .and_then(|v| v.as_u64())
        .and_then(|n| u16::try_from(n).ok())
    {
        out.default_port = port;
    }
    if let Some(b) = map.get("autostart").and_then(Value::as_bool) {
        out.autostart_service = b;
    }
    if let Some(b) = map.get("notifyOnStatus").and_then(Value::as_bool) {
        out.notify_on_status = b;
    }
    out.base_override = map
        .get("baseOverride")
        .and_then(Value::as_str)
        .and_then(|raw| normalize_base_override(Some(raw)));
    if let Some(app) = app {
        if let Ok(enabled) = app.autolaunch().is_enabled() {
            out.launch_at_login = enabled;
        }
    }
    out
}

fn write_launcher_settings<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    settings: &LauncherSettings,
) -> Result<(), String> {
    let mut map = load_launcher_settings_from_prefs();
    map.insert("port".into(), Value::from(settings.default_port as u64));
    map.insert("autostart".into(), Value::from(settings.autostart_service));
    map.insert(
        "notifyOnStatus".into(),
        Value::from(settings.notify_on_status),
    );
    match settings
        .base_override
        .as_ref()
        .and_then(|s| normalize_base_override(Some(s)))
    {
        Some(value) => {
            map.insert("baseOverride".into(), Value::from(value));
        }
        None => {
            map.remove("baseOverride");
        }
    }
    persist_launcher_prefs(map).map_err(|e| e.to_string())?;

    // Update launcher autostart (login) flag.
    let mgr = app.autolaunch();
    let currently_enabled = mgr.is_enabled().map_err(|e| e.to_string())?;
    if settings.launch_at_login && !currently_enabled {
        mgr.enable().map_err(|e| e.to_string())?;
    } else if !settings.launch_at_login && currently_enabled {
        mgr.disable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn launcher_logs_dir_string(create_dirs: bool) -> Option<String> {
    launcher_logs_dir(create_dirs).map(|p| p.to_string_lossy().to_string())
}

#[cfg(windows)]
fn detect_webview2_runtime() -> LauncherWebView2Status {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let subkeys = [
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_LOCAL_MACHINE,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
        (
            HKEY_CURRENT_USER,
            format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{GUID}"),
        ),
    ];

    for (hive, path) in subkeys {
        if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ) {
            if let Ok(version) = key.get_value::<String, _>("pv") {
                return LauncherWebView2Status {
                    supported: true,
                    installed: true,
                    channel: Some("Evergreen".into()),
                    detail: Some(version),
                };
            }
        }
    }

    LauncherWebView2Status {
        supported: true,
        installed: false,
        channel: None,
        detail: Some("Evergreen runtime not detected.".into()),
    }
}

#[cfg(not(windows))]
fn detect_webview2_runtime() -> LauncherWebView2Status {
    LauncherWebView2Status {
        supported: false,
        installed: true,
        channel: None,
        detail: Some("WebView2 runtime is only required on Windows.".into()),
    }
}

#[cfg(windows)]
async fn install_webview2_runtime_silent() -> Result<(), String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    const BOOTSTRAPPER_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
    let response = reqwest::get(BOOTSTRAPPER_URL)
        .await
        .map_err(|e| format!("failed to download WebView2 runtime: {e}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("failed to read WebView2 payload: {e}"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = std::env::temp_dir().join(format!("arw-webview2-{timestamp}.exe"));

    // Write bootstrapper
    tokio::task::spawn_blocking({
        let path = path.clone();
        let data = bytes.to_vec();
        move || std::fs::write(path, data)
    })
    .await
    .map_err(|e| format!("failed to spawn write task: {e}"))?
    .map_err(|e| format!("failed to write bootstrapper: {e}"))?;

    // Run installer silently
    let status = tokio::task::spawn_blocking({
        let path = path.clone();
        move || Command::new(&path).arg("/silent").arg("/install").status()
    })
    .await
    .map_err(|e| format!("failed to spawn installer: {e}"))?
    .map_err(|e| format!("failed to run installer: {e}"))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&path);

    if !status.success() {
        return Err(format!(
            "WebView2 installer exited with status {:?}",
            status.code()
        ));
    }

    Ok(())
}

#[cfg(not(windows))]
async fn install_webview2_runtime_silent() -> Result<(), String> {
    Err("WebView2 installation is only supported on Windows.".into())
}

fn capture_output(mut cmd: Command, label: &str) -> Result<String, String> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if output.status.success() {
                let mut combined = String::new();
                if !stdout.is_empty() {
                    combined.push_str(&stdout);
                }
                if !stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&stderr);
                }
                if combined.is_empty() {
                    combined.push_str(label);
                    combined.push_str(" completed");
                }
                Ok(combined)
            } else {
                let code = output
                    .status
                    .code()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".into());
                let mut detail = stdout;
                if !detail.is_empty() && !stderr.is_empty() {
                    detail.push_str(" | ");
                }
                detail.push_str(&stderr);
                if detail.is_empty() {
                    Err(format!("{} exited with code {}", label, code))
                } else {
                    Err(format!("{} exited with code {}: {}", label, code, detail))
                }
            }
        }
        Err(err) => Err(format!("{} failed: {}", label, err)),
    }
}

fn run_trials_preflight_script(root: &Path, script: &Path) -> Result<String, String> {
    let mut errors: Vec<String> = Vec::new();

    let script_label = script.display().to_string();

    let mut try_command = |cmd: Command, label: String| -> Option<String> {
        let mut command = cmd;
        command.current_dir(root);
        match capture_output(command, &label) {
            Ok(out) => Some(out),
            Err(err) => {
                errors.push(format!("{label}: {err}"));
                None
            }
        }
    };

    if let Some(out) = try_command(Command::new(script), script_label.clone()) {
        return Ok(out);
    }

    let ext = script
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    if ext.as_deref() == Some("ps1") {
        let make_ps_command = |shell: &str| {
            let mut cmd = Command::new(shell);
            cmd.arg("-NoLogo")
                .arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-File")
                .arg(script);
            cmd
        };

        let mut shells: Vec<&str> = Vec::new();
        if cfg!(windows) {
            shells.extend(["powershell.exe", "pwsh.exe", "powershell", "pwsh"]);
        } else {
            shells.extend(["pwsh", "pwsh.exe"]);
        }

        for shell in shells {
            if let Some(out) = try_command(
                make_ps_command(shell),
                format!("{} {}", shell, script.display()),
            ) {
                return Ok(out);
            }
        }
    } else {
        if let Some(out) = try_command(
            {
                let mut cmd = Command::new("bash");
                cmd.arg(script);
                cmd
            },
            format!("bash {}", script.display()),
        ) {
            return Ok(out);
        }

        if let Some(out) = try_command(
            {
                let mut cmd = Command::new("sh");
                cmd.arg(script);
                cmd
            },
            format!("sh {}", script.display()),
        ) {
            return Ok(out);
        }
    }

    if errors.is_empty() {
        Err("trial preflight helper unavailable".into())
    } else {
        Err(errors.join("; "))
    }
}

fn run_trials_preflight_sync() -> Result<String, String> {
    let mut errors = Vec::new();
    for root in candidate_trial_roots() {
        let mut scripts = Vec::new();
        if cfg!(windows) {
            scripts.push(root.join("scripts").join("trials_preflight.ps1"));
        }
        scripts.push(root.join("scripts").join("trials_preflight.sh"));

        for script in scripts {
            if script.exists() {
                match run_trials_preflight_script(&root, &script) {
                    Ok(out) => return Ok(out),
                    Err(err) => errors.push(format!("{}: {}", script.display(), err)),
                }
            }
        }

        if root.join("Justfile").exists() {
            let mut cmd = Command::new("just");
            cmd.arg("trials-preflight");
            cmd.current_dir(&root);
            match capture_output(cmd, "just trials-preflight") {
                Ok(out) => return Ok(out),
                Err(err) => errors.push(format!("just@{}: {}", root.display(), err)),
            }
        }
    }

    if errors.is_empty() {
        Err("trial preflight helpers not found".into())
    } else {
        Err(format!("trial preflight failed: {}", errors.join("; ")))
    }
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
    pub async fn check_service_health(
        base: Option<String>,
        port: Option<u16>,
    ) -> Result<bool, String> {
        static HTTP: OnceCell<reqwest::Client> = OnceCell::new();
        let client = HTTP.get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_millis(1200))
                .build()
                .unwrap()
        });
        let url = base
            .and_then(|raw| {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    reqwest::Url::parse(trimmed)
                        .map(|mut parsed| {
                            let existing = parsed.path().trim_end_matches('/');
                            let next = if existing.is_empty() || existing == "/" {
                                "/healthz".to_string()
                            } else {
                                format!("{}/healthz", existing)
                            };
                            parsed.set_path(&next);
                            parsed.set_query(None);
                            parsed.to_string()
                        })
                        .ok()
                }
            })
            .unwrap_or_else(|| service_url("healthz", port));
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
    pub fn open_mascot_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
        profile: Option<String>,
        character: Option<String>,
        quiet: Option<bool>,
        compact: Option<bool>,
    ) -> Result<(), String> {
        let window_label = label
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "mascot".to_string());
        let profile_ref = profile
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "global".to_string());
        let mut params = vec![format!("profile={}", urlencoding::encode(&profile_ref))];
        if let Some(ch) = character
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            params.push(format!("character={}", urlencoding::encode(ch)));
        }
        if quiet.unwrap_or(false) {
            params.push("quiet=1".to_string());
        }
        if compact.unwrap_or(false) {
            params.push("compact=1".to_string());
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        if app.get_webview_window(&window_label).is_none() {
            let title_suffix = if profile_ref != "global" {
                format!(" — {}", profile_ref)
            } else {
                String::new()
            };
            let mut builder = tauri::WebviewWindowBuilder::new(
                &app,
                &window_label,
                tauri::WebviewUrl::App(format!("mascot.html{}", query).into()),
            );
            builder = builder
                .title(format!("ARW — Mascot{}", title_suffix))
                .inner_size(220.0, 260.0)
                .decorations(false)
                .resizable(false)
                .always_on_top(true)
                .transparent(true)
                .skip_taskbar(true);
            builder.build().map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(&window_label) {
            let _ = w.set_focus();
        }
        // Nudge into view in case monitors changed
        let _ = ensure_window_in_view(app.clone(), Some(window_label));
        Ok(())
    }

    #[tauri::command]
    pub fn close_mascot_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
    ) -> Result<(), String> {
        let target = label
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "mascot".to_string());
        if let Some(window) = app.get_webview_window(&target) {
            window.close().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn ensure_window_in_view<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
    ) -> Result<(), String> {
        let id = label.unwrap_or_else(|| "mascot".into());
        let Some(w) = app.get_webview_window(&id) else {
            return Err("no window".into());
        };
        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;
        let win_w = i32::try_from(size.width).unwrap_or(200);
        let win_h = i32::try_from(size.height).unwrap_or(200);
        let mon = w
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())
            .ok_or_else(|| "no monitor".to_string())?;
        let mx = mon.position().x;
        let my = mon.position().y;
        let mw = i32::try_from(mon.size().width).unwrap_or(1920);
        let mh = i32::try_from(mon.size().height).unwrap_or(1080);
        let min_x = mx;
        let min_y = my;
        let max_x = mx + mw - win_w;
        let max_y = my + mh - win_h;
        let nx = pos.x.clamp(min_x, max_x);
        let ny = pos.y.clamp(min_y, max_y);
        if nx != pos.x || ny != pos.y {
            w.set_position(tauri::PhysicalPosition::new(nx, ny))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    #[tauri::command]
    pub fn snap_window_to_edges<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
        threshold: Option<i32>,
        margin: Option<i32>,
    ) -> Result<(), String> {
        let id = label.unwrap_or_else(|| "mascot".into());
        let Some(w) = app.get_webview_window(&id) else {
            return Err("no window".into());
        };

        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;
        let mon = w
            .current_monitor()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no monitor".to_string())?;

        let t = threshold.unwrap_or(24).max(0);
        let m = margin.unwrap_or(8).max(0);

        let win_w = i32::try_from(size.width).unwrap_or(200);
        let win_h = i32::try_from(size.height).unwrap_or(200);
        let mon_x = mon.position().x;
        let mon_y = mon.position().y;
        let mon_w = i32::try_from(mon.size().width).unwrap_or(1920);
        let mon_h = i32::try_from(mon.size().height).unwrap_or(1080);

        let left = mon_x + m;
        let right = mon_x + mon_w - win_w - m;
        let top = mon_y + m;
        let bottom = mon_y + mon_h - win_h - m;

        let mut x = pos.x;
        let mut y = pos.y;

        let dist_left = (pos.x - left).abs();
        let dist_right = (pos.x - right).abs();
        let dist_top = (pos.y - top).abs();
        let dist_bottom = (pos.y - bottom).abs();

        if dist_left.min(dist_right) <= t {
            x = if dist_left <= dist_right { left } else { right };
        }
        if dist_top.min(dist_bottom) <= t {
            y = if dist_top <= dist_bottom { top } else { bottom };
        }

        // Clamp inside monitor bounds regardless
        x = x.clamp(left, right);
        y = y.clamp(top, bottom);

        w.set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[tauri::command]
    pub fn snap_window_to_surfaces<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
        threshold: Option<i32>,
        margin: Option<i32>,
    ) -> Result<(), String> {
        let id = label.unwrap_or_else(|| "mascot".into());
        let Some(w) = app.get_webview_window(&id) else {
            return Err("no window".into());
        };

        let pos = w.outer_position().map_err(|e| e.to_string())?;
        let size = w.outer_size().map_err(|e| e.to_string())?;
        let win_w = i32::try_from(size.width).unwrap_or(200);
        let win_h = i32::try_from(size.height).unwrap_or(200);
        let t = threshold.unwrap_or(28).max(0);
        let m = margin.unwrap_or(8).max(0);

        // Gather other windows' bounds
        let mut best_delta = i32::MAX;
        let mut best_pos = (pos.x, pos.y);
        for (other_id, other) in app.webview_windows() {
            if other_id == id {
                continue;
            }
            if let (Ok(op), Ok(os)) = (other.outer_position(), other.outer_size()) {
                let ox = op.x;
                let oy = op.y;
                let ow = i32::try_from(os.width).unwrap_or(400);
                let oh = i32::try_from(os.height).unwrap_or(300);
                let left = ox;
                let right = ox + ow;
                let top = oy;
                let bottom = oy + oh;
                // Candidate positions: left, right, top, bottom surfaces
                let candidates: [(i32, i32); 4] = [
                    (left - win_w - m, pos.y),
                    (right + m, pos.y),
                    (pos.x, top - win_h - m),
                    (pos.x, bottom + m),
                ];
                for &(cx, cy) in &candidates {
                    let dx = (pos.x - cx).abs();
                    let dy = (pos.y - cy).abs();
                    let delta = dx.saturating_add(dy);
                    if delta < best_delta && (dx <= t || dy <= t) {
                        // snap; also clamp Y within other vertical range when snapping to left/right
                        let mut nx = cx;
                        let mut ny = cy;
                        if cx != pos.x {
                            // snapping horizontally
                            let min_y = top;
                            let max_y = bottom - win_h; // keep in vertical band
                            ny = ny.clamp(min_y, max_y);
                        } else {
                            // snapping vertically
                            let min_x = left;
                            let max_x = right - win_w;
                            nx = nx.clamp(min_x, max_x);
                        }
                        best_delta = delta;
                        best_pos = (nx, ny);
                    }
                }
            }
        }

        // If no window snap applied, fall back to edge snap
        if best_delta == i32::MAX {
            return snap_window_to_edges(app, Some(id), Some(t), Some(m));
        }

        w.set_position(tauri::PhysicalPosition::new(best_pos.0, best_pos.1))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[tauri::command]
    pub fn position_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
        anchor: String,
        margin: Option<i32>,
    ) -> Result<(), String> {
        let id = label.unwrap_or_else(|| "mascot".into());
        let Some(w) = app.get_webview_window(&id) else {
            return Err("no window".into());
        };
        let size = w.outer_size().map_err(|e| e.to_string())?;
        let win_w = i32::try_from(size.width).unwrap_or(200);
        let win_h = i32::try_from(size.height).unwrap_or(200);
        let m = margin.unwrap_or(8).max(0);
        let mon = w
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| app.primary_monitor().ok().flatten())
            .ok_or_else(|| "no monitor".to_string())?;
        let mx = mon.position().x;
        let my = mon.position().y;
        let mw = i32::try_from(mon.size().width).unwrap_or(1920);
        let mh = i32::try_from(mon.size().height).unwrap_or(1080);
        let left = mx + m;
        let top = my + m;
        let right = mx + mw - win_w - m;
        let bottom = my + mh - win_h - m;
        let anchor = anchor.to_lowercase();
        let (x, y) = match anchor.as_str() {
            "left" => (left, (top + bottom) / 2),
            "right" => (right, (top + bottom) / 2),
            "top" => ((left + right) / 2, top),
            "bottom" => ((left + right) / 2, bottom),
            "top-left" => (left, top),
            "top-right" => (right, top),
            "bottom-left" => (left, bottom),
            _ => (right, bottom), // default bottom-right
        };
        w.set_position(tauri::PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[tauri::command]
    pub fn smart_snap_window<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        label: Option<String>,
        pointer_x: Option<i32>,
        pointer_y: Option<i32>,
        margin: Option<i32>,
        preview_only: Option<bool>,
        snap_to_surfaces: Option<bool>,
    ) -> Result<String, String> {
        let id = label.clone().unwrap_or_else(|| "mascot".into());
        let Some(window) = app.get_webview_window(&id) else {
            return Err("no window".into());
        };

        let pos = window.outer_position().map_err(|e| e.to_string())?;
        let size = window.outer_size().map_err(|e| e.to_string())?;
        let win_w = i32::try_from(size.width).unwrap_or(200);
        let win_h = i32::try_from(size.height).unwrap_or(200);

        let margin = margin.unwrap_or(12).max(0);

        let mut monitors = app.available_monitors().map_err(|e| e.to_string())?;
        if monitors.is_empty() {
            if let Ok(mon) = window.current_monitor() {
                if let Some(mon) = mon {
                    monitors.push(mon);
                }
            }
        }
        if monitors.is_empty() {
            if let Ok(mon) = app.primary_monitor() {
                if let Some(mon) = mon {
                    monitors.push(mon);
                }
            }
        }

        let pointer = match (pointer_x, pointer_y) {
            (Some(x), Some(y)) => (x, y),
            _ => (pos.x + win_w / 2, pos.y + win_h / 2),
        };

        let mut monitor = monitors.iter().find(|m| {
            let rect = m.position();
            let size = m.size();
            pointer.0 >= rect.x
                && pointer.0 <= rect.x + i32::try_from(size.width).unwrap_or(0)
                && pointer.1 >= rect.y
                && pointer.1 <= rect.y + i32::try_from(size.height).unwrap_or(0)
        });

        if monitor.is_none() {
            monitor = monitors.iter().min_by_key(|m| {
                let rect = m.position();
                let size = m.size();
                let cx = rect.x + i32::try_from(size.width).unwrap_or(0) / 2;
                let cy = rect.y + i32::try_from(size.height).unwrap_or(0) / 2;
                let dx = pointer.0 - cx;
                let dy = pointer.1 - cy;
                dx.saturating_mul(dx) + dy.saturating_mul(dy)
            });
        }

        let monitor = monitor.ok_or_else(|| "no monitors".to_string())?;
        let rect = monitor.position();
        let size = monitor.size();
        let mon_w = i32::try_from(size.width).unwrap_or(1920);
        let mon_h = i32::try_from(size.height).unwrap_or(1080);

        let rel_x = ((pointer.0 - rect.x) as f32 / mon_w.max(1) as f32).clamp(0.0, 1.0);
        let rel_y = ((pointer.1 - rect.y) as f32 / mon_h.max(1) as f32).clamp(0.0, 1.0);

        let edge_threshold = 0.22f32;
        let corner_threshold = 0.18f32;

        let mut anchor = "bottom-right".to_string();

        if rel_x <= corner_threshold && rel_y <= corner_threshold {
            anchor = "top-left".into();
        } else if rel_x >= 1.0 - corner_threshold && rel_y <= corner_threshold {
            anchor = "top-right".into();
        } else if rel_x <= corner_threshold && rel_y >= 1.0 - corner_threshold {
            anchor = "bottom-left".into();
        } else if rel_x >= 1.0 - corner_threshold && rel_y >= 1.0 - corner_threshold {
            anchor = "bottom-right".into();
        } else if rel_y <= edge_threshold {
            anchor = "top".into();
        } else if rel_y >= 1.0 - edge_threshold {
            anchor = "bottom".into();
        } else if rel_x <= edge_threshold {
            anchor = "left".into();
        } else if rel_x >= 1.0 - edge_threshold {
            anchor = "right".into();
        }

        let x_range = (rect.x + margin, rect.x + mon_w - win_w - margin);
        let y_range = (rect.y + margin, rect.y + mon_h - win_h - margin);

        let (next_x, next_y) = match anchor.as_str() {
            "top-left" => (x_range.0, y_range.0),
            "top-right" => (x_range.1, y_range.0),
            "bottom-left" => (x_range.0, y_range.1),
            "left" => (
                x_range.0,
                (pointer.1 - win_h / 2).clamp(y_range.0, y_range.1),
            ),
            "right" => (
                x_range.1,
                (pointer.1 - win_h / 2).clamp(y_range.0, y_range.1),
            ),
            "top" => (
                (pointer.0 - win_w / 2).clamp(x_range.0, x_range.1),
                y_range.0,
            ),
            "bottom" => (
                (pointer.0 - win_w / 2).clamp(x_range.0, x_range.1),
                y_range.1,
            ),
            "bottom-right" => (x_range.1, y_range.1),
            _ => (
                (pointer.0 - win_w / 2).clamp(x_range.0, x_range.1),
                (pointer.1 - win_h / 2).clamp(y_range.0, y_range.1),
            ),
        };

        let preview = preview_only.unwrap_or(false);
        if !preview {
            window
                .set_position(tauri::PhysicalPosition::new(next_x, next_y))
                .map_err(|e| e.to_string())?;

            if snap_to_surfaces.unwrap_or(true) {
                let _ = snap_window_to_surfaces(app.clone(), label, Some(28), Some(margin));
            }
        }

        Ok(anchor)
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
    pub fn open_settings_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "settings";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("settings.html".into()),
            )
            .title("Agent Hub (ARW) — Launcher Settings")
            .inner_size(900.0, 720.0)
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
    pub fn open_trial_window<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
        let label = "trial";
        if app.get_webview_window(label).is_none() {
            tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("trial.html".into()),
            )
            .title("Agent Hub (ARW) — Experiment Control")
            .inner_size(1100.0, 800.0)
            .build()
            .map_err(|e| e.to_string())?;
        } else if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_focus();
        }
        Ok(())
    }

    #[tauri::command]
    pub async fn run_trials_preflight() -> Result<String, String> {
        tokio::task::spawn_blocking(run_trials_preflight_sync)
            .await
            .map_err(|err| err.to_string())?
    }

    #[tauri::command]
    pub fn start_service<R: tauri::Runtime + 'static>(
        app: tauri::AppHandle<R>,
        state: tauri::State<'_, ServiceState>,
        port: Option<u16>,
    ) -> Result<(), String> {
        {
            let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
            if let Some(process) = guard.as_mut() {
                if let Ok(None) = process.child.try_wait() {
                    return Ok(());
                }
            }
        }

        let svc_bin =
            locate_service_binary().ok_or_else(|| "service binary not found".to_string())?;
        let port_value = effective_port(port);
        let mut cmd = Command::new(svc_bin);
        cmd.env("ARW_PORT", format!("{port_value}"));
        if let Some(token) = admin_token() {
            cmd.env("ARW_ADMIN_TOKEN", token);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let log_path = service_log_path(true);
        let writer: Option<SharedLogWriter> = match log_path.as_ref() {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let _ = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(path);
                match OpenOptions::new().create(true).append(true).open(path) {
                    Ok(file) => Some(Arc::new(Mutex::new(file))),
                    Err(_) => None,
                }
            }
            None => None,
        };

        state.recent.lock().map_err(|e| e.to_string())?.clear();

        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let recent = state.recent.clone();
        let mut threads = Vec::new();

        if let Some(stdout) = stdout {
            let app_clone = app.clone();
            let recent_clone = recent.clone();
            let writer_clone = writer.clone();
            let log_path_clone = log_path.clone();
            threads.push(std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            capture_line(
                                &app_clone,
                                "stdout",
                                trimmed.as_str(),
                                writer_clone.as_ref(),
                                &recent_clone,
                                log_path_clone.as_deref(),
                            );
                        }
                        Err(_) => break,
                    }
                }
            }));
        }

        if let Some(stderr) = stderr {
            let app_clone = app.clone();
            let recent_clone = recent.clone();
            let writer_clone = writer.clone();
            let log_path_clone = log_path.clone();
            threads.push(std::thread::spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            capture_line(
                                &app_clone,
                                "stderr",
                                trimmed.as_str(),
                                writer_clone.as_ref(),
                                &recent_clone,
                                log_path_clone.as_deref(),
                            );
                        }
                        Err(_) => break,
                    }
                }
            }));
        }

        let process = ServiceProcess {
            child,
            threads,
            log_path: log_path.clone(),
            writer: writer.clone(),
        };
        *state.inner.lock().map_err(|e| e.to_string())? = Some(process);

        let marker = format!("launcher started service on port {port_value}");
        capture_line(
            &app,
            "launcher",
            marker.as_str(),
            writer.as_ref(),
            &state.recent,
            log_path.as_deref(),
        );

        Ok(())
    }

    #[tauri::command]
    pub async fn stop_service<R: tauri::Runtime + 'static>(
        app: tauri::AppHandle<R>,
        state: tauri::State<'_, ServiceState>,
        _port: Option<u16>,
    ) -> Result<(), String> {
        if let Some(mut process) = state.inner.lock().map_err(|e| e.to_string())?.take() {
            let _ = process.child.kill();
            let _ = process.child.wait();
            for handle in process.threads.drain(..) {
                let _ = handle.join();
            }
            capture_line(
                &app,
                "launcher",
                "launcher requested service stop",
                process.writer.as_ref(),
                &state.recent,
                process.log_path.as_deref(),
            );
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
    pub fn launcher_service_log_path() -> Result<Option<String>, String> {
        Ok(service_log_path(true).map(|p| p.display().to_string()))
    }

    #[tauri::command]
    pub fn launcher_recent_service_logs(
        state: tauri::State<'_, ServiceState>,
        limit: Option<usize>,
    ) -> Result<Vec<serde_json::Value>, String> {
        let max = limit
            .unwrap_or(MAX_SERVICE_LOG_LINES)
            .min(MAX_SERVICE_LOG_LINES);
        let guard = state.recent.lock().map_err(|e| e.to_string())?;
        let total = guard.len();
        let skip = total.saturating_sub(max);
        Ok(guard
            .iter()
            .skip(skip)
            .map(log_record_to_json)
            .collect::<Vec<_>>())
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
    pub async fn get_launcher_settings<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<LauncherSettingsBundle, String> {
        let settings = load_launcher_settings(Some(&app));
        Ok(LauncherSettingsBundle {
            settings,
            webview2: detect_webview2_runtime(),
            logs_dir: launcher_logs_dir_string(true),
        })
    }

    #[tauri::command]
    pub async fn save_launcher_settings<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
        payload: LauncherSettingsPayload,
    ) -> Result<LauncherSettingsBundle, String> {
        let mut settings = payload.settings;
        if settings.default_port == 0 {
            settings.default_port = default_port();
        }
        write_launcher_settings(&app, &settings)?;
        let bundle = LauncherSettingsBundle {
            settings: load_launcher_settings(Some(&app)),
            webview2: detect_webview2_runtime(),
            logs_dir: launcher_logs_dir_string(true),
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or_default();
        let _ = app.emit(
            "launcher://settings-updated",
            json!({
                "settings": bundle.settings,
                "webview2": bundle.webview2,
                "logsDir": bundle.logs_dir,
                "timestamp": timestamp
            }),
        );
        Ok(bundle)
    }

    #[tauri::command]
    pub async fn install_webview2_runtime<R: tauri::Runtime>(
        app: tauri::AppHandle<R>,
    ) -> Result<LauncherWebView2Status, String> {
        install_webview2_runtime_silent().await?;
        let status = detect_webview2_runtime();
        let _ = app.emit(
            "launcher://webview2-updated",
            json!({
                "status": status,
                "timestamp": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or_default()
            }),
        );
        Ok(status)
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
                open_settings_window,
                open_hub_window,
                open_chat_window,
                open_training_window,
                open_trial_window,
                open_mascot_window,
                close_mascot_window,
                snap_window_to_edges,
                snap_window_to_surfaces,
                position_window,
                smart_snap_window,
                run_trials_preflight,
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
                launcher_service_log_path,
                launcher_recent_service_logs,
                launcher_autostart_status,
                set_launcher_autostart,
                get_launcher_settings,
                save_launcher_settings,
                install_webview2_runtime,
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
