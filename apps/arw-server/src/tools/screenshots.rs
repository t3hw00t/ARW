use super::ocr_support::{BackendKind, Overrides, QualityTier, RunMetadata, RuntimeClass};
use super::{ocr_support, ToolError, Value};
use crate::util;
use ::screenshots as capture_backend;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{Datelike, Utc};
use image::{self, imageops, DynamicImage, GenericImageView, ImageFormat, RgbaImage};
use serde_json::{json, Value as JsonValue};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{Builder as TempFileBuilder, TempPath};
use tokio::task::spawn_blocking;
use tracing::warn;

#[cfg(feature = "ocr_compression")]
use serde::Deserialize;
#[cfg(feature = "ocr_compression")]
use std::time::Duration;

const OCR_SIDECAR_VERSION: u32 = 1;
const LITE_MAX_DIMENSION: u32 = 1280;
const METRIC_OCR_RUNS: &str = "arw_ocr_runs_total";
const METRIC_OCR_CACHE_HITS: &str = "arw_ocr_cache_hits_total";
const METRIC_OCR_PREPROCESS: &str = "arw_ocr_preprocess_total";
const METRIC_OCR_FALLBACKS: &str = "arw_ocr_backend_fallbacks_total";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OcrBlock {
    text: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<f32>,
}

#[derive(Debug, Clone)]
struct OcrResult {
    text: String,
    blocks: Vec<OcrBlock>,
    lang: String,
}

#[derive(Debug)]
struct PreparedImage {
    path: String,
    _temp_path: Option<TempPath>,
    steps: Vec<String>,
}

impl PreparedImage {
    fn original(path: &str) -> Self {
        PreparedImage {
            path: path.to_string(),
            _temp_path: None,
            steps: Vec::new(),
        }
    }

    fn with_temp(path: TempPath, steps: Vec<String>) -> Self {
        let path_str = path.to_string_lossy().into_owned();
        PreparedImage {
            path: path_str,
            _temp_path: Some(path),
            steps,
        }
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn steps(&self) -> &[String] {
        &self.steps
    }
}

#[derive(Debug)]
enum BackendError {
    Tool(ToolError),
    Unavailable(String),
}

impl From<ToolError> for BackendError {
    fn from(value: ToolError) -> Self {
        BackendError::Tool(value)
    }
}

impl BackendError {
    fn into_tool_error(self) -> ToolError {
        match self {
            BackendError::Tool(err) => err,
            BackendError::Unavailable(reason) => ToolError::Runtime(reason),
        }
    }
}

pub(super) async fn capture(input: Value) -> Result<Value, ToolError> {
    spawn_blocking(move || capture_blocking(&input))
        .await
        .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))?
}

pub(super) async fn annotate(input: Value) -> Result<Value, ToolError> {
    spawn_blocking(move || annotate_blocking(&input))
        .await
        .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))?
}

pub(super) async fn ocr(state: &crate::AppState, input: Value) -> Result<Value, ToolError> {
    let capability = state.capability();
    spawn_blocking(move || {
        let capability_ref = capability.as_ref();
        ocr_blocking(&input, capability_ref)
    })
    .await
    .map_err(|e| ToolError::Runtime(format!("join error: {}", e)))?
}

fn capture_blocking(input: &Value) -> Result<Value, ToolError> {
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
                    .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
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

fn annotate_blocking(input: &Value) -> Result<Value, ToolError> {
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
                .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
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

fn prepare_image_for_quality(path: &str, quality: QualityTier) -> Result<PreparedImage, ToolError> {
    match quality {
        QualityTier::Lite => prepare_lite_image(path),
        QualityTier::Balanced | QualityTier::Full => Ok(PreparedImage::original(path)),
    }
}

fn prepare_lite_image(path: &str) -> Result<PreparedImage, ToolError> {
    let img = image::open(path).map_err(|e| ToolError::Runtime(e.to_string()))?;
    let (width, height) = img.dimensions();
    let mut steps = Vec::new();
    let mut buffer: image::GrayImage = img.to_luma8();
    steps.push("grayscale".to_string());

    if width > LITE_MAX_DIMENSION || height > LITE_MAX_DIMENSION {
        let scale_w = LITE_MAX_DIMENSION as f32 / width as f32;
        let scale_h = LITE_MAX_DIMENSION as f32 / height as f32;
        let scale = scale_w.min(scale_h).min(1.0);
        let new_w = ((width as f32) * scale).round().max(1.0) as u32;
        let new_h = ((height as f32) * scale).round().max(1.0) as u32;
        buffer = imageops::resize(
            &buffer,
            new_w.max(1),
            new_h.max(1),
            imageops::FilterType::Triangle,
        );
        steps.push(format!(
            "downscale:max={} ({}x{}→{}x{})",
            LITE_MAX_DIMENSION, width, height, new_w, new_h
        ));
    }

    let temp_path = write_temp_png(&buffer)?;
    Ok(PreparedImage::with_temp(temp_path, steps))
}

fn write_temp_png(image: &image::GrayImage) -> Result<TempPath, ToolError> {
    let file = TempFileBuilder::new()
        .prefix("arw-ocr-prep-")
        .suffix(".png")
        .tempfile()
        .map_err(|e| ToolError::Runtime(e.to_string()))?;
    image
        .save(file.path())
        .map_err(|e| ToolError::Runtime(e.to_string()))?;
    Ok(file.into_temp_path())
}

fn run_backend_ocr(
    prepared: &PreparedImage,
    backend: BackendKind,
    lang: &str,
    quality: QualityTier,
) -> Result<OcrResult, BackendError> {
    match backend {
        BackendKind::LegacyTesseract => run_legacy_backend(prepared, lang),
        BackendKind::VisionCompression => run_vision_backend(prepared, lang, quality),
    }
}

fn run_legacy_backend(prepared: &PreparedImage, lang: &str) -> Result<OcrResult, BackendError> {
    ocr_image_text(prepared.path(), lang).map_err(BackendError::from)
}

#[cfg(feature = "ocr_compression")]
fn run_vision_backend(
    prepared: &PreparedImage,
    lang: &str,
    quality: QualityTier,
) -> Result<OcrResult, BackendError> {
    use reqwest::blocking::Client as BlockingClient;

    let endpoint = match std::env::var("ARW_OCR_COMPRESSION_ENDPOINT") {
        Ok(value) => value,
        Err(_) => {
            return Err(BackendError::Unavailable(
                "ARW_OCR_COMPRESSION_ENDPOINT not set".into(),
            ))
        }
    };

    let timeout_secs = std::env::var("ARW_OCR_COMPRESSION_TIMEOUT_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(120);

    let client = BlockingClient::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|err| {
            BackendError::Unavailable(format!("vision backend client build failed: {err}"))
        })?;

    let payload = json!({
        "path": prepared.path(),
        "lang": lang,
        "quality": quality.as_str(),
        "preprocess_steps": prepared.steps(),
    });

    let response = client
        .post(&endpoint)
        .json(&payload)
        .send()
        .map_err(|err| {
            BackendError::Unavailable(format!("vision backend request failed: {err}"))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(BackendError::Tool(ToolError::Runtime(format!(
            "vision backend returned {status}: {body}"
        ))));
    }

    let parsed: VisionResponse = response.json().map_err(|err| {
        BackendError::Tool(ToolError::Runtime(format!(
            "vision backend response decode failed: {err}"
        )))
    })?;

    if parsed.text.trim().is_empty() {
        return Err(BackendError::Tool(ToolError::Runtime(
            "vision backend returned empty text".into(),
        )));
    }

    let blocks = parsed
        .blocks
        .into_iter()
        .map(|block| OcrBlock {
            text: block.text,
            x: block.x,
            y: block.y,
            w: block.w,
            h: block.h,
            confidence: block.confidence,
        })
        .collect();

    let lang_out = parsed.lang.unwrap_or_else(|| lang.to_string());

    Ok(OcrResult {
        text: parsed.text,
        blocks,
        lang: lang_out,
    })
}

#[cfg(not(feature = "ocr_compression"))]
fn run_vision_backend(
    _prepared: &PreparedImage,
    _lang: &str,
    _quality: QualityTier,
) -> Result<OcrResult, BackendError> {
    Err(BackendError::Unavailable(
        "vision compression backend not built into this binary".into(),
    ))
}

#[cfg(feature = "ocr_compression")]
#[derive(Debug, Deserialize)]
struct VisionResponse {
    text: String,
    #[serde(default)]
    blocks: Vec<VisionResponseBlock>,
    #[serde(default)]
    lang: Option<String>,
}

#[cfg(feature = "ocr_compression")]
#[derive(Debug, Deserialize)]
struct VisionResponseBlock {
    text: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    #[serde(default)]
    confidence: Option<f32>,
}

fn ocr_blocking(
    input: &Value,
    capability: &crate::capability::CapabilityService,
) -> Result<Value, ToolError> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'path'".into()))?;
    let lang = input
        .get("lang")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("eng");
    let force = input
        .get("force")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let src = Path::new(path);
    let parent = src.parent().unwrap_or_else(|| Path::new("."));
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy())
        .ok_or_else(|| ToolError::Invalid("invalid 'path'".into()))?;

    let requested_lang = lang.trim();
    let requested_lang = if requested_lang.is_empty() {
        "eng"
    } else {
        requested_lang
    };
    let requested_fragment = sanitize_lang_fragment(requested_lang);

    let stem_str = stem.as_ref();

    let overrides = Overrides::from_input(input);
    let mut run_meta = ocr_support::compute_run_metadata(capability, &overrides);
    if !run_meta.backend_supported {
        return Err(ToolError::Runtime(run_meta.backend_reason.clone()));
    }

    if !force {
        if let Some(cached) = load_cached_ocr(parent, stem_str, &requested_fragment) {
            if cached_metadata_matches(&cached, &run_meta) {
                record_cache_hit_metrics(&cached);
                return Ok(cached);
            }
        }
    }

    let prepared = match prepare_image_for_quality(path, run_meta.quality) {
        Ok(image) => image,
        Err(err) => {
            tracing::warn!(
                %path,
                quality = %run_meta.quality.as_str(),
                %err,
                "preprocessing for OCR quality tier failed; using original image"
            );
            PreparedImage::original(path)
        }
    };

    if !prepared.steps().is_empty() {
        metrics::counter!(
            METRIC_OCR_PREPROCESS,
            "quality" => run_meta.quality.as_str()
        )
        .increment(1);
    }

    let original_backend = run_meta.backend;
    let result = match run_backend_ocr(
        &prepared,
        run_meta.backend,
        requested_lang,
        run_meta.quality,
    ) {
        Ok(res) => res,
        Err(BackendError::Unavailable(reason)) => {
            warn!(
                %path,
                backend = %run_meta.backend.as_str(),
                %reason,
                "vision backend unavailable; falling back to legacy OCR"
            );
            metrics::counter!(
                METRIC_OCR_FALLBACKS,
                "from" => run_meta.backend.as_str(),
                "to" => BackendKind::LegacyTesseract.as_str()
            )
            .increment(1);
            run_meta.backend = BackendKind::LegacyTesseract;
            run_meta.backend_reason =
                format!("{reason}; fell back to legacy backend for execution");
            run_meta.backend_supported = true;
            run_meta.runtime = RuntimeClass::CpuBalanced;
            run_meta.runtime_reason = "legacy backend fallback".into();
            run_meta.compression_target = None;
            let (expected, confidence) =
                ocr_support::quality_confidence_hint(run_meta.backend, run_meta.quality);
            run_meta.expected_quality = expected;
            run_meta.confidence_hint = confidence;
            run_backend_ocr(
                &prepared,
                run_meta.backend,
                requested_lang,
                run_meta.quality,
            )
            .map_err(|err| err.into_tool_error())?
        }
        Err(err) => return Err(err.into_tool_error()),
    };

    if original_backend != run_meta.backend {
        // Ensure compression target and hints reflect the backend we actually used.
        if run_meta.backend == BackendKind::LegacyTesseract {
            run_meta.compression_target = None;
            let (expected, confidence) =
                ocr_support::quality_confidence_hint(run_meta.backend, run_meta.quality);
            run_meta.expected_quality = expected;
            run_meta.confidence_hint = confidence;
        }
    }

    let effective_fragment = sanitize_lang_fragment(&result.lang);
    if !force && effective_fragment != requested_fragment {
        if let Some(cached) = load_cached_ocr(parent, stem_str, &effective_fragment) {
            if cached_metadata_matches(&cached, &run_meta) {
                record_cache_hit_metrics(&cached);
                return Ok(cached);
            }
        }
    }

    metrics::counter!(
        METRIC_OCR_RUNS,
        "backend" => run_meta.backend.as_str(),
        "quality" => run_meta.quality.as_str(),
        "runtime" => run_meta.runtime.as_str()
    )
    .increment(1);

    let ocr_path = sidecar_path(parent, stem_str, &effective_fragment);
    let blocks_value =
        serde_json::to_value(&result.blocks).map_err(|e| ToolError::Runtime(e.to_string()))?;
    let preprocess_steps_json = json!(prepared.steps());

    let generated_at = Utc::now().to_rfc3339();
    let mut sidecar_map = serde_json::Map::new();
    sidecar_map.insert("schema_version".into(), json!(OCR_SIDECAR_VERSION));
    sidecar_map.insert("generated_at".into(), json!(generated_at.clone()));
    sidecar_map.insert("source_path".into(), json!(path));
    sidecar_map.insert("lang".into(), json!(result.lang.clone()));
    sidecar_map.insert("text".into(), json!(result.text.clone()));
    sidecar_map.insert("blocks".into(), blocks_value.clone());
    sidecar_map.insert("backend".into(), json!(run_meta.backend.as_str()));
    sidecar_map.insert(
        "backend_reason".into(),
        json!(run_meta.backend_reason.clone()),
    );
    sidecar_map.insert(
        "backend_supported".into(),
        json!(run_meta.backend_supported),
    );
    sidecar_map.insert("quality_tier".into(), json!(run_meta.quality.as_str()));
    sidecar_map.insert(
        "quality_reason".into(),
        json!(run_meta.quality_reason.clone()),
    );
    sidecar_map.insert("runtime_class".into(), json!(run_meta.runtime.as_str()));
    sidecar_map.insert(
        "runtime_reason".into(),
        json!(run_meta.runtime_reason.clone()),
    );
    if let Some(ratio) = run_meta.compression_target {
        sidecar_map.insert("compression_target".into(), json!(ratio));
    }
    if let Some(expected) = run_meta.expected_quality {
        sidecar_map.insert("expected_quality".into(), json!(expected));
    }
    if let Some(confidence) = run_meta.confidence_hint {
        sidecar_map.insert("confidence_hint".into(), json!(confidence));
    }
    sidecar_map.insert("preprocess_steps".into(), preprocess_steps_json.clone());
    if let Ok(capability_value) = serde_json::to_value(&run_meta.profile) {
        sidecar_map.insert("capability_profile".into(), capability_value);
    }
    let sidecar_value = JsonValue::Object(sidecar_map);
    let sidecar_bytes =
        serde_json::to_vec_pretty(&sidecar_value).map_err(|e| ToolError::Runtime(e.to_string()))?;
    write_sidecar_atomic(&ocr_path, &sidecar_bytes)?;

    let mut response = json!({
        "text": result.text,
        "blocks": blocks_value,
        "lang": result.lang,
        "ocr_path": ocr_path.to_string_lossy(),
        "source_path": path,
        "generated_at": generated_at,
        "cached": false,
    });
    response["backend"] = json!(run_meta.backend.as_str());
    response["backend_reason"] = json!(run_meta.backend_reason);
    response["backend_supported"] = json!(run_meta.backend_supported);
    response["quality_tier"] = json!(run_meta.quality.as_str());
    response["quality_reason"] = json!(run_meta.quality_reason);
    response["runtime_class"] = json!(run_meta.runtime.as_str());
    response["runtime_reason"] = json!(run_meta.runtime_reason);
    if let Some(ratio) = run_meta.compression_target {
        response["compression_target"] = json!(ratio);
    }
    if let Some(expected) = run_meta.expected_quality {
        response["expected_quality"] = json!(expected);
    }
    if let Some(confidence) = run_meta.confidence_hint {
        response["confidence_hint"] = json!(confidence);
    }
    if let Ok(capability_value) = serde_json::to_value(run_meta.profile) {
        response["capability_profile"] = capability_value;
    }
    response["preprocess_steps"] = preprocess_steps_json;

    Ok(response)
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
    let screens = capture_backend::Screen::all().map_err(|e| e.to_string())?;
    let screen = if let Some(rest) = scope.strip_prefix("display:") {
        let idx: usize = rest.parse().unwrap_or(0);
        screens
            .get(idx)
            .cloned()
            .ok_or_else(|| "display index out of range".to_string())?
    } else {
        capture_backend::Screen::from_point(0, 0)
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

#[cfg(feature = "ocr_tesseract")]
fn ocr_image_text(path: &str, lang: &str) -> Result<OcrResult, ToolError> {
    let requested_lang = lang.trim();
    let engine_lang = if requested_lang.is_empty() {
        "eng"
    } else {
        requested_lang
    };
    let mut lang_used = engine_lang.to_string();
    let mut lt = match leptess::LepTess::new(None, engine_lang) {
        Ok(engine) => engine,
        Err(err) => {
            if engine_lang != "eng" {
                tracing::warn!(
                    %engine_lang,
                    %path,
                    "falling back to 'eng' for OCR ({}); is the language data installed?",
                    err
                );
                lang_used = "eng".to_string();
                leptess::LepTess::new(None, "eng")
                    .map_err(|fallback| ToolError::Runtime(fallback.to_string()))?
            } else {
                return Err(ToolError::Runtime(err.to_string()));
            }
        }
    };
    lt.set_image(path)
        .map_err(|e| ToolError::Runtime(e.to_string()))?;
    lt.set_fallback_source_resolution(300);
    if lt.recognize() != 0 {
        return Err(ToolError::Runtime("tesseract recognize failed".into()));
    }
    let full_text = lt
        .get_utf8_text()
        .map_err(|e| ToolError::Runtime(e.to_string()))?;
    let normalized_text = normalize_ocr_text(full_text);
    let tsv = lt
        .get_tsv_text(0)
        .map_err(|e| ToolError::Runtime(e.to_string()))?;
    let blocks = parse_tsv_blocks(&tsv);
    Ok(OcrResult {
        text: normalized_text,
        blocks,
        lang: lang_used,
    })
}

#[cfg(not(feature = "ocr_tesseract"))]
fn ocr_image_text(_path: &str, _lang: &str) -> Result<OcrResult, ToolError> {
    Err(ToolError::Runtime(
        "ocr feature not compiled (enable arw-server/ocr_tesseract)".into(),
    ))
}

#[cfg(feature = "ocr_tesseract")]
fn parse_tsv_blocks(tsv: &str) -> Vec<OcrBlock> {
    tsv.lines().skip(1).filter_map(parse_tsv_line).collect()
}

#[cfg(feature = "ocr_tesseract")]
fn parse_tsv_line(line: &str) -> Option<OcrBlock> {
    let cols: Vec<&str> = line.split('\t').collect();
    if cols.len() < 12 {
        return None;
    }
    if cols[0].trim() != "5" {
        return None;
    }
    let text = cols[11].trim();
    if text.is_empty() {
        return None;
    }
    let left = cols[6].parse().ok()?;
    let top = cols[7].parse().ok()?;
    let width = cols[8].parse().ok()?;
    let height = cols[9].parse().ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let confidence = cols[10]
        .parse::<f32>()
        .ok()
        .filter(|v| *v >= 0.0)
        .map(|v| (v * 100.0).round() / 100.0);
    Some(OcrBlock {
        text: text.to_string(),
        x: left,
        y: top,
        w: width,
        h: height,
        confidence,
    })
}

#[cfg(feature = "ocr_tesseract")]
fn normalize_ocr_text(text: String) -> String {
    let cleaned = text.replace("\r\n", "\n");
    cleaned.trim_end().to_string()
}

fn sanitize_lang_fragment(lang: &str) -> String {
    let mut out = String::new();
    for c in lang.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, '+' | '-' | '_') {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "eng".into()
    } else {
        out
    }
}

fn sidecar_path(parent: &Path, stem: &str, lang_fragment: &str) -> PathBuf {
    parent.join(format!("{}.ocr.{}.json", stem, lang_fragment))
}

fn guess_screenshot_path(parent: &Path, stem: &str) -> Option<String> {
    const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"]; // seen capture formats
    for ext in EXTENSIONS {
        let candidate = parent.join(format!("{}.{}", stem, ext));
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

fn load_cached_ocr(parent: &Path, stem: &str, lang_fragment: &str) -> Option<Value> {
    let path = sidecar_path(parent, stem, lang_fragment);
    let data = fs::read(&path).ok()?;
    let doc: JsonValue = serde_json::from_slice(&data).ok()?;
    let text = doc.get("text")?.as_str()?.to_owned();
    let blocks = doc.get("blocks")?.clone();
    let lang = doc
        .get("lang")
        .and_then(|v| v.as_str())
        .unwrap_or(lang_fragment)
        .to_owned();
    let generated_at = doc
        .get("generated_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let source_path = doc
        .get("source_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
        .or_else(|| guess_screenshot_path(parent, stem))
        .unwrap_or_else(|| parent.join(stem).to_string_lossy().into_owned());

    let mut response = json!({
        "text": text,
        "blocks": blocks.clone(),
        "lang": lang,
        "ocr_path": path.to_string_lossy(),
        "source_path": source_path,
        "cached": true,
    });
    if let Some(ts) = generated_at {
        response["generated_at"] = json!(ts);
    }
    if let Some(backend) = doc.get("backend").and_then(|v| v.as_str()) {
        response["backend"] = json!(backend);
    }
    if let Some(reason) = doc.get("backend_reason").and_then(|v| v.as_str()) {
        response["backend_reason"] = json!(reason);
    }
    if let Some(supported) = doc.get("backend_supported").and_then(|v| v.as_bool()) {
        response["backend_supported"] = json!(supported);
    }
    if let Some(quality) = doc.get("quality_tier").and_then(|v| v.as_str()) {
        response["quality_tier"] = json!(quality);
    }
    if let Some(reason) = doc.get("quality_reason").and_then(|v| v.as_str()) {
        response["quality_reason"] = json!(reason);
    }
    if let Some(runtime_class) = doc.get("runtime_class").and_then(|v| v.as_str()) {
        response["runtime_class"] = json!(runtime_class);
    }
    if let Some(reason) = doc.get("runtime_reason").and_then(|v| v.as_str()) {
        response["runtime_reason"] = json!(reason);
    }
    if let Some(ratio) = doc.get("compression_target").and_then(|v| v.as_f64()) {
        response["compression_target"] = json!(ratio);
    }
    if let Some(expected) = doc.get("expected_quality").and_then(|v| v.as_f64()) {
        response["expected_quality"] = json!(expected);
    }
    if let Some(confidence) = doc.get("confidence_hint").and_then(|v| v.as_f64()) {
        response["confidence_hint"] = json!(confidence);
    }
    if let Some(capability) = doc.get("capability_profile") {
        response["capability_profile"] = capability.clone();
    }
    let preprocess_steps_value = doc
        .get("preprocess_steps")
        .cloned()
        .unwrap_or_else(|| json!([]));
    response["preprocess_steps"] = preprocess_steps_value;
    Some(response)
}

fn cached_metadata_matches(cached: &Value, meta: &RunMetadata) -> bool {
    let backend_ok = cached
        .get("backend")
        .and_then(|v| v.as_str())
        .map(|backend| backend.eq(meta.backend.as_str()))
        .unwrap_or(true);
    let quality_ok = cached
        .get("quality_tier")
        .and_then(|v| v.as_str())
        .map(|tier| tier.eq(meta.quality.as_str()))
        .unwrap_or(true);
    backend_ok && quality_ok
}

fn record_cache_hit_metrics(cached: &Value) {
    let backend = metric_backend_label(cached.get("backend").and_then(|v| v.as_str()));
    let quality = metric_quality_label(cached.get("quality_tier").and_then(|v| v.as_str()));
    metrics::counter!(
        METRIC_OCR_CACHE_HITS,
        "backend" => backend,
        "quality" => quality
    )
    .increment(1);
}

fn metric_backend_label(raw: Option<&str>) -> &'static str {
    match raw {
        Some(value) if value.eq_ignore_ascii_case("legacy") => "legacy",
        Some(value) if value.eq_ignore_ascii_case("vision_compression") => "vision_compression",
        _ => "unknown",
    }
}

fn metric_quality_label(raw: Option<&str>) -> &'static str {
    match raw {
        Some(value) if value.eq_ignore_ascii_case("lite") => "lite",
        Some(value) if value.eq_ignore_ascii_case("balanced") => "balanced",
        Some(value) if value.eq_ignore_ascii_case("full") => "full",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::ocr_support::{BackendKind, CapabilityProfile, GpuKind, QualityTier, RuntimeClass};
    use super::*;
    use image::{ImageBuffer, Luma};
    use serde_json::json;
    use tempfile::Builder as TempFileBuilder;

    fn dummy_profile() -> CapabilityProfile {
        CapabilityProfile {
            total_mem_mb: 8192,
            available_mem_mb: 4096,
            logical_cpus: 8,
            physical_cpus: 4,
            gpu_vram_mb: Some(4096),
            gpu_kind: GpuKind::Dedicated,
            low_power_hint: false,
            low_power_hint_source: None,
            gpu_vram_source: None,
            os: "test".into(),
            collected_at: "1970-01-01T00:00:00Z".into(),
        }
    }

    fn dummy_metadata() -> RunMetadata {
        RunMetadata {
            backend: BackendKind::LegacyTesseract,
            backend_supported: true,
            backend_reason: "test".into(),
            quality: QualityTier::Balanced,
            quality_reason: "test".into(),
            runtime: RuntimeClass::CpuBalanced,
            runtime_reason: "test".into(),
            compression_target: None,
            expected_quality: Some(0.97),
            confidence_hint: Some(0.95),
            profile: dummy_profile(),
        }
    }

    #[test]
    fn cached_metadata_without_fields_defaults_to_match() {
        let cache = json!({"text":"","blocks":[],"lang":"eng"});
        let meta = dummy_metadata();
        assert!(cached_metadata_matches(&cache, &meta));
    }

    #[test]
    fn cached_metadata_mismatch_backend() {
        let cache = json!({"backend":"vision_compression"});
        let meta = dummy_metadata();
        assert!(!cached_metadata_matches(&cache, &meta));
    }

    #[test]
    fn cached_metadata_mismatch_quality() {
        let cache = json!({"quality_tier":"lite"});
        let meta = dummy_metadata();
        assert!(!cached_metadata_matches(&cache, &meta));
    }

    #[test]
    fn prepare_lite_quality_downscales_and_creates_temp_file() {
        let mut tmp = TempFileBuilder::new()
            .prefix("arw-test-orig-")
            .suffix(".png")
            .tempfile()
            .expect("tempfile");
        let path = tmp.path().to_path_buf();
        let img: ImageBuffer<Luma<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(2000, 1200, Luma([180u8]));
        img.save(&path).expect("save original");

        let prepared =
            prepare_image_for_quality(path.to_string_lossy().as_ref(), QualityTier::Lite)
                .expect("prepare");
        assert_ne!(prepared.path(), path.to_string_lossy());
        assert!(prepared.steps().iter().any(|s| s.contains("grayscale")));
        assert!(prepared.steps().iter().any(|s| s.contains("downscale")));
        let dims = image::open(prepared.path())
            .expect("open prepared")
            .dimensions();
        assert!(dims.0 <= LITE_MAX_DIMENSION && dims.1 <= LITE_MAX_DIMENSION);
    }

    #[test]
    fn prepare_balanced_quality_is_noop() {
        let mut tmp = TempFileBuilder::new()
            .prefix("arw-test-orig-")
            .suffix(".png")
            .tempfile()
            .expect("tempfile");
        let path = tmp.path().to_path_buf();
        let img: ImageBuffer<Luma<u8>, Vec<u8>> = ImageBuffer::from_pixel(640, 480, Luma([200u8]));
        img.save(&path).expect("save original");

        let prepared =
            prepare_image_for_quality(path.to_string_lossy().as_ref(), QualityTier::Balanced)
                .expect("prepare");
        assert_eq!(prepared.path(), path.to_string_lossy());
        assert!(prepared.steps().is_empty());
    }
}

fn write_sidecar_atomic(path: &Path, bytes: &[u8]) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ToolError::Runtime(e.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| ToolError::Runtime(e.to_string()))?;
    match fs::rename(&tmp, path) {
        Ok(_) => Ok(()),
        Err(_) => {
            let _ = fs::remove_file(path);
            match fs::rename(&tmp, path) {
                Ok(_) => Ok(()),
                Err(err) => {
                    let _ = fs::remove_file(&tmp);
                    Err(ToolError::Runtime(err.to_string()))
                }
            }
        }
    }
}
