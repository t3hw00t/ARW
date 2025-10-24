use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use serde_json::{json, Value as JsonValue};
use walkdir::WalkDir;

use crate::load_effective_paths;

#[derive(Subcommand)]
pub enum ScreenshotsCmd {
    /// Re-run OCR for screenshots missing per-language sidecars
    BackfillOcr(BackfillOcrArgs),
}

#[derive(Args)]
pub struct BackfillOcrArgs {
    /// Base URL of the service running arw-server
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token (falls back to ARW_ADMIN_TOKEN)
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Language to OCR (tessdata language code)
    #[arg(long, default_value = "eng")]
    pub lang: String,
    /// Force OCR even if a cached sidecar exists
    #[arg(long)]
    pub force: bool,
    /// Requested OCR backend (legacy or vision_compression)
    #[arg(long)]
    pub backend: Option<String>,
    /// Requested quality tier (lite, balanced, or full)
    #[arg(long)]
    pub quality: Option<String>,
    /// Prefer low-power execution mode when scheduling OCR
    #[arg(long)]
    pub prefer_low_power: bool,
    /// Refresh capability profile before running OCR
    #[arg(long)]
    pub refresh_capabilities: bool,
    /// Only print the files that would be processed
    #[arg(long)]
    pub dry_run: bool,
    /// Limit number of screenshots to process
    #[arg(long)]
    pub limit: Option<usize>,
    /// Timeout in seconds for each HTTP call
    #[arg(long, default_value_t = 20)]
    pub timeout: u64,
    /// Show per-file progress
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Debug, Clone)]
struct ScreenshotTarget {
    path: PathBuf,
    sidecar: PathBuf,
}

#[derive(Debug, Default, Clone, Copy)]
struct ScanStats {
    scanned: usize,
    skipped_cached: usize,
    skipped_other: usize,
}

pub fn execute(cmd: ScreenshotsCmd) -> Result<()> {
    match cmd {
        ScreenshotsCmd::BackfillOcr(args) => cmd_backfill_ocr(&args),
    }
}

fn cmd_backfill_ocr(args: &BackfillOcrArgs) -> Result<()> {
    const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

    let paths = load_effective_paths();
    let state_dir = paths
        .get("state_dir")
        .and_then(JsonValue::as_str)
        .map(PathBuf::from)
        .context("state_dir missing from effective paths")?;
    let screenshots_dir = state_dir.join("screenshots");
    if !screenshots_dir.exists() {
        println!(
            "No screenshots directory found at {}",
            screenshots_dir.display()
        );
        return Ok(());
    }

    let lang_fragment = sanitize_lang_fragment_cli(&args.lang);
    let (mut targets, stats) = collect_screenshot_targets(
        &screenshots_dir,
        &lang_fragment,
        args.force,
        args.limit,
        SUPPORTED_EXTENSIONS,
    )?;

    if targets.is_empty() {
        println!(
            "No screenshots required OCR (scanned: {}, skipped existing: {}, skipped other: {})",
            stats.scanned, stats.skipped_cached, stats.skipped_other
        );
        return Ok(());
    }

    if args.dry_run {
        for target in &targets {
            println!(
                "[dry-run] {} -> {}",
                target.path.display(),
                target
                    .sidecar
                    .file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default()
            );
        }
        println!(
            "Dry run only ({} candidates, scanned {}, skipped existing {}, skipped other {})",
            targets.len(),
            stats.scanned,
            stats.skipped_cached,
            stats.skipped_other
        );
        return Ok(());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/tools/run", base);

    let mut processed = 0usize;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    for target in targets.drain(..) {
        let path = target.path;
        if args.verbose {
            println!("Running OCR for {}", path.display());
        }
        let path_str = path.to_string_lossy().to_string();
        let mut payload = json!({
            "id": "ui.screenshot.ocr",
            "input": {
                "path": path_str,
                "lang": args.lang,
            }
        });
        if let Some(input) = payload.get_mut("input") {
            if let Some(ref backend) = args.backend {
                input["backend"] = JsonValue::String(backend.clone());
            }
            if let Some(ref quality) = args.quality {
                input["quality"] = JsonValue::String(quality.to_lowercase());
            }
            if args.prefer_low_power {
                input["prefer_low_power"] = JsonValue::Bool(true);
            }
            if args.refresh_capabilities {
                input["refresh_capabilities"] = JsonValue::Bool(true);
            }
        }
        if args.force {
            if let Some(input) = payload.get_mut("input") {
                input["force"] = JsonValue::Bool(true);
            }
        }
        let mut req = client.post(&url).json(&payload);
        if let Some(ref tok) = token {
            req = req.header("X-ARW-Admin", tok);
            req = req.bearer_auth(tok);
        }
        let resp = match req.send() {
            Ok(resp) => resp,
            Err(err) => {
                failures.push((path.clone(), err.to_string()));
                if args.verbose {
                    eprintln!("  error: {}", err);
                }
                continue;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            failures.push((path.clone(), format!("{}: {}", status, text)));
            if args.verbose {
                eprintln!("  server error: {} {}", status, text);
            }
            continue;
        }
        match resp.json::<JsonValue>() {
            Ok(body) => {
                processed += 1;
                if args.verbose {
                    let cached = body
                        .get("cached")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let lang = body
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&args.lang);
                    println!("  ok (lang={}, cached={})", lang, cached);
                }
            }
            Err(err) => {
                failures.push((path.clone(), err.to_string()));
                if args.verbose {
                    eprintln!("  parse error: {}", err);
                }
            }
        }
    }

    println!(
        "OCR backfill complete: processed {}, failures {}, scanned {}, skipped existing {} (other {})",
        processed,
        failures.len(),
        stats.scanned,
        stats.skipped_cached,
        stats.skipped_other
    );
    if !failures.is_empty() {
        eprintln!("Failures:");
        for (path, err) in failures {
            eprintln!("  {} => {}", path.display(), err);
        }
        return Err(anyhow!("some OCR requests failed"));
    }
    Ok(())
}

fn sanitize_lang_fragment_cli(lang: &str) -> String {
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

fn collect_screenshot_targets(
    screenshots_dir: &Path,
    lang_fragment: &str,
    force: bool,
    limit: Option<usize>,
    extensions: &[&str],
) -> Result<(Vec<ScreenshotTarget>, ScanStats)> {
    let mut stats = ScanStats::default();
    let mut targets = Vec::new();

    for entry in WalkDir::new(screenshots_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(ext) => ext.to_ascii_lowercase(),
            None => {
                stats.skipped_other += 1;
                continue;
            }
        };
        if !extensions.contains(&ext.as_str()) {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem,
            None => {
                stats.skipped_other += 1;
                continue;
            }
        };
        if stem.ends_with(".ann") || stem.contains(".ocr.") {
            stats.skipped_other += 1;
            continue;
        }

        stats.scanned += 1;
        let parent = match path.parent() {
            Some(parent) => parent,
            None => continue,
        };
        let sidecar = parent.join(format!("{}.ocr.{}.json", stem, lang_fragment));
        if !force && sidecar.exists() {
            stats.skipped_cached += 1;
            continue;
        }

        targets.push(ScreenshotTarget {
            path: path.to_path_buf(),
            sidecar,
        });

        if let Some(limit) = limit {
            if targets.len() >= limit {
                break;
            }
        }
    }

    Ok((targets, stats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn sanitize_lang_fragment_cli_normalizes() {
        assert_eq!(sanitize_lang_fragment_cli("ENg"), "eng");
        assert_eq!(sanitize_lang_fragment_cli("fr+best"), "fr+best");
        assert_eq!(sanitize_lang_fragment_cli(" zh - Hans "), "zh_-_hans");
        assert_eq!(sanitize_lang_fragment_cli(""), "eng");
        assert_eq!(sanitize_lang_fragment_cli("@!#"), "___");
    }

    #[test]
    fn collect_screenshot_targets_skips_sidecars_and_respects_limit() -> Result<()> {
        const EXT: &[&str] = &["png"];
        let tmp = TempDir::new()?;
        std::fs::create_dir_all(tmp.path().join("nested"))?;
        std::fs::write(tmp.path().join("one.png"), b"img")?;
        std::fs::write(tmp.path().join("one.ocr.eng.json"), b"{}")?;
        std::fs::write(tmp.path().join("two.png"), b"img2")?;
        std::fs::write(tmp.path().join("two.ocr.eng.json"), b"{}")?;
        std::fs::write(tmp.path().join("three.png"), b"img3")?;
        std::fs::write(tmp.path().join("nested").join("four.png"), b"img4")?;
        std::fs::write(tmp.path().join("nested").join("four.ocr.eng.json"), b"{}")?;
        let (targets, stats) = collect_screenshot_targets(tmp.path(), "eng", false, Some(5), EXT)?;
        assert_eq!(stats.scanned, 4);
        assert_eq!(stats.skipped_cached, 3);
        assert_eq!(stats.skipped_other, 0);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].path.ends_with("three.png"));
        Ok(())
    }

    #[test]
    fn collect_screenshot_targets_forces_when_requested() -> Result<()> {
        const EXT: &[&str] = &["png"];
        let tmp = TempDir::new()?;
        std::fs::write(tmp.path().join("one.png"), b"img")?;
        std::fs::write(
            tmp.path().join("one.ocr.eng.json"),
            json!({"cached": true}).to_string(),
        )?;
        let (targets, stats) = collect_screenshot_targets(tmp.path(), "eng", true, Some(1), EXT)?;
        assert_eq!(stats.scanned, 1);
        assert_eq!(stats.skipped_cached, 0);
        assert_eq!(targets.len(), 1);
        Ok(())
    }
}
