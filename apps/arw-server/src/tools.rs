use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arw_topics as topics;
use arw_wasi::{ToolHost, WasiError};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{Datelike, Utc};
use image::{self, imageops, DynamicImage, ImageOutputFormat, RgbaImage};
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::task::spawn_blocking;
use tokio::time::sleep;

use crate::tool_cache::StoreOutcome;
use crate::{util, AppState};

static GUARD_RETRIES: AtomicU64 = AtomicU64::new(0);
static GUARD_HTTP_ERRORS: AtomicU64 = AtomicU64::new(0);
static GUARD_CB_TRIPS: AtomicU64 = AtomicU64::new(0);

struct CircuitBreaker {
    fail_count: AtomicU64,
    open_until_ms: AtomicU64,
}

static CB: OnceCell<CircuitBreaker> = OnceCell::new();

static RE_EMAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}").expect("email regex")
});
static RE_AWS: Lazy<Regex> = Lazy::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws regex"));
static RE_GAPI: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"AIza[0-9A-Za-z\-_]{35}").expect("gapi regex"));
static RE_SLACK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"xox[baprs]-[0-9A-Za-z-]{10,}").expect("slack regex"));
static RE_URL: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\\s)]+").expect("url regex"));

#[derive(Debug)]
pub enum ToolError {
    Unsupported(String),
    Invalid(String),
    Runtime(String),
    Denied {
        reason: String,
        dest_host: Option<String>,
        dest_port: Option<i64>,
        protocol: Option<String>,
    },
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::Unsupported(id) => write!(f, "unsupported tool: {}", id),
            ToolError::Invalid(msg) => write!(f, "invalid request: {}", msg),
            ToolError::Runtime(msg) => write!(f, "runtime error: {}", msg),
            ToolError::Denied {
                reason,
                dest_host,
                dest_port,
                protocol,
            } => {
                write!(f, "denied: {}", reason)?;
                if let Some(host) = dest_host {
                    write!(f, " host={}", host)?;
                }
                if let Some(port) = dest_port {
                    write!(f, " port={}", port)?;
                }
                if let Some(proto) = protocol {
                    write!(f, " proto={}", proto)?;
                }
                Ok(())
            }
        }
    }
}

pub async fn run_tool(state: &AppState, id: &str, input: Value) -> Result<Value, ToolError> {
    let start = Instant::now();
    let bus = state.bus();
    let cache = state.tool_cache();
    let cacheable = cache.enabled() && cache.is_cacheable(id);
    let cache_key = cacheable.then(|| cache.action_key(id, &input));

    if let Some(ref key) = cache_key {
        if let Some(hit) = cache.lookup(key).await {
            metrics::counter!("arw_tools_cache_hits", 1);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let mut cache_evt = json!({
                "tool": id,
                "outcome": "hit",
                "elapsed_ms": elapsed_ms,
                "key": key,
                "digest": hit.digest,
                "cached": true,
                "age_secs": hit.age_secs,
            });
            ensure_corr(&mut cache_evt);
            bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);

            let mut payload = json!({"id": id, "output": hit.value.clone()});
            ensure_corr(&mut payload);
            bus.publish(topics::TOPIC_TOOL_RAN, &payload);
            if id == "ui.screenshot.capture" {
                let mut shot = hit.value.clone();
                ensure_corr(&mut shot);
                bus.publish(topics::TOPIC_SCREENSHOTS_CAPTURED, &shot);
            }
            return Ok(hit.value);
        }
    }

    let output = run_tool_inner(state, id, &input).await?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    if let Some(ref key) = cache_key {
        match cache.store(key, &output).await {
            Some(StoreOutcome {
                digest,
                cached: true,
            }) => {
                metrics::counter!("arw_tools_cache_miss", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "miss",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": true,
                    "age_secs": Value::Null,
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            Some(StoreOutcome {
                digest,
                cached: false,
            }) => {
                metrics::counter!("arw_tools_cache_error", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "error",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": false,
                    "reason": "store_failed",
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            None => {
                metrics::counter!("arw_tools_cache_error", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "error",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "cached": false,
                    "reason": "serialize_failed",
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
        }
    } else if cache.enabled() {
        cache.record_bypass();
        metrics::counter!("arw_tools_cache_bypass", 1);
        let mut cache_evt = json!({
            "tool": id,
            "outcome": "not_cacheable",
            "elapsed_ms": elapsed_ms,
            "cached": false,
            "reason": "not_cacheable",
        });
        ensure_corr(&mut cache_evt);
        bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
    }

    let mut payload = json!({"id": id, "output": output.clone()});
    ensure_corr(&mut payload);
    bus.publish(topics::TOPIC_TOOL_RAN, &payload);
    if id == "ui.screenshot.capture" {
        let mut shot = output.clone();
        ensure_corr(&mut shot);
        bus.publish(topics::TOPIC_SCREENSHOTS_CAPTURED, &shot);
    }

    Ok(output)
}

async fn run_tool_inner(state: &AppState, id: &str, input: &Value) -> Result<Value, ToolError> {
    match id {
        "ui.screenshot.capture" => {
            let input = input.clone();
            let value = spawn_blocking(move || screenshot_capture(&input))
                .await
                .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))??;
            Ok(value)
        }
        "ui.screenshot.annotate_burn" => {
            let input = input.clone();
            let value = spawn_blocking(move || screenshot_annotate(&input))
                .await
                .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))??;
            Ok(value)
        }
        "ui.screenshot.ocr" => {
            let input = input.clone();
            let value = spawn_blocking(move || screenshot_ocr(&input))
                .await
                .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))??;
            Ok(value)
        }
        "guardrails.check" => run_guardrails(input).await,
        "demo.echo" => Ok(json!({"echo": input.clone()})),
        "introspect.tools" => serde_json::to_value(arw_core::introspect_tools())
            .map_err(|e| ToolError::Runtime(e.to_string())),
        _ => run_host_tool(state.host(), id, input).await,
    }
}

async fn run_host_tool(
    host: std::sync::Arc<dyn ToolHost>,
    id: &str,
    input: &Value,
) -> Result<Value, ToolError> {
    host.run_tool(id, input).await.map_err(|err| match err {
        WasiError::Unsupported(name) => ToolError::Unsupported(name),
        WasiError::Runtime(msg) => ToolError::Runtime(msg),
        WasiError::Denied {
            reason,
            dest_host,
            dest_port,
            protocol,
        } => ToolError::Denied {
            reason,
            dest_host,
            dest_port,
            protocol,
        },
    })
}

async fn run_guardrails(input: &Value) -> Result<Value, ToolError> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'text'".into()))?;

    if let Some(base) = std::env::var("ARW_GUARDRAILS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        let cb = CB.get_or_init(|| CircuitBreaker {
            fail_count: AtomicU64::new(0),
            open_until_ms: AtomicU64::new(0),
        });
        let now_ms = now_millis();
        let open_until = cb.open_until_ms.load(Ordering::Relaxed);
        if now_ms >= open_until {
            if let Some(remote) = guardrails_remote(text, input, &base, cb).await? {
                return Ok(remote);
            }
        }
    }

    guardrails_local(text)
}

async fn guardrails_remote(
    text: &str,
    input: &Value,
    base: &str,
    cb: &CircuitBreaker,
) -> Result<Option<Value>, ToolError> {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
    let timeout_ms: u64 = std::env::var("ARW_GUARDRAILS_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3_000);
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_millis(timeout_ms.max(1)))
            .build()
            .expect("guardrails client")
    });

    let url = format!("{}/check", base.trim_end_matches('/'));
    let mut body = json!({"text": text});
    if let Some(policy) = input.get("policy") {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("policy".into(), policy.clone());
        }
    }
    if let Some(rules) = input.get("rules") {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("rules".into(), rules.clone());
        }
    }

    let max_retries: u32 = std::env::var("ARW_GUARDRAILS_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

    let mut attempt: u32 = 0;
    let mut out_opt: Option<Value> = None;
    while attempt <= max_retries {
        let resp = client.post(&url).json(&body).send().await;
        match resp {
            Ok(rsp) if rsp.status().is_success() => match rsp.json::<Value>().await {
                Ok(v) => {
                    out_opt = Some(json!({
                        "ok": v.get("ok").and_then(|b| b.as_bool()).unwrap_or(true),
                        "score": v.get("score").cloned().unwrap_or(json!(0.0)),
                        "issues": v.get("issues").cloned().unwrap_or(json!([])),
                        "suggestions": v.get("suggestions").cloned().unwrap_or(json!([])),
                    }));
                    cb.fail_count.store(0, Ordering::Relaxed);
                    cb.open_until_ms.store(0, Ordering::Relaxed);
                    break;
                }
                Err(_) => {
                    GUARD_HTTP_ERRORS.fetch_add(1, Ordering::Relaxed);
                }
            },
            _ => {
                GUARD_HTTP_ERRORS.fetch_add(1, Ordering::Relaxed);
            }
        }
        attempt += 1;
        if attempt <= max_retries {
            GUARD_RETRIES.fetch_add(1, Ordering::Relaxed);
            let base_delay = 100u64 * (1u64 << (attempt - 1).min(4));
            let jitter = random_jitter(base_delay / 2);
            sleep(Duration::from_millis(base_delay + jitter)).await;
        }
    }

    if let Some(out) = out_opt {
        return Ok(Some(out));
    }

    let failures = cb.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
    let threshold: u64 = std::env::var("ARW_GUARDRAILS_CB_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .max(1);
    if failures >= threshold {
        let cooldown_ms: u64 = std::env::var("ARW_GUARDRAILS_CB_COOLDOWN_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30_000);
        cb.open_until_ms
            .store(now_millis().saturating_add(cooldown_ms), Ordering::Relaxed);
        cb.fail_count.store(0, Ordering::Relaxed);
        GUARD_CB_TRIPS.fetch_add(1, Ordering::Relaxed);
    }

    Ok(None)
}

fn guardrails_local(text: &str) -> Result<Value, ToolError> {
    #[derive(Serialize)]
    struct Issue {
        code: String,
        severity: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<(usize, usize)>,
    }

    let mut issues: Vec<Issue> = Vec::new();
    for m in RE_EMAIL.find_iter(text) {
        issues.push(Issue {
            code: "pii.email".into(),
            severity: "medium".into(),
            message: "Email address detected".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_AWS.find_iter(text) {
        issues.push(Issue {
            code: "secret.aws_access_key".into(),
            severity: "high".into(),
            message: "AWS access key pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_GAPI.find_iter(text) {
        issues.push(Issue {
            code: "secret.gcp_api_key".into(),
            severity: "high".into(),
            message: "Google API key pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_SLACK.find_iter(text) {
        issues.push(Issue {
            code: "secret.slack_token".into(),
            severity: "high".into(),
            message: "Slack token pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }

    let allowlist: Vec<String> = std::env::var("ARW_GUARDRAILS_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    for m in RE_URL.find_iter(text) {
        let url = m.as_str();
        if let Ok(parsed) = url::Url::parse(url) {
            let host = parsed.host_str().unwrap_or("").to_lowercase();
            if !allowlist.is_empty()
                && !allowlist
                    .iter()
                    .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            {
                issues.push(Issue {
                    code: "egress.unlisted_host".into(),
                    severity: "medium".into(),
                    message: format!("URL host not in allowlist: {}", host),
                    span: Some((m.start(), m.end())),
                });
            }
        }
    }

    let markers = [
        "ignore previous",
        "disregard prior",
        "override instructions",
        "exfiltrate",
    ];
    let lower = text.to_ascii_lowercase();
    for pat in markers.iter() {
        if let Some(pos) = lower.find(pat) {
            issues.push(Issue {
                code: "prompt_injection.marker".into(),
                severity: "medium".into(),
                message: format!("Suspicious instruction: '{}'", pat),
                span: Some((pos, pos + pat.len())),
            });
        }
    }

    let mut score = 0.0;
    for issue in &issues {
        score += match issue.severity.as_str() {
            "high" => 3.0,
            "medium" => 1.0,
            _ => 0.5,
        };
    }
    let ok = issues.iter().all(|i| i.severity != "high");
    Ok(json!({
        "ok": ok,
        "score": score,
        "issues": issues,
        "suggestions": []
    }))
}

pub fn guardrails_metrics_value() -> Value {
    let cb = CB.get_or_init(|| CircuitBreaker {
        fail_count: AtomicU64::new(0),
        open_until_ms: AtomicU64::new(0),
    });
    let now = now_millis();
    let open_until = cb.open_until_ms.load(Ordering::Relaxed);
    json!({
        "retries": GUARD_RETRIES.load(Ordering::Relaxed),
        "http_errors": GUARD_HTTP_ERRORS.load(Ordering::Relaxed),
        "cb_trips": GUARD_CB_TRIPS.load(Ordering::Relaxed),
        "cb_open": if now < open_until { 1 } else { 0 },
        "cb_open_until_ms": open_until,
    })
}

pub fn ensure_corr(value: &mut Value) {
    if let Value::Object(map) = value {
        if !map.contains_key("corr_id") {
            map.insert(
                "corr_id".into(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
        }
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn random_jitter(cap: u64) -> u64 {
    if cap == 0 {
        return 0;
    }
    now_millis() % cap.max(1)
}

fn screenshot_base_dir() -> PathBuf {
    util::state_dir().join("screenshots")
}

fn safe_scope_fragment(scope: &str) -> String {
    scope
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn parse_region(scope: &str) -> Result<Option<(i32, i32, u32, u32)>, ToolError> {
    if let Some(rest) = scope.strip_prefix("region:") {
        let parts: Vec<i32> = rest
            .split(',')
            .filter_map(|t| t.trim().parse::<i32>().ok())
            .collect();
        if parts.len() != 4 {
            return Err(ToolError::Invalid(
                "scope region must be x,y,w,h".to_string(),
            ));
        }
        let (x, y, w, h) = (parts[0], parts[1], parts[2], parts[3]);
        if w <= 0 || h <= 0 {
            return Err(ToolError::Invalid("region dimensions must be > 0".into()));
        }
        return Ok(Some((x, y, w as u32, h as u32)));
    }
    Ok(None)
}

fn capture_rgba(scope: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let screens = screenshots::Screen::all().map_err(|e| e.to_string())?;
    let screen = if let Some(rest) = scope.strip_prefix("display:") {
        let idx: usize = rest.parse().unwrap_or(0);
        screens
            .get(idx)
            .cloned()
            .ok_or_else(|| "display index out of range".to_string())?
    } else {
        screenshots::Screen::from_point(0, 0)
            .unwrap_or_else(|_| screens.into_iter().next().expect("no screens"))
    };

    let img = if let Some((x, y, w, h)) = parse_region(scope).map_err(|e| e.to_string())? {
        screen.capture_area(x, y, w, h).map_err(|e| e.to_string())?
    } else {
        screen.capture().map_err(|e| e.to_string())?
    };

    let width = img.width();
    let height = img.height();
    let buf = img.into_raw();
    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for chunk in buf.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let b = chunk[0];
        let g = chunk[1];
        let r = chunk[2];
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    Ok((width, height, rgba))
}

fn screenshot_capture(input: &Value) -> Result<Value, ToolError> {
    let scope = input
        .get("scope")
        .and_then(|v| v.as_str())
        .unwrap_or("screen");
    let fmt = input
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let ext = if fmt == "jpg" || fmt == "jpeg" {
        "jpg"
    } else {
        "png"
    };
    let downscale = input
        .get("downscale")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let (width, height, rgba, cap_err) = match capture_rgba(scope) {
        Ok((w, h, data)) => (w, h, Some(data), None),
        Err(err) => (1, 1, None, Some(err)),
    };

    let now = Utc::now();
    let dir = screenshot_base_dir()
        .join(format!("{:04}", now.year()))
        .join(format!("{:02}", now.month()))
        .join(format!("{:02}", now.day()));
    fs::create_dir_all(&dir).map_err(|e| ToolError::Runtime(e.to_string()))?;
    let fname = format!(
        "{}-{}.{}",
        now.format("%H%M%S%3f"),
        safe_scope_fragment(scope),
        ext
    );
    let path = dir.join(fname);

    let mut preview_b64: Option<String> = None;
    if let Some(data) = rgba {
        if let Err(err) = image::save_buffer(&path, &data, width, height, image::ColorType::Rgba8) {
            return Err(ToolError::Runtime(err.to_string()));
        }
        if let Some(maxw) = downscale {
            if width > 0 && height > 0 {
                let img = RgbaImage::from_raw(width, height, data)
                    .ok_or_else(|| ToolError::Runtime("invalid buffer".into()))?;
                let ratio = (height as f32) / (width as f32);
                let new_w = maxw.max(1);
                let new_h = ((new_w as f32) * ratio).round().max(1.0) as u32;
                let resized =
                    imageops::resize(&img, new_w, new_h.max(1), imageops::FilterType::Triangle);
                let mut bytes: Vec<u8> = Vec::new();
                let dynimg = DynamicImage::ImageRgba8(resized);
                dynimg
                    .write_to(
                        &mut std::io::Cursor::new(&mut bytes),
                        ImageOutputFormat::Png,
                    )
                    .map_err(|e| ToolError::Runtime(e.to_string()))?;
                preview_b64 = Some(format!(
                    "data:image/png;base64,{}",
                    BASE64_STANDARD.encode(&bytes)
                ));
            }
        }
    } else {
        tracing::warn!("screenshot capture failed: {}", cap_err.unwrap_or_default());
        File::create(&path)
            .and_then(|mut f| f.flush())
            .map_err(|e| ToolError::Runtime(e.to_string()))?;
    }

    let mut out = json!({
        "path": path.to_string_lossy(),
        "width": width,
        "height": height,
    });
    if let Some(b64) = preview_b64 {
        out["preview_b64"] = json!(b64);
    }
    Ok(out)
}

fn screenshot_annotate(input: &Value) -> Result<Value, ToolError> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'path'".into()))?;
    let ann = input
        .get("annotate")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let downscale = input
        .get("downscale")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let img_dyn = image::open(path).map_err(|e| ToolError::Runtime(e.to_string()))?;
    let mut img = img_dyn.to_rgba8();
    let (width, height) = img.dimensions();

    for it in ann.iter() {
        let x = it.get("x").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let y = it.get("y").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let w = it.get("w").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let h = it.get("h").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let blur = it.get("blur").and_then(|v| v.as_bool()).unwrap_or(true);

        let x2 = x.min(width.saturating_sub(1));
        let y2 = y.min(height.saturating_sub(1));
        let w2 = w.min(width.saturating_sub(x2));
        let h2 = h.min(height.saturating_sub(y2));
        if w2 == 0 || h2 == 0 {
            continue;
        }
        if blur {
            let sub = imageops::crop(&mut img, x2, y2, w2, h2).to_image();
            let blurred = imageops::blur(&sub, 3.0);
            imageops::overlay(&mut img, &blurred, x2 as i64, y2 as i64);
        }
        let teal = image::Rgba([27, 179, 163, 255]);
        for dx in x2..(x2 + w2) {
            for t in 0..2 {
                if y2 + t < height {
                    img.put_pixel(dx, y2 + t, teal);
                }
                if y2 + h2 > t {
                    let yy = (y2 + h2 - 1).saturating_sub(t);
                    img.put_pixel(dx, yy, teal);
                }
            }
        }
        for dy in y2..(y2 + h2) {
            for t in 0..2 {
                if x2 + t < width {
                    img.put_pixel(x2 + t, dy, teal);
                }
                if x2 + w2 > t {
                    let xx = (x2 + w2 - 1).saturating_sub(t);
                    img.put_pixel(xx, dy, teal);
                }
            }
        }
    }

    let src = Path::new(path);
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("png");
    let ann_path = src.with_file_name(format!("{}.ann.{}", stem, ext));
    img.save(&ann_path)
        .map_err(|e| ToolError::Runtime(e.to_string()))?;

    let ann_sidecar = src.with_file_name(format!("{}.ann.json", stem));
    let sidecar = json!({"annotate": ann});
    fs::write(
        &ann_sidecar,
        serde_json::to_vec_pretty(&sidecar).unwrap_or_default(),
    )
    .map_err(|e| ToolError::Runtime(e.to_string()))?;

    let mut preview_b64 = None;
    if let Some(maxw) = downscale {
        if width > 0 && height > 0 {
            let ratio = (height as f32) / (width as f32);
            let new_w = maxw.max(1);
            let new_h = ((new_w as f32) * ratio).round().max(1.0) as u32;
            let resized =
                imageops::resize(&img, new_w, new_h.max(1), imageops::FilterType::Triangle);
            let mut bytes: Vec<u8> = Vec::new();
            let dynimg = DynamicImage::ImageRgba8(resized);
            dynimg
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    ImageOutputFormat::Png,
                )
                .map_err(|e| ToolError::Runtime(e.to_string()))?;
            preview_b64 = Some(format!(
                "data:image/png;base64,{}",
                BASE64_STANDARD.encode(&bytes)
            ));
        }
    }

    let mut out = json!({
        "path": ann_path.to_string_lossy(),
        "ann_path": ann_sidecar.to_string_lossy(),
        "width": width,
        "height": height,
    });
    if let Some(b64) = preview_b64 {
        out["preview_b64"] = json!(b64);
    }
    Ok(out)
}

fn screenshot_ocr(input: &Value) -> Result<Value, ToolError> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'path'".into()))?;
    let text = ocr_image_text(path)?;
    Ok(json!({"text": text, "blocks": []}))
}

#[cfg(feature = "ocr_tesseract")]
fn ocr_image_text(path: &str) -> Result<String, ToolError> {
    let mut lt =
        leptess::LepTess::new(None, "eng").map_err(|e| ToolError::Runtime(e.to_string()))?;
    lt.set_image(path);
    lt.get_utf8_text()
        .map_err(|e| ToolError::Runtime(e.to_string()))
}

#[cfg(not(feature = "ocr_tesseract"))]
fn ocr_image_text(_path: &str) -> Result<String, ToolError> {
    Err(ToolError::Runtime(
        "ocr feature not compiled (enable arw-server/ocr_tesseract)".into(),
    ))
}
