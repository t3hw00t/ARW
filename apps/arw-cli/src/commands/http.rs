use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use clap::{Args, Subcommand, ValueEnum};
use reqwest::blocking::Client;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};

use crate::{resolve_admin_token, submit_action_payload, wait_for_action};

#[derive(Subcommand)]
pub enum HttpCmd {
    /// Fetch a URL using the built-in http.fetch tool
    Fetch(HttpFetchArgs),
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum HttpMethod {
    Get,
    Post,
}

impl HttpMethod {
    fn action_kind(self) -> &'static str {
        match self {
            HttpMethod::Get => "net.http.get",
            HttpMethod::Post => "net.http.post",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
        }
    }
}

#[derive(Clone)]
pub struct HeaderArg {
    pub name: String,
    pub value: String,
}

impl FromStr for HeaderArg {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err("headers must be in key:value format".into());
        }
        let name = parts[0].trim();
        let value = parts[1].trim();
        if name.is_empty() {
            return Err("header name cannot be empty".into());
        }
        Ok(HeaderArg {
            name: name.to_string(),
            value: value.to_string(),
        })
    }
}

#[derive(Args, Clone)]
pub struct HttpBaseArgs {
    /// Base URL of the service handling actions
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Request timeout when talking to arw-server (seconds)
    #[arg(long, default_value_t = 10)]
    pub timeout: u64,
}

impl HttpBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args, Clone)]
pub struct HttpFetchArgs {
    #[command(flatten)]
    pub base: HttpBaseArgs,
    /// URL to fetch (scheme required)
    pub url: String,
    /// HTTP method to use
    #[arg(long, value_enum, default_value = "get")]
    pub method: HttpMethod,
    /// Request header (repeatable, format: Key: Value)
    #[arg(long = "header")]
    pub headers: Vec<HeaderArg>,
    /// Inline request body (POST only)
    #[arg(long)]
    pub data: Option<String>,
    /// Load request body from file (POST only)
    #[arg(long)]
    pub data_file: Option<PathBuf>,
    /// Override Content-Type header for POST requests
    #[arg(long)]
    pub content_type: Option<String>,
    /// Inject connector credentials by id
    #[arg(long)]
    pub connector_id: Option<String>,
    /// Wait timeout for action completion (seconds)
    #[arg(long, default_value_t = 60)]
    pub wait_timeout_secs: u64,
    /// Emit raw JSON instead of a formatted summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Write the body preview bytes to a file
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Print the body preview as base64 instead of UTF-8 text
    #[arg(long)]
    pub raw_preview: bool,
    /// Override the preview size captured from the response head (kilobytes, 1-1024)
    #[arg(long = "preview-kb")]
    pub preview_kb: Option<u32>,
}

pub fn execute(cmd: HttpCmd) -> Result<()> {
    match cmd {
        HttpCmd::Fetch(args) => cmd_http_fetch(&args),
    }
}

struct PreviewData {
    bytes: Vec<u8>,
    truncated: bool,
}

fn validate_preview_kb(value: Option<u32>) -> Result<Option<u32>> {
    if let Some(kb) = value {
        if kb == 0 {
            bail!("--preview-kb must be greater than zero");
        }
        if kb > 1024 {
            bail!("--preview-kb maximum is 1024 KB");
        }
        Ok(Some(kb))
    } else {
        Ok(None)
    }
}

fn cmd_http_fetch(args: &HttpFetchArgs) -> Result<()> {
    if args.data.is_some() && args.data_file.is_some() {
        bail!("--data and --data-file cannot be used together");
    }

    if matches!(args.method, HttpMethod::Get) && (args.data.is_some() || args.data_file.is_some()) {
        eprintln!("warning: GET requests ignore request bodies; use --method POST if needed");
    }

    let timeout = Duration::from_secs(args.base.timeout.max(1));
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.base.admin_token);
    let base = args.base.base_url().to_string();

    let mut body = args.data.clone();
    if let Some(path) = &args.data_file {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading request body from {}", path.display()))?;
        body = Some(
            String::from_utf8(bytes)
                .map_err(|_| anyhow!("request body file is not valid UTF-8"))?,
        );
    }

    let mut headers_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for header in &args.headers {
        headers_map
            .entry(header.name.clone())
            .or_default()
            .push(header.value.clone());
    }
    if let Some(ct) = &args.content_type {
        headers_map
            .entry("content-type".to_string())
            .or_default()
            .push(ct.clone());
    }

    let mut input = JsonMap::new();
    input.insert("url".into(), JsonValue::String(args.url.clone()));
    input.insert(
        "method".into(),
        JsonValue::String(args.method.as_str().to_string()),
    );
    if !headers_map.is_empty() {
        let mut headers_obj = JsonMap::new();
        for (name, values) in headers_map {
            let value = if values.len() == 1 {
                JsonValue::String(values[0].clone())
            } else {
                JsonValue::Array(values.into_iter().map(JsonValue::String).collect())
            };
            headers_obj.insert(name, value);
        }
        input.insert("headers".into(), JsonValue::Object(headers_obj));
    }
    if let Some(ct) = &args.content_type {
        input.insert("content_type".into(), JsonValue::String(ct.clone()));
    }
    if let Some(body) = body {
        if matches!(args.method, HttpMethod::Post) {
            input.insert("body".into(), JsonValue::String(body));
        }
    }
    if let Some(connector_id) = &args.connector_id {
        input.insert(
            "connector_id".into(),
            JsonValue::String(connector_id.clone()),
        );
    }

    let preview_kb = validate_preview_kb(args.preview_kb)?;
    if let Some(kb) = preview_kb {
        input.insert("head_kb".into(), JsonValue::Number(JsonNumber::from(kb)));
    }

    let action_id = submit_action_payload(
        &client,
        &base,
        token.as_deref(),
        args.method.action_kind(),
        JsonValue::Object(input),
    )?;

    let action = wait_for_action(
        &client,
        &base,
        token.as_deref(),
        &action_id,
        Duration::from_secs(args.wait_timeout_secs.max(1)),
    )?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&action).unwrap_or_else(|_| action.to_string())
            );
        } else {
            println!("{}", action);
        }
        if let Some(path) = &args.output {
            if let Some(preview) = extract_preview_data_from_action(&action)? {
                std::fs::write(path, &preview.bytes)
                    .with_context(|| format!("writing preview to {}", path.display()))?;
            }
        }
        return Ok(());
    }

    let output = action
        .get("output")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("action output missing"))?;

    let preview = extract_preview_data(output)?;
    if let Some(path) = &args.output {
        if let Some(data) = &preview {
            std::fs::write(path, &data.bytes)
                .with_context(|| format!("writing preview to {}", path.display()))?;
        } else {
            eprintln!(
                "(no preview bytes available to write to {})",
                path.display()
            );
        }
    }

    render_http_summary(&action, output, preview.as_ref(), args.raw_preview)?;
    Ok(())
}

fn extract_preview_data_from_action(action: &JsonValue) -> Result<Option<PreviewData>> {
    if let Some(output) = action.get("output").and_then(|v| v.as_object()) {
        extract_preview_data(output)
    } else {
        Ok(None)
    }
}

fn extract_preview_data(output: &JsonMap<String, JsonValue>) -> Result<Option<PreviewData>> {
    if let Some(b64) = output.get("body_base64").and_then(|v| v.as_str()) {
        let bytes = BASE64
            .decode(b64)
            .map_err(|e| anyhow!("failed to decode body_base64: {e}"))?;
        return Ok(Some(PreviewData {
            bytes,
            truncated: false,
        }));
    }
    if let Some(b64) = output.get("body_head_b64").and_then(|v| v.as_str()) {
        let bytes = BASE64
            .decode(b64)
            .map_err(|e| anyhow!("failed to decode body_head_b64: {e}"))?;
        let truncated = output
            .get("body_truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return Ok(Some(PreviewData { bytes, truncated }));
    }
    Ok(None)
}

fn preview_base64(output: &JsonMap<String, JsonValue>) -> Option<&str> {
    output
        .get("body_base64")
        .and_then(|v| v.as_str())
        .or_else(|| output.get("body_head_b64").and_then(|v| v.as_str()))
}

fn render_http_summary(
    action: &JsonValue,
    output: &JsonMap<String, JsonValue>,
    preview: Option<&PreviewData>,
    raw_preview: bool,
) -> Result<()> {
    let action_id = action
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let state = action
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    println!("Action: {} ({})", action_id, state);

    if let Some(err) = output.get("error").and_then(|v| v.as_str()) {
        println!("Error : {}", err);
        if let Some(missing) = output.get("missing_scopes").and_then(|v| v.as_array()) {
            if !missing.is_empty() {
                let scopes = missing
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("Missing scopes: {}", scopes);
            }
        }
        return Ok(());
    }

    let status = output
        .get("status")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let status_text = output
        .get("status_text")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if status_text.is_empty() {
        println!("Status: {}", status);
    } else {
        println!("Status: {} {}", status, status_text);
    }

    let url = output.get("url").and_then(|v| v.as_str()).unwrap_or("");
    println!("URL   : {}", url);
    if let Some(final_url) = output
        .get("final_url")
        .and_then(|v| v.as_str())
        .filter(|final_url| !final_url.is_empty() && *final_url != url)
    {
        println!("Final : {}", final_url);
    }

    let bytes_in = output.get("bytes_in").and_then(|v| v.as_i64()).unwrap_or(0);
    let bytes_out = output
        .get("bytes_out")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let elapsed = output.get("elapsed_ms").and_then(|v| v.as_u64());
    match elapsed {
        Some(ms) => println!(
            "Bytes : in {} out {} (elapsed {} ms)",
            bytes_in, bytes_out, ms
        ),
        None => println!("Bytes : in {} out {}", bytes_in, bytes_out),
    }

    if let Some(posture) = output.get("posture").and_then(|v| v.as_str()) {
        println!("Posture: {}", posture);
    }

    if let Some(content_type) = output
        .get("content_type")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        println!("Type  : {}", content_type);
    }

    if let Some(guard) = output.get("guard").and_then(|v| v.as_object()) {
        if let Some(allowed) = guard.get("allowed").and_then(|v| v.as_bool()) {
            println!("Guard : {}", if allowed { "allowed" } else { "blocked" });
        }
        if let Some(required_caps) = guard
            .get("required_capabilities")
            .and_then(|v| v.as_array())
        {
            let caps: Vec<_> = required_caps.iter().filter_map(|v| v.as_str()).collect();
            if !caps.is_empty() {
                println!("Required capabilities: {}", caps.join(", "));
            }
        }
        if let Some(lease_id) = guard
            .get("lease")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("id"))
            .and_then(|v| v.as_str())
        {
            println!("Lease : {}", lease_id);
        }
    }

    if let Some(headers) = output.get("headers").and_then(|v| v.as_object()) {
        if !headers.is_empty() {
            println!("Headers:");
            for (name, value) in headers.iter().take(20) {
                match value {
                    JsonValue::String(s) => println!("  {}: {}", name, s),
                    JsonValue::Array(arr) => {
                        for val in arr.iter().filter_map(|v| v.as_str()) {
                            println!("  {}: {}", name, val);
                        }
                    }
                    other => println!("  {}: {}", name, other),
                }
            }
            if headers.len() > 20 {
                println!("  … ({} more)", headers.len() - 20);
            }
        }
    }

    if raw_preview {
        if let Some(b64) = preview_base64(output) {
            let truncated_note = if preview.map(|p| p.truncated).unwrap_or_else(|| {
                output
                    .get("body_truncated")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            }) {
                " (truncated head)"
            } else {
                ""
            };
            println!("Preview{}:\n{}", truncated_note, b64);
        } else {
            println!("Preview: (no base64 preview available)");
        }
        return Ok(());
    }

    let preview_text = output
        .get("body_preview_utf8")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            preview.map(|data| {
                let text = String::from_utf8_lossy(&data.bytes);
                text.to_string()
            })
        });

    if let Some(text) = preview_text {
        let truncated = preview.map(|p| p.truncated).unwrap_or_else(|| {
            output
                .get("body_truncated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        });
        let preview_bytes = output
            .get("body_preview_bytes")
            .and_then(|v| v.as_i64())
            .unwrap_or(preview.map(|p| p.bytes.len() as i64).unwrap_or(0));
        println!(
            "Preview ({} bytes{}):",
            preview_bytes,
            if truncated { ", truncated" } else { "" }
        );
        println!("{}", text);
    } else {
        println!("Preview: (not available)");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_preview_kb_allows_valid_range() {
        assert_eq!(validate_preview_kb(Some(128)).unwrap(), Some(128));
        assert_eq!(validate_preview_kb(None).unwrap(), None);
    }

    #[test]
    fn validate_preview_kb_rejects_zero() {
        assert!(validate_preview_kb(Some(0)).is_err());
    }

    #[test]
    fn validate_preview_kb_rejects_over_limit() {
        assert!(validate_preview_kb(Some(2048)).is_err());
    }

    #[test]
    fn http_preview_prefers_body_base64() {
        let mut map = JsonMap::new();
        map.insert(
            "body_base64".into(),
            JsonValue::String(BASE64.encode(b"Hello World")),
        );
        map.insert(
            "body_head_b64".into(),
            JsonValue::String(BASE64.encode(b"He")),
        );
        map.insert("body_truncated".into(), JsonValue::Bool(true));

        let preview = extract_preview_data(&map)
            .expect("preview ok")
            .expect("preview some");
        assert_eq!(preview.bytes, b"Hello World");
        assert!(!preview.truncated);
        assert_eq!(
            preview_base64(&map).expect("base64 available"),
            BASE64.encode(b"Hello World")
        );
    }

    #[test]
    fn http_preview_uses_head_when_full_body_missing() {
        let mut map = JsonMap::new();
        map.insert(
            "body_head_b64".into(),
            JsonValue::String(BASE64.encode(b"He")),
        );
        map.insert("body_truncated".into(), JsonValue::Bool(true));

        let preview = extract_preview_data(&map)
            .expect("preview ok")
            .expect("preview some");
        assert_eq!(preview.bytes, b"He");
        assert!(preview.truncated);
        assert_eq!(
            preview_base64(&map).expect("base64 available"),
            BASE64.encode(b"He")
        );
    }
}
