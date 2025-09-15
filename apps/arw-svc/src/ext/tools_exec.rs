use base64::Engine;
use chrono::Datelike;
use moka::sync::Cache;
use once_cell::sync::OnceCell;
use serde_json::{json, Map, Value};
use sha2::Digest as _;
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Condvar, Mutex, RwLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH}; // for base64::engine::general_purpose::STANDARD.encode

struct Entry {
    summary: &'static str,
    exec: fn(&Value) -> Result<Value, String>,
}

static REG: OnceCell<RwLock<HashMap<&'static str, Entry>>> = OnceCell::new();

fn reg() -> &'static RwLock<HashMap<&'static str, Entry>> {
    REG.get_or_init(|| {
        let mut map: HashMap<&'static str, Entry> = HashMap::new();
        // Built-in examples
        map.insert(
            "math.add",
            Entry {
                summary:
                    "Add two numbers: input {\"a\": number, \"b\": number} -> {\"sum\": number}",
                exec: |input| {
                    let a = input
                        .get("a")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'a'")?;
                    let b = input
                        .get("b")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'b'")?;
                    Ok(json!({"sum": a + b}))
                },
            },
        );
        map.insert(
            "time.now",
            Entry {
                summary: "UTC time in ms: input {} -> {\"now_ms\": number}",
                exec: |_input| {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis() as i64;
                    Ok(json!({"now_ms": now}))
                },
            },
        );
        map.insert(
            "ui.screenshot.capture",
            Entry {
                summary: "Capture screenshot: input {scope, format?, downscale?} -> {path,width,height,preview_b64?}",
                exec: |input| {
                    let scope = input.get("scope").and_then(|v| v.as_str()).unwrap_or("screen");
                    let fmt = input
                        .get("format")
                        .and_then(|v| v.as_str())
                        .unwrap_or("png")
                        .to_ascii_lowercase();
                    let downscale = input.get("downscale").and_then(|v| v.as_u64()).map(|n| n as u32);
                    let ext = if fmt == "jpg" || fmt == "jpeg" { "jpg" } else { "png" };

                    // Capture using the `screenshots` crate (best-effort; fallback to stub)
                    let mut width: u32 = 1;
                    let mut height: u32 = 1;
                    let mut rgba: Vec<u8> = Vec::new();
                    let cap_res = || -> Result<(), String> {
                        let screens = screenshots::Screen::all().map_err(|e| e.to_string())?;
                        let screen = if let Some(rest) = scope.strip_prefix("display:") {
                            let idx: usize = rest.parse().unwrap_or(0);
                            screens.get(idx).cloned().ok_or_else(|| "display index out of range".to_string())?
                        } else {
                            // pick screen containing origin (0,0) or fallback to first
                            screenshots::Screen::from_point(0, 0)
                                .unwrap_or_else(|_| screens.into_iter().next().expect("no screens"))
                        };
                        let img = if let Some(rest) = scope.strip_prefix("region:") {
                            // parse x,y,w,h
                            let parts: Vec<i32> = rest
                                .split(',')
                                .filter_map(|t| t.trim().parse::<i32>().ok())
                                .collect();
                            if parts.len() != 4 {
                                return Err("region must be x,y,w,h".to_string());
                            }
                            let (x, y, w, h) = (parts[0], parts[1], parts[2], parts[3]);
                            if w <= 0 || h <= 0 { return Err("invalid region dims".into()); }
                            screen
                                .capture_area(x, y, w as u32, h as u32)
                                .map_err(|e| e.to_string())?
                        } else {
                            screen.capture().map_err(|e| e.to_string())?
                        };
                        width = img.width();
                        height = img.height();
                        let buf = img.into_raw();
                        // Convert BGRA -> RGBA
                        rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
                        for chunk in buf.chunks(4) {
                            if chunk.len() < 4 { break; }
                            let b = chunk[0];
                            let g = chunk[1];
                            let r = chunk[2];
                            let a = 255u8; // screenshots' alpha may be undefined; force opaque
                            rgba.extend_from_slice(&[r, g, b, a]);
                        }
                        Ok(())
                    }();

                    let now = chrono::Utc::now();
                    let dir = super::paths::screenshots_dir()
                        .join(format!("{:04}", now.year()))
                        .join(format!("{:02}", now.month()))
                        .join(format!("{:02}", now.day()));
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let safe_scope = scope.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>();
                    let fname = format!("{}-{}.{}", now.format("%H%M%S%3f"), safe_scope, ext);
                    let path = dir.join(fname);

                    let mut preview_b64: Option<String> = None;
                    match cap_res {
                        Ok(()) => {
                            // Save full image
                            image::save_buffer(
                                &path,
                                &rgba,
                                width,
                                height,
                                image::ColorType::Rgba8,
                            )
                            .map_err(|e| e.to_string())?;
                            // Preview (optional)
                            if let Some(maxw) = downscale {
                                let img = image::RgbaImage::from_raw(width, height, rgba.clone())
                                    .ok_or_else(|| "invalid buffer".to_string())?;
                                let ratio = (height as f32) / (width as f32);
                                let new_w = maxw.max(1);
                                let new_h = ((new_w as f32) * ratio).round().max(1.0) as u32;
                                let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Triangle);
                                let mut bytes: Vec<u8> = Vec::new();
                                let dynimg = image::DynamicImage::ImageRgba8(resized);
                                dynimg
                                    .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageOutputFormat::Png)
                                    .map_err(|e| e.to_string())?;
                                preview_b64 = Some(format!(
                                    "data:image/png;base64,{}",
                                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                                ));
                            }
                        }
                        Err(_e) => {
                            // Fallback: create empty file to indicate attempt
                            let mut f = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                            f.flush().map_err(|e| e.to_string())?;
                        }
                    }

                    let mut out = json!({
                        "path": path.to_string_lossy(),
                        "width": width,
                        "height": height,
                    });
                    if let Some(b64) = preview_b64 { out["preview_b64"] = json!(b64); }
                    Ok(out)
                },
            },
        );
        map.insert(
            "ui.screenshot.annotate_burn",
            Entry {
                summary: "Annotate existing image: input {path, annotate[], downscale?} -> {path, ann_path, width, height, preview_b64?}",
                exec: |input| {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'path'")?;
                    let ann = input
                        .get("annotate")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let downscale = input.get("downscale").and_then(|v| v.as_u64()).map(|n| n as u32);
                    // Load image
                    let img_dyn = image::open(path).map_err(|e| e.to_string())?;
                    let mut img = img_dyn.to_rgba8();
                    let (width, height) = img.dimensions();
                    // Sidecar annotate JSON
                    let sidecar = serde_json::json!({"annotate": ann});
                    // Apply annotations (blur + border)
                    for it in ann.iter() {
                        let x = it.get("x").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let y = it.get("y").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let w = it.get("w").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let h = it.get("h").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let blur = it.get("blur").and_then(|v| v.as_bool()).unwrap_or(true);
                        // Clamp to bounds
                        let x2 = x.min(width.saturating_sub(1));
                        let y2 = y.min(height.saturating_sub(1));
                        let w2 = w.min(width.saturating_sub(x2));
                        let h2 = h.min(height.saturating_sub(y2));
                        if w2 == 0 || h2 == 0 { continue; }
                        if blur {
                            let sub = image::imageops::crop(&mut img, x2, y2, w2, h2).to_image();
                            let blurred = image::imageops::blur(&sub, 3.0);
                            image::imageops::overlay(&mut img, &blurred, x2 as i64, y2 as i64);
                        }
                        // Draw border (2px)
                        let teal = image::Rgba([27, 179, 163, 255]);
                        // top/bottom
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
                        // left/right
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
                    // Save annotated image next to original
                    let p = std::path::Path::new(path);
                    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
                    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("png");
                    let ann_path = p.with_file_name(format!("{}.ann.{}", stem, ext));
                    img.save(&ann_path).map_err(|e| e.to_string())?;
                    // Write sidecar
        let _ann_json_path = ann_path.with_extension(format!("{}.json", ann_path.extension().and_then(|s| s.to_str()).unwrap_or("json")));
                    // but prefer .ann.json next to file
                    let ann_sidecar = p.with_file_name(format!("{}.ann.json", stem));
                    std::fs::write(&ann_sidecar, serde_json::to_vec_pretty(&sidecar).unwrap_or_default()).map_err(|e| e.to_string())?;
                    // Build preview
                    let mut preview_b64 = None;
                    if let Some(maxw) = downscale {
                        let ratio = (height as f32) / (width as f32);
                        let new_w = maxw.max(1);
                        let new_h = ((new_w as f32) * ratio).round().max(1.0) as u32;
                        let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Triangle);
                        let mut bytes: Vec<u8> = Vec::new();
                        let dynimg = image::DynamicImage::ImageRgba8(resized);
                        dynimg
                            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageOutputFormat::Png)
                            .map_err(|e| e.to_string())?;
                        preview_b64 = Some(format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(&bytes)
                        ));
                    }
                    let mut out = json!({
                        "path": ann_path.to_string_lossy(),
                        "ann_path": ann_sidecar.to_string_lossy(),
                        "width": width,
                        "height": height
                    });
                    if let Some(b64) = preview_b64 { out["preview_b64"] = json!(b64); }
                    Ok(out)
                },
            },
        );
        map.insert(
            "ui.screenshot.ocr",
            Entry {
                summary: "Extract text from image: input {path} -> {text,blocks[]}",
                exec: |input| {
                    let path = input
                        .get("path")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'path'")?;
                    let text = ocr_image_text(path).unwrap_or_else(|e| format!("(unavailable: {})", e));
                    Ok(json!({"text": text, "blocks": []}))
                },
            },
        );
        map.insert(
            "guardrails.check",
            Entry {
                summary: "Heuristic/content guard checks with optional HTTP backend: input {text, policy?, rules?} -> {ok, score, issues[], suggestions[]}",
                exec: |input| {
                    let text = input
                        .get("text")
                        .and_then(|v| v.as_str())
                        .ok_or("missing 'text'")?;
                    // Optional HTTP backend when ARW_GUARDRAILS_URL is set
                    if let Ok(base) = std::env::var("ARW_GUARDRAILS_URL") {
                        if !base.trim().is_empty() {
                            let url = format!("{}/check", base.trim_end_matches('/'));
                            let mut body = serde_json::json!({"text": text});
                            if let Some(p) = input.get("policy") {
                                if let Some(obj) = body.as_object_mut() { obj.insert("policy".into(), p.clone()); }
                            }
                            if let Some(r) = input.get("rules") {
                                if let Some(obj) = body.as_object_mut() { obj.insert("rules".into(), r.clone()); }
                            }
                            let resp = ureq::post(&url)
                                .set("Content-Type", "application/json")
                                .send_json(body.clone());
                            if let Ok(r) = resp {
                                if r.status() >= 200 && r.status() < 300 {
                                    if let Ok(v) = r.into_json::<serde_json::Value>() {
                                        let okf = v.get("ok").and_then(|b| b.as_bool()).unwrap_or(true);
                                        let score = v.get("score").cloned().unwrap_or(serde_json::json!(0.0));
                                        let issues = v.get("issues").cloned().unwrap_or(serde_json::json!([]));
                                        let suggestions = v.get("suggestions").cloned().unwrap_or(serde_json::json!([]));
                                        return Ok(serde_json::json!({"ok": okf, "score": score, "issues": issues, "suggestions": suggestions}));
                                    }
                                }
                            }
                            // fall through to local heuristics on failure
                        }
                    }
                    // Local heuristic checks
                    #[derive(serde::Serialize)]
                    struct Issue { code: String, severity: String, message: String, #[serde(skip_serializing_if = "Option::is_none")] span: Option<(usize,usize)> }
                    let mut issues: Vec<Issue> = Vec::new();
                    // Email
                    let re_email = regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap();
                    for m in re_email.find_iter(text) {
                        issues.push(Issue{ code: "pii.email".into(), severity: "medium".into(), message: "Email address detected".into(), span: Some((m.start(), m.end()))});
                    }
                    // AWS Access Key ID
                    let re_aws = regex::Regex::new(r"AKIA[0-9A-Z]{16}").unwrap();
                    for m in re_aws.find_iter(text) { issues.push(Issue{ code: "secret.aws_access_key".into(), severity: "high".into(), message: "AWS access key pattern".into(), span: Some((m.start(), m.end()))}); }
                    // Google API key
                    let re_gapi = regex::Regex::new(r"AIza[0-9A-Za-z\-_]{35}").unwrap();
                    for m in re_gapi.find_iter(text) { issues.push(Issue{ code: "secret.gcp_api_key".into(), severity: "high".into(), message: "Google API key pattern".into(), span: Some((m.start(), m.end()))}); }
                    // Slack token
                    let re_slack = regex::Regex::new(r"xox[baprs]-[0-9A-Za-z-]{10,}").unwrap();
                    for m in re_slack.find_iter(text) { issues.push(Issue{ code: "secret.slack_token".into(), severity: "high".into(), message: "Slack token pattern".into(), span: Some((m.start(), m.end()))}); }
                    // URLs and allowlist
                    let re_url = regex::Regex::new(r"https?://[^\s)]+").unwrap();
                    let allowlist: Vec<String> = std::env::var("ARW_GUARDRAILS_ALLOWLIST")
                        .ok()
                        .map(|s| s.split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect())
                        .unwrap_or_default();
                    for m in re_url.find_iter(text) {
                        let url = m.as_str();
                        if let Ok(u) = url::Url::parse(url) {
                            let host = u.host_str().unwrap_or("").to_lowercase();
                            if !allowlist.is_empty() && !allowlist.iter().any(|h| host==*h || host.ends_with(&format!(".{h}"))) {
                                issues.push(Issue{ code: "egress.unlisted_host".into(), severity: "medium".into(), message: format!("URL host not in allowlist: {}", host), span: Some((m.start(), m.end()))});
                            }
                        }
                    }
                    // Basic prompt injection markers
                    let inj_markers = ["ignore previous", "disregard prior", "override instructions", "exfiltrate"];
                    let lower = text.to_ascii_lowercase();
                    for pat in inj_markers.iter() {
                        if let Some(pos) = lower.find(pat) {
                            issues.push(Issue{ code: "prompt_injection.marker".into(), severity: "medium".into(), message: format!("Suspicious instruction: '{}'", pat), span: Some((pos, pos+pat.len()))});
                        }
                    }
                    // Score: weighted count
                    let mut score: f64 = 0.0;
                    for it in &issues {
                        score += match it.severity.as_str() { "high" => 3.0, "medium" => 1.0, _ => 0.5 };
                    }
                    let ok = issues.iter().all(|i| i.severity != "high");
                    let issues_val = serde_json::to_value(&issues).unwrap_or(serde_json::json!([]));
                    Ok(serde_json::json!({ "ok": ok, "score": score, "issues": issues_val, "suggestions": serde_json::Value::Array(Vec::new()) }))
                },
            },
        );
        RwLock::new(map)
    })
}

// ---- Action Cache (MVP scaffold) ----
// Map: action_key -> content_digest (sha256 hex of serialized output)
// In-memory index with TTL and capacity (W-TinyLFU via moka)
static ACTION_MEM: OnceCell<Cache<String, String>> = OnceCell::new();
fn cache_capacity() -> u64 {
    std::env::var("ARW_TOOLS_CACHE_CAP")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(2048)
}
fn cache_ttl() -> Duration {
    let secs = std::env::var("ARW_TOOLS_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);
    Duration::from_secs(secs.max(1))
}
fn action_mem() -> &'static Cache<String, String> {
    ACTION_MEM.get_or_init(|| {
        Cache::builder()
            .max_capacity(cache_capacity())
            .time_to_live(cache_ttl())
            .build()
    })
}

fn tools_cas_dir() -> PathBuf {
    super::paths::state_dir().join("tools").join("by-digest")
}

fn canonicalize_json(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            // Sort keys to achieve a stable representation
            let mut pairs: Vec<(&String, &Value)> = m.iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = Map::new();
            for (k, val) in pairs.into_iter() {
                out.insert(k.clone(), canonicalize_json(val));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        _ => v.clone(),
    }
}

fn compute_action_key(tool_id: &str, tool_ver: &str, input: &Value) -> String {
    // Compose a stable key from tool id@version, an environment/policy signature,
    // and a canonicalized representation of input.
    let mut hasher = sha2::Sha256::new();
    hasher.update(tool_id.as_bytes());
    hasher.update(b"@\0");
    hasher.update(tool_ver.as_bytes());
    hasher.update(b"\0");
    // Environment/policy signature: include non-secret markers that should bust cache
    // when policy or secret versions change. We avoid hashing actual secret values.
    // Recognized markers (optional):
    // - ARW_POLICY_VERSION, ARW_SECRETS_VERSION
    // - ARW_PROJECT_ID, ARW_NET_POSTURE
    // - ARW_TOOLS_CACHE_SALT (manual salt)
    // Additionally, include a compact hash of the gating snapshot (deny lists/contracts).
    fn env_signature() -> String {
        let mut pairs: Vec<(String, String)> = Vec::new();
        let add = |k: &str, pairs: &mut Vec<(String, String)>| {
            if let Ok(v) = std::env::var(k) {
                if !v.is_empty() {
                    pairs.push((k.to_string(), v));
                }
            }
        };
        // Known version markers and posture context
        for k in [
            "ARW_POLICY_VERSION",
            "ARW_SECRETS_VERSION",
            "ARW_PROJECT_ID",
            "ARW_NET_POSTURE",
            "ARW_TOOLS_CACHE_SALT",
        ] {
            add(k, &mut pairs);
        }
        // Back-compat aliases sometimes used in setups
        for k in ["ARW_POLICY_VER", "ARW_SECRETS_VER"] {
            add(k, &mut pairs);
        }
        // Include a short hash of gating snapshot (policy denies/contracts)
        // to invalidate caches when policy is updated at runtime.
        let gating_hash = {
            let snap = arw_core::gating::snapshot();
            let bytes = serde_json::to_vec(&snap).unwrap_or_default();
            let mut h = sha2::Sha256::new();
            h.update(&bytes);
            format!("{:x}", h.finalize())
        };
        pairs.push(("GATING".into(), gating_hash));
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = String::new();
        for (k, v) in pairs.into_iter() {
            out.push_str(&k);
            out.push('=');
            out.push_str(&v);
            out.push(';');
        }
        out
    }
    let env_sig = env_signature();
    hasher.update(b"env:\0");
    hasher.update(env_sig.as_bytes());
    hasher.update(b"\0");

    let canon = canonicalize_json(input);
    let bytes = serde_json::to_vec(&canon).unwrap_or_default();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn compute_digest(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn tool_version(id: &str) -> &'static str {
    for ti in arw_core::introspect_tools() {
        if ti.id == id {
            return ti.version;
        }
    }
    // Unknown in registry (builtin examples):
    "0.0.0"
}

// Singleflight to coalesce identical misses
struct SfEntry {
    inner: Mutex<SfInner>,
    cv: Condvar,
}
struct SfInner {
    done: bool,
    result: Option<Result<Value, String>>,
}
static SINGLEFLIGHT: OnceCell<Mutex<HashMap<String, Arc<SfEntry>>>> = OnceCell::new();
fn sf_map() -> &'static Mutex<HashMap<String, Arc<SfEntry>>> {
    SINGLEFLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}
fn sf_begin(key: &str) -> (Arc<SfEntry>, bool) {
    let mut m = sf_map().lock().unwrap();
    if let Some(e) = m.get(key) {
        return (Arc::clone(e), false);
    }
    let ent = Arc::new(SfEntry {
        inner: Mutex::new(SfInner {
            done: false,
            result: None,
        }),
        cv: Condvar::new(),
    });
    m.insert(key.to_string(), Arc::clone(&ent));
    (ent, true)
}
fn sf_end(key: &str) {
    let mut m = sf_map().lock().unwrap();
    m.remove(key);
}

pub fn run(id: &str, input: &Value) -> Result<Value, String> {
    let (out, _, _, _, _) = run_with_cache_stats(id, input)?;
    Ok(out)
}

fn ocr_image_text(_path: &str) -> Result<String, String> {
    #[cfg(feature = "ocr_tesseract")]
    {
        let mut lt = leptess::LepTess::new(None, "eng").map_err(|e| e.to_string())?;
        lt.set_image(path);
        let text = lt.get_utf8_text().map_err(|e| e.to_string())?;
        return Ok(text);
    }
    #[allow(unreachable_code)]
    Err("ocr feature not compiled".into())
}

// Returns (output, outcome, digest_opt, action_key, age_secs)
// outcome: "hit" | "miss" | "coalesced"
pub type ToolRunOutcome = (Value, &'static str, Option<String>, String, Option<u64>);
pub fn run_with_cache_stats(id: &str, input: &Value) -> Result<ToolRunOutcome, String> {
    let map = reg().read().unwrap();
    let ent = match map.get(id) {
        Some(e) => e,
        None => return Err(format!("unknown tool id: {}", id)),
    };
    let ver = tool_version(id);
    let key = compute_action_key(id, ver, input);

    // Fast path: in-memory index → disk CAS
    if let Some(digest) = action_mem().get(&key) {
        let path = tools_cas_dir().join(format!("{}.json", digest));
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                CACHE_HIT.fetch_add(1, Ordering::Relaxed);
                // Age from file mtime (seconds)
                let age_secs = fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .map(|d| d.as_secs());
                return Ok((v, "hit", Some(digest), key, age_secs));
            }
        }
        // stale: continue to compute
    }

    // Coalesce identical misses
    let (sf, is_leader) = sf_begin(&key);
    if !is_leader {
        // Wait for leader to finish
        let mut guard = sf.inner.lock().unwrap();
        while !guard.done {
            guard = sf.cv.wait(guard).unwrap();
        }
        if let Some(res) = guard.result.clone() {
            drop(guard);
            let (v, d_opt) = match res {
                Ok(v) => {
                    // Compute digest for event; best-effort
                    let d = serde_json::to_vec(&v).ok().map(|b| compute_digest(&b));
                    (v, d)
                }
                Err(e) => return Err(e),
            };
            CACHE_COALESCED.fetch_add(1, Ordering::Relaxed);
            return Ok((v, "coalesced", d_opt, key, None));
        }
        // Should not happen; fall through to compute
    }

    // Leader: execute and store
    let res = (ent.exec)(input);
    let outcome: Result<(Value, Option<String>), String> = match res {
        Ok(out) => {
            let digest_opt = match serde_json::to_vec(&out) {
                Ok(bytes) => {
                    let digest = compute_digest(&bytes);
                    let dir = tools_cas_dir();
                    let _ = fs::create_dir_all(&dir);
                    let path = dir.join(format!("{}.json", &digest));
                    if !path.exists() {
                        if let Ok(mut f) = fs::File::create(&path) {
                            let _ = f.write_all(&bytes);
                        }
                    }
                    action_mem().insert(key.clone(), digest.clone());
                    Some(digest)
                }
                Err(_) => None,
            };
            Ok((out, digest_opt))
        }
        Err(e) => Err(e),
    };
    // Publish to followers and clean up
    let mut inner = sf.inner.lock().unwrap();
    match &outcome {
        Ok((v, _)) => {
            inner.result = Some(Ok(v.clone()));
        }
        Err(e) => {
            inner.result = Some(Err(e.clone()));
        }
    }
    inner.done = true;
    sf.cv.notify_all();
    drop(inner);
    sf_end(&key);

    match outcome {
        Ok((v, d)) => {
            CACHE_MISS.fetch_add(1, Ordering::Relaxed);
            Ok((v, "miss", d, key, Some(0)))
        }
        Err(e) => Err(e),
    }
}

// ---- Counters and stats ----
static CACHE_HIT: AtomicU64 = AtomicU64::new(0);
static CACHE_MISS: AtomicU64 = AtomicU64::new(0);
static CACHE_COALESCED: AtomicU64 = AtomicU64::new(0);

pub fn cache_stats_value() -> Value {
    json!({
        "hit": CACHE_HIT.load(Ordering::Relaxed),
        "miss": CACHE_MISS.load(Ordering::Relaxed),
        "coalesced": CACHE_COALESCED.load(Ordering::Relaxed),
        "capacity": cache_capacity(),
        "ttl_secs": cache_ttl().as_secs(),
        "entries": action_mem().entry_count() as u64,
    })
}

pub fn list() -> Vec<(&'static str, &'static str)> {
    let map = reg().read().unwrap();
    map.iter().map(|(k, v)| (*k, v.summary)).collect()
}
