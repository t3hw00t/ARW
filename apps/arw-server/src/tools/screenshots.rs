use super::{ToolError, Value};
use crate::util;
use ::screenshots as capture_backend;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use chrono::{Datelike, Utc};
use image::{self, imageops, DynamicImage, ImageOutputFormat, RgbaImage};
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::task::spawn_blocking;

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

pub(super) async fn ocr(input: Value) -> Result<Value, ToolError> {
    spawn_blocking(move || ocr_blocking(&input))
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

fn ocr_blocking(input: &Value) -> Result<Value, ToolError> {
    let path = input
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'path'".into()))?;
    let text = ocr_image_text(path)?;
    Ok(json!({"text": text, "blocks": []}))
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
