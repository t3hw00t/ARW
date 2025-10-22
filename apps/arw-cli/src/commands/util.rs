use std::env;
use std::fs::{self, create_dir_all, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use base64::Engine;
use chrono::{DateTime, Local, TimeZone, Utc};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;
use serde_json::Value as JsonValue;

/// Resolve the admin token, preferring CLI argument over environment variable.
pub(crate) fn resolve_admin_token(opt: &Option<String>) -> Option<String> {
    opt.clone()
        .or_else(|| env::var("ARW_ADMIN_TOKEN").ok())
        .filter(|s| !s.trim().is_empty())
}

/// Resolve the persona id, preferring CLI argument over the `ARW_PERSONA_ID` env var.
pub(crate) fn resolve_persona_id(opt: &Option<String>) -> Option<String> {
    resolve_persona_id_with_envs(opt, &["ARW_PERSONA_ID"])
}

/// Resolve the persona id, preferring CLI argument and then the provided environment keys.
pub(crate) fn resolve_persona_id_with_envs(
    opt: &Option<String>,
    env_keys: &[&str],
) -> Option<String> {
    if let Some(value) = opt
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    for key in env_keys {
        if let Ok(raw) = env::var(key) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Attach both legacy and bearer admin headers when a token is provided.
pub(crate) fn with_admin_headers(mut req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
    if let Some(tok) = token {
        req = req.header("X-ARW-Admin", tok);
        req = req.bearer_auth(tok);
    }
    req
}

/// Format bytes into a human-readable string (e.g., `1.2 GB`).
pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

/// Format floating-point bytes into a human-readable string (e.g., `850.5 MB`).
pub(crate) fn format_bytes_f64(bytes: f64) -> String {
    if bytes <= 0.0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{:.0} {}", value.round(), UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

/// Format a millisecond duration as a compact label (e.g., `1m05s`).
pub(crate) fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    if ms < 60_000 {
        return format!("{:.1} s", ms as f64 / 1_000.0);
    }
    let total_secs = ms / 1_000;
    if total_secs < 3_600 {
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        return format!("{minutes}m{seconds:02}s");
    }
    let hours = total_secs / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    let seconds = total_secs % 60;
    format!("{hours}h{minutes:02}m{seconds:02}s")
}

pub(crate) fn format_seconds(secs: u64) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3_600 {
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{minutes}m{seconds:02}s")
    } else {
        let hours = secs / 3_600;
        let minutes = (secs % 3_600) / 60;
        let seconds = secs % 60;
        format!("{hours}h{minutes:02}m{seconds:02}s")
    }
}

/// Format seconds (with fractional component) into a human-readable label.
pub(crate) fn format_seconds_f64(secs: f64) -> String {
    if secs < 1.0 {
        return format!("{:.0} ms", secs * 1_000.0);
    }
    if secs < 60.0 {
        return format!("{:.1} s", secs);
    }
    let total_secs = secs.floor() as u64;
    let remainder = secs - total_secs as f64;
    let base = format_seconds(total_secs);
    if remainder >= 0.5 {
        // include ~0.5s remainder to avoid discarding observable fractional seconds
        format!("{base} (~{secs:.1}s)")
    } else {
        base
    }
}

/// Format an epoch timestamp (ms) into a local datetime string.
pub(crate) fn format_local_timestamp(ms: u64) -> String {
    match Utc.timestamp_millis_opt(ms as i64).single() {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S %Z")
            .to_string(),
        None => "(invalid timestamp)".to_string(),
    }
}

/// Express a timestamp difference (ms) in relative terms (e.g., `in 5m`).
pub(crate) fn format_relative_from_now(target_ms: u64, now_ms: u64) -> String {
    let diff = target_ms as i128 - now_ms as i128;
    let future = diff >= 0;
    let abs = if future {
        diff as u128
    } else {
        (-diff) as u128
    };
    let seconds = (abs / 1_000) as u64;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let label = if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    };
    if future {
        format!("in {label}")
    } else {
        format!("{label} ago")
    }
}

/// Validate base64-encoded trust keys for supported algorithms.
pub(crate) fn validate_trust_key(alg: &str, key_b64: &str) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(key_b64)
        .context("decoding base64 key")?;
    match alg {
        "ed25519" => {
            if bytes.len() != 32 {
                bail!("ed25519 keys must be 32 bytes (got {})", bytes.len());
            }
        }
        "secp256k1" => {
            if !(bytes.len() == 33 || bytes.len() == 64 || bytes.len() == 65) {
                bail!(
                    "secp256k1 keys should be 33, 64, or 65 bytes (got {})",
                    bytes.len()
                );
            }
        }
        _ => bail!("unsupported algorithm '{alg}'"),
    }
    Ok(())
}

/// Truncate payload strings to a maximum length, appending ellipsis when truncated.
pub(crate) fn truncate_payload(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

/// Submit an action payload and return the resulting action id.
pub(crate) fn submit_action_payload(
    client: &Client,
    base: &str,
    admin_token: Option<&str>,
    persona_id: Option<&str>,
    kind: &str,
    input: JsonValue,
) -> Result<String> {
    let mut payload = serde_json::Map::new();
    payload.insert("kind".to_string(), JsonValue::String(kind.to_string()));
    payload.insert("input".to_string(), input);
    if let Some(pid) = persona_id {
        payload.insert("persona_id".to_string(), JsonValue::String(pid.to_string()));
    }
    let payload = JsonValue::Object(payload);

    let response = with_admin_headers(client.post(format!("{base}/actions")), admin_token)
        .json(&payload)
        .send()
        .context("submit action")?
        .error_for_status()
        .context("submit action status")?;

    let body: JsonValue = response
        .json()
        .context("parse action submission response")?;
    if let Some(id) = body.get("id").and_then(|v| v.as_str()) {
        return Ok(id.to_string());
    }
    if let Some(action) = body.get("action").and_then(|v| v.as_object()) {
        if let Some(id) = action.get("id").and_then(|v| v.as_str()) {
            return Ok(id.to_string());
        }
    }
    bail!("action submission response missing id: {body}");
}

/// Poll for an action to complete, returning the final document.
pub(crate) fn wait_for_action(
    client: &Client,
    base: &str,
    admin_token: Option<&str>,
    action_id: &str,
    timeout: Duration,
) -> Result<JsonValue> {
    let deadline = Instant::now() + timeout;
    let url = format!("{base}/actions/{action_id}");
    loop {
        let mut request = client.get(&url);
        if let Some(token) = admin_token {
            request = request.bearer_auth(token);
        }
        match request.send() {
            Ok(resp) => {
                if resp.status() == StatusCode::NOT_FOUND {
                    if Instant::now() >= deadline {
                        bail!("action {action_id} not found before timeout");
                    }
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
                let resp = resp.error_for_status().context("action status request")?;
                let doc: JsonValue = resp.json().context("parse action status response")?;
                match doc.get("state").and_then(|v| v.as_str()).unwrap_or("") {
                    "completed" => return Ok(doc),
                    "queued" | "running" => {
                        if Instant::now() >= deadline {
                            bail!(
                                "action {action_id} did not complete in time (last state {})",
                                doc.get("state")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown"),
                            );
                        }
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    other => bail!("unexpected action state {other}: {doc}"),
                }
            }
            Err(err) => {
                if Instant::now() >= deadline {
                    bail!("failed to fetch action {action_id} status: {err}");
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
}

/// Append text output to a file, optionally rotating when the file grows beyond `rotate_limit`.
pub(crate) fn append_text_output(
    path: &Path,
    stamp: Option<&str>,
    text: &str,
    rotate_limit: Option<u64>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
        }
    }
    if let Some(limit) = rotate_limit {
        maybe_rotate_output(path, limit)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening output file {}", path.display()))?;
    if let Some(stamp_value) = stamp {
        writeln!(file, "=== {} ===", stamp_value)?;
    }
    writeln!(file, "{}", text.trim_end())?;
    writeln!(file)?;
    Ok(())
}

/// Append JSON to a file, optionally rotating when the file grows beyond `rotate_limit`.
pub(crate) fn append_json_output(
    path: &Path,
    body: &JsonValue,
    pretty: bool,
    rotate_limit: Option<u64>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
        }
    }
    if let Some(limit) = rotate_limit {
        maybe_rotate_output(path, limit)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening output file {}", path.display()))?;
    let text = if pretty {
        serde_json::to_string_pretty(body)?
    } else {
        body.to_string()
    };
    writeln!(file, "{}", text)?;
    Ok(())
}

/// Parse human-friendly byte limits (e.g., `64KB`, `3m`) for log rotation thresholds.
pub(crate) fn parse_byte_limit_arg(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("rotate limit must not be empty".into());
    }
    if trimmed.eq_ignore_ascii_case("0") {
        return Ok(0);
    }
    let digit_count = trimmed
        .chars()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (num_part, suffix_part) = trimmed.split_at(digit_count);
    if num_part.is_empty() {
        return Err("rotate limit must start with digits".into());
    }
    let base = num_part
        .parse::<u64>()
        .map_err(|_| "rotate limit digits out of range".to_string())?;
    let suffix = suffix_part.trim().to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" => 1u64,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024u64.pow(4),
        _ => {
            return Err("unsupported rotate suffix (use K, M, G, or T with optional B)".to_string())
        }
    };
    let value = base
        .checked_mul(multiplier)
        .ok_or_else(|| "rotate limit overflow".to_string())?;
    if value != 0 && value < 64 * 1024 {
        Err("rotate limit must be at least 64KB; see CLI docs for details".to_string())
    } else {
        Ok(value)
    }
}

/// Format an RFC3339 timestamp into a local time string suitable for CLI output.
pub(crate) fn format_observation_timestamp(raw: &str) -> String {
    match DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S%.3f %Z")
            .to_string(),
        Err(_) => raw.to_string(),
    }
}

/// Render a compact relative duration (e.g., `1h05m`) when the timestamp parses successfully.
pub(crate) fn format_elapsed_since_with_now(raw: &str, now: DateTime<Utc>) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?;
    let delta = now - parsed.with_timezone(&Utc);
    let seconds = delta.num_seconds().max(0);
    Some(format_compact_duration(seconds))
}

/// Render a compact, human-friendly duration label.
pub(crate) fn format_compact_duration(total_seconds: i64) -> String {
    let seconds = total_seconds.max(0);
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        if hours > 0 {
            format!("{}d{:02}h", days, hours)
        } else {
            format!("{}d", days)
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{}h{:02}m", hours, minutes)
        } else {
            format!("{}h", hours)
        }
    } else if minutes > 0 {
        if secs > 0 {
            format!("{}m{:02}s", minutes, secs)
        } else {
            format!("{}m", minutes)
        }
    } else {
        format!("{}s", secs)
    }
}

/// Return a single-line JSON payload preview truncated to `width` characters.
pub(crate) fn format_payload_snippet(value: &JsonValue, width: usize) -> String {
    if width == 0 {
        return "-".to_string();
    }
    let raw = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    let cleaned = raw.replace(['\n', '\r'], " ");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "-".to_string()
    } else {
        ellipsize_str(trimmed, width)
    }
}

/// Truncate a string to `max_chars`, appending an ellipsis where appropriate.
pub(crate) fn ellipsize_str(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = input.chars().count();
    if total <= max_chars {
        return input.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let mut out = String::with_capacity(max_chars);
    for ch in input.chars().take(max_chars - 3) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

/// Parse relative duration strings (e.g., `15m`, `2h30m`) into chrono durations.
pub(crate) fn parse_relative_duration(input: &str) -> Result<chrono::Duration> {
    let mut total_seconds: i64 = 0;
    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        if current.is_empty() {
            anyhow::bail!("expected digits before unit in '{}'", input);
        }
        let value: i64 = current
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid number in '{}'", input))?;
        current.clear();
        let unit = ch.to_ascii_lowercase();
        let component = match unit {
            's' => value,
            'm' => value
                .checked_mul(60)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            'h' => value
                .checked_mul(3_600)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            'd' => value
                .checked_mul(86_400)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            _ => anyhow::bail!("unsupported unit '{}' in '{}'", ch, input),
        };
        total_seconds = total_seconds
            .checked_add(component)
            .ok_or_else(|| anyhow::anyhow!("duration overflow"))?;
    }

    if !current.is_empty() {
        anyhow::bail!("missing unit after '{}' in '{}'", current, input);
    }
    if total_seconds <= 0 {
        anyhow::bail!("relative duration must be greater than zero");
    }
    Ok(chrono::Duration::seconds(total_seconds))
}

fn maybe_rotate_output(path: &Path, max_bytes: u64) -> Result<()> {
    if max_bytes == 0 {
        return Ok(());
    }
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() < max_bytes {
        return Ok(());
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let mut rotated = path.to_path_buf();
    rotated.set_file_name(format!("{}.prev", file_name));
    if rotated.exists() {
        fs::remove_file(&rotated).ok();
    }
    fs::rename(path, &rotated)
        .with_context(|| format!("rotating output file {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs;

    use tempfile::TempDir;

    #[test]
    fn append_text_output_rotates_when_limit_reached() -> Result<()> {
        let dir = TempDir::new()?;
        let log_path = dir.path().join("context.log");
        fs::write(&log_path, vec![b'x'; 16])?;

        append_text_output(&log_path, Some("2025-10-02 12:00:00"), "New entry", Some(8))?;

        let rotated = dir.path().join("context.log.prev");
        assert!(rotated.is_file());
        assert_eq!(fs::read(rotated)?, vec![b'x'; 16]);

        let fresh = fs::read_to_string(&log_path)?;
        assert!(fresh.contains("2025-10-02 12:00:00"));
        assert!(fresh.contains("New entry"));
        Ok(())
    }

    #[test]
    fn append_text_output_respects_no_rotation() -> Result<()> {
        let dir = TempDir::new()?;
        let log_path = dir.path().join("context.log");

        append_text_output(&log_path, Some("stamp"), "entry", None)?;
        append_text_output(&log_path, Some("stamp-2"), "entry-2", None)?;

        let updated = fs::read_to_string(&log_path)?;
        assert!(updated.contains("entry"));
        assert!(updated.contains("entry-2"));
        Ok(())
    }

    #[test]
    fn parse_byte_limit_arg_supports_suffixes() {
        assert_eq!(parse_byte_limit_arg("64KB").unwrap(), 64 * 1024);
        assert_eq!(parse_byte_limit_arg("3m").unwrap(), 3 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("4MB").unwrap(), 4 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("5G").unwrap(), 5 * 1024 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("0").unwrap(), 0);
    }

    #[test]
    fn parse_byte_limit_arg_rejects_invalid() {
        assert!(parse_byte_limit_arg("").is_err());
        assert!(parse_byte_limit_arg("kb").is_err());
        assert!(parse_byte_limit_arg("2KB").is_err());
        assert!(parse_byte_limit_arg("63KB").is_err());
        assert!(parse_byte_limit_arg("10x").is_err());
        assert!(parse_byte_limit_arg("1000000000000000000000000000000").is_err());
    }

    #[test]
    fn format_compact_duration_handles_units() {
        assert_eq!(format_compact_duration(42), "42s");
        assert_eq!(format_compact_duration(125), "2m05s");
        assert_eq!(format_compact_duration(3600), "1h");
        assert_eq!(format_compact_duration(3700), "1h01m");
        assert_eq!(format_compact_duration(86_400), "1d");
        assert_eq!(format_compact_duration(86_400 + 7_200), "1d02h");
    }

    #[test]
    fn format_elapsed_since_with_now_clamps_future() {
        let now = Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let future = "2025-10-02T12:05:00Z";
        let formatted = format_elapsed_since_with_now(future, now).expect("formatted");
        assert_eq!(formatted, "0s");
    }

    #[test]
    fn parse_relative_duration_supports_composites() {
        let duration = parse_relative_duration("1h30m").expect("duration");
        assert_eq!(duration.num_seconds(), 5_400);
    }

    #[test]
    fn parse_relative_duration_rejects_invalid_input() {
        assert!(parse_relative_duration("abc").is_err());
        assert!(parse_relative_duration("15").is_err());
        assert!(parse_relative_duration("0s").is_err());
    }

    #[test]
    fn ellipsize_str_enforces_max_width() {
        assert_eq!(ellipsize_str("hello", 0), "");
        assert_eq!(ellipsize_str("hello", 1), ".");
        assert_eq!(ellipsize_str("hello", 2), "..");
        assert_eq!(ellipsize_str("hello", 3), "...");
        assert_eq!(ellipsize_str("hello", 4), "h...");
        assert_eq!(ellipsize_str("hello", 5), "hello");
    }

    #[test]
    fn format_bytes_scales_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn truncate_payload_respects_limit() {
        assert_eq!(truncate_payload("hello", 10), "hello");
        assert_eq!(truncate_payload("hello", 4), "hell...");
        assert_eq!(truncate_payload("hello", 0), "...");
    }
}
