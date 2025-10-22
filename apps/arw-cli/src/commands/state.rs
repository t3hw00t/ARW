use std::io::{self, BufRead, BufReader, Write};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Utc};
use clap::{Args, Subcommand};
use json_patch::{patch as apply_json_patch, Patch as JsonPatch};
use reqwest::{blocking::Client, header::ACCEPT, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::util::{
    ellipsize_str, format_elapsed_since_with_now, format_local_timestamp,
    format_observation_timestamp, format_relative_from_now, parse_relative_duration,
    resolve_admin_token, with_admin_headers,
};

#[derive(Subcommand)]
pub enum StateCmd {
    /// Snapshot filtered actions via /state/actions
    Actions(StateActionsArgs),
    /// Inspect identity registry via /state/identity
    Identity(StateIdentityArgs),
    /// Inspect cluster registry via /state/cluster
    Cluster(StateClusterArgs),
}

#[derive(Args)]
pub struct StateActionsArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Maximum number of items to request (server clamps 1-2000)
    #[arg(long)]
    limit: Option<usize>,
    /// Filter by action state (queued|running|completed|failed)
    #[arg(long)]
    state: Option<String>,
    /// Restrict kinds by prefix (e.g., chat.)
    #[arg(long)]
    kind_prefix: Option<String>,
    /// Only include actions updated at or after this RFC3339 timestamp
    #[arg(long, conflicts_with = "updated_relative")]
    updated_since: Option<String>,
    /// Relative lookback for action updates (e.g., 30m, 4h) converted to RFC3339
    #[arg(
        long = "updated-relative",
        value_name = "WINDOW",
        conflicts_with = "updated_since"
    )]
    updated_relative: Option<String>,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Width for the rendered kind column in text output (ignored in JSON mode)
    #[arg(long, default_value_t = 36)]
    kind_width: usize,
    /// Stream live updates via state.read.model.patch SSE
    #[arg(long, conflicts_with = "json")]
    watch: bool,
}

#[derive(Args)]
pub struct StateIdentityArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
pub struct StateClusterArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliIdentitySnapshot {
    #[serde(default)]
    pub(crate) loaded_ms: Option<u64>,
    #[serde(default)]
    pub(crate) source_path: Option<String>,
    #[serde(default)]
    pub(crate) version: Option<u32>,
    #[serde(default)]
    pub(crate) principals: Vec<CliIdentityPrincipal>,
    #[serde(default, rename = "env_principals")]
    pub(crate) env: Vec<CliIdentityPrincipal>,
    #[serde(default)]
    pub(crate) diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliClusterSnapshot {
    #[serde(default)]
    nodes: Vec<CliClusterNode>,
    #[serde(default)]
    generated: Option<String>,
    #[serde(default)]
    generated_ms: Option<u64>,
    #[serde(default)]
    ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliClusterNode {
    id: String,
    role: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    health: Option<String>,
    #[serde(default)]
    capabilities: Option<JsonValue>,
    #[serde(default)]
    models: Option<JsonValue>,
    #[serde(default)]
    last_seen: Option<String>,
    #[serde(default)]
    last_seen_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CliIdentityPrincipal {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) display_name: Option<String>,
    #[serde(default)]
    pub(crate) roles: Vec<String>,
    #[serde(default)]
    pub(crate) scopes: Vec<String>,
    #[serde(default)]
    pub(crate) tokens: Option<usize>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
}

pub fn execute(cmd: StateCmd) -> Result<()> {
    match cmd {
        StateCmd::Actions(args) => cmd_state_actions(&args),
        StateCmd::Identity(args) => cmd_state_identity(&args),
        StateCmd::Cluster(args) => cmd_state_cluster(&args),
    }
}

pub(crate) fn render_identity_snapshot(snapshot: &CliIdentitySnapshot) {
    println!("Identity registry snapshot");
    let loaded = snapshot
        .loaded_ms
        .map(format_local_timestamp)
        .unwrap_or_else(|| "—".to_string());
    let source = snapshot
        .source_path
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("—");
    let version = snapshot
        .version
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".into());

    println!("  Loaded : {}", loaded);
    println!("  Source : {}", source);
    println!("  Version: {}", version);
    println!(
        "  Principals: {} (file) · {} (env)",
        snapshot.principals.len(),
        snapshot.env.len()
    );

    if !snapshot.diagnostics.is_empty() {
        println!("\nDiagnostics:");
        for diag in &snapshot.diagnostics {
            println!("  - {}", diag);
        }
    }

    let mut entries: Vec<(&str, &CliIdentityPrincipal)> = Vec::new();
    for principal in &snapshot.principals {
        entries.push(("file", principal));
    }
    for principal in &snapshot.env {
        entries.push(("env", principal));
    }

    if entries.is_empty() {
        println!("\n(no principals loaded)");
        return;
    }

    entries.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    println!(
        "\n{:<4} {:<24} {:<18} {:<28} {:<8} Name / Notes",
        "Src", "ID", "Roles", "Scopes", "Tokens"
    );
    for (source, principal) in entries {
        let id_display = ellipsize_str(&principal.id, 24);
        let roles_display = if principal.roles.is_empty() {
            "—".to_string()
        } else {
            ellipsize_str(&principal.roles.join(", "), 18)
        };
        let scopes_display = if principal.scopes.is_empty() {
            "—".to_string()
        } else {
            ellipsize_str(&principal.scopes.join(", "), 28)
        };
        let tokens_display = principal
            .tokens
            .filter(|count| *count > 0)
            .map(|count| count.to_string())
            .unwrap_or_else(|| "—".into());
        let name_notes = match (
            principal
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            principal
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            (Some(name), Some(note)) => format!("{} · {}", name, note),
            (Some(name), None) => name.to_string(),
            (None, Some(note)) => note.to_string(),
            (None, None) => "—".into(),
        };
        println!(
            "{:<4} {:<24} {:<18} {:<28} {:<8} {}",
            source, id_display, roles_display, scopes_display, tokens_display, name_notes
        );
    }
}

fn cmd_state_actions(args: &StateActionsArgs) -> Result<()> {
    if args.watch && args.json {
        bail!("--watch cannot be combined with --json output");
    }

    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let filters = ActionFilters::from_args(args)?;

    let mut full_snapshot = fetch_full_actions(&client, base, token.as_deref())?;
    let view = build_filtered_actions_view(&full_snapshot, &filters)?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&view).unwrap_or_else(|_| view.to_string())
            );
        } else {
            println!("{}", view);
        }
        return Ok(());
    }

    render_actions_text(&view, args, None)?;

    if args.watch {
        eprintln!("watching actions; press Ctrl-C to exit");
        watch_actions(base, token.as_deref(), &filters, args, &mut full_snapshot)?;
    }

    Ok(())
}

fn cmd_state_identity(args: &StateIdentityArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/identity", base);
    let response = with_admin_headers(
        client.get(&url).header(ACCEPT, "application/json"),
        token.as_deref(),
    )
    .send()
    .with_context(|| format!("requesting identity snapshot from {}", url))?;

    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!(
            "request to {} returned 401 Unauthorized; supply an admin token via --admin-token or ARW_ADMIN_TOKEN",
            url
        );
    }
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read body>".into());
        bail!("request to {} failed ({}): {}", url, status, body);
    }

    let raw: serde_json::Value = response
        .json()
        .context("parsing identity snapshot JSON payload")?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&raw).unwrap_or_else(|_| raw.to_string())
            );
        } else {
            println!("{}", raw);
        }
        return Ok(());
    }

    let snapshot: CliIdentitySnapshot =
        serde_json::from_value(raw).context("materialising identity snapshot structure")?;
    render_identity_snapshot(&snapshot);
    Ok(())
}

fn cmd_state_cluster(args: &StateClusterArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/cluster", base);
    let response = with_admin_headers(
        client.get(&url).header(ACCEPT, "application/json"),
        token.as_deref(),
    )
    .send()
    .with_context(|| format!("requesting cluster snapshot from {}", url))?;

    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!(
            "request to {} returned 401 Unauthorized; supply an admin token via --admin-token or ARW_ADMIN_TOKEN",
            url
        );
    }
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read body>".into());
        bail!("request to {} failed ({}): {}", url, status, body);
    }

    let raw: serde_json::Value = response
        .json()
        .context("parsing cluster snapshot JSON payload")?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&raw).unwrap_or_else(|_| raw.to_string())
            );
        } else {
            println!("{}", raw);
        }
        return Ok(());
    }

    let snapshot: CliClusterSnapshot =
        serde_json::from_value(raw).context("materialising cluster snapshot structure")?;
    render_cluster_snapshot(&snapshot);
    Ok(())
}

fn render_cluster_snapshot(snapshot: &CliClusterSnapshot) {
    println!("Cluster snapshot");
    let generated = snapshot
        .generated_ms
        .map(format_local_timestamp)
        .or_else(|| snapshot.generated.clone())
        .unwrap_or_else(|| "—".to_string());
    let ttl = snapshot.ttl_seconds.unwrap_or(0);
    println!("  Generated : {}", generated);
    if ttl > 0 {
        println!("  TTL       : {}s", ttl);
    } else {
        println!("  TTL       : —");
    }
    println!("  Nodes     : {}", snapshot.nodes.len());

    if snapshot.nodes.is_empty() {
        println!("\n  (no nodes advertised)");
        return;
    }

    println!();
    println!(
        "{:<20} {:<10} {:<10} {:<32} {:<6} {:<8} Capabilities",
        "ID", "Role", "Health", "Last Seen", "Stale", "Models"
    );

    let now_raw = Utc::now().timestamp_millis();
    let now_ms = if now_raw < 0 { 0 } else { now_raw as u64 };
    let ttl_ms = snapshot.ttl_seconds.unwrap_or(0).saturating_mul(1_000);

    for node in &snapshot.nodes {
        let id_display = if let Some(name) = node.name.as_deref() {
            if !name.is_empty() {
                format!("{} ({})", node.id, name)
            } else {
                node.id.clone()
            }
        } else {
            node.id.clone()
        };
        let role = node.role.to_lowercase();
        let health = node.health.clone().unwrap_or_else(|| "—".into());
        let last_seen_ms = node.last_seen_ms.unwrap_or(0);
        let base_last = node
            .last_seen_ms
            .map(format_local_timestamp)
            .or_else(|| node.last_seen.clone())
            .unwrap_or_else(|| "—".to_string());
        let last_seen = if last_seen_ms > 0 && now_ms > 0 {
            format!(
                "{} ({})",
                base_last,
                format_relative_from_now(last_seen_ms, now_ms)
            )
        } else {
            base_last
        };
        let stale = if ttl_ms == 0 || last_seen_ms == 0 {
            "no"
        } else if now_ms > last_seen_ms {
            if now_ms - last_seen_ms > ttl_ms {
                "yes"
            } else {
                "no"
            }
        } else {
            "no"
        };
        let models = summarize_models_field(&node.models);
        let capabilities = summarize_capabilities_field(&node.capabilities);

        println!(
            "{:<20} {:<10} {:<10} {:<32} {:<6} {:<8} {}",
            truncate_pad(&id_display, 20),
            truncate_pad(&role, 10),
            truncate_pad(&health, 10),
            truncate_pad(&last_seen, 32),
            stale,
            truncate_pad(&models, 8),
            truncate_pad(&capabilities, 40)
        );
    }
}

fn summarize_capabilities_field(raw: &Option<JsonValue>) -> String {
    let Some(value) = raw else {
        return "—".into();
    };
    match value {
        JsonValue::Object(map) => {
            if map.is_empty() {
                return "—".into();
            }
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let rendered: Vec<&str> = keys.into_iter().take(3).collect();
            let mut out = rendered.join(",");
            if map.len() > rendered.len() {
                if !out.is_empty() {
                    out.push_str(",…");
                } else {
                    out.push('…');
                }
            }
            if out.is_empty() {
                "—".into()
            } else {
                out
            }
        }
        JsonValue::Array(items) => {
            if items.is_empty() {
                return "—".into();
            }
            let mut chunks = Vec::new();
            for v in items.iter().take(3) {
                if let Some(s) = v.as_str() {
                    chunks.push(s);
                }
            }
            if chunks.is_empty() {
                "—".into()
            } else {
                let mut out = chunks.join(",");
                if items.len() > chunks.len() {
                    out.push_str(",…");
                }
                out
            }
        }
        JsonValue::String(s) => {
            if s.is_empty() {
                "—".into()
            } else {
                s.clone()
            }
        }
        other => other.to_string(),
    }
}

fn summarize_models_field(raw: &Option<JsonValue>) -> String {
    let Some(value) = raw else {
        return "—".into();
    };
    match value {
        JsonValue::Object(map) => {
            if let Some(count) = map.get("count").and_then(JsonValue::as_u64) {
                let mut out = count.to_string();
                if let Some(preview) = map.get("preview").and_then(JsonValue::as_array) {
                    if !preview.is_empty() {
                        let mut tags = Vec::new();
                        for entry in preview.iter().take(2) {
                            if let Some(s) = entry.as_str() {
                                tags.push(shorten_hash(s));
                            }
                        }
                        if !tags.is_empty() {
                            out.push(' ');
                            out.push('(');
                            out.push_str(&tags.join(","));
                            if preview.len() > tags.len() {
                                out.push_str(",…");
                            }
                            out.push(')');
                        }
                    }
                }
                out
            } else {
                "—".into()
            }
        }
        JsonValue::Number(num) => num.to_string(),
        JsonValue::String(s) => {
            if s.is_empty() {
                "—".into()
            } else {
                s.clone()
            }
        }
        _ => "—".into(),
    }
}

fn shorten_hash(input: &str) -> String {
    if input.len() <= 8 {
        input.to_string()
    } else {
        input[..8].to_string()
    }
}

fn truncate_pad(input: &str, width: usize) -> String {
    if input.len() <= width {
        let mut s = input.to_string();
        if s.len() < width {
            s.push_str(&" ".repeat(width - s.len()));
        }
        s
    } else {
        let mut out = input[..width.saturating_sub(1)].to_string();
        out.push('…');
        out
    }
}

fn fetch_full_actions(client: &Client, base: &str, token: Option<&str>) -> Result<JsonValue> {
    let url = format!("{}/state/actions", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing actions response")?;
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn build_filtered_actions_view(snapshot: &JsonValue, filters: &ActionFilters) -> Result<JsonValue> {
    let version = snapshot
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let items = snapshot
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut selected: Vec<JsonValue> = Vec::new();
    for item in items.iter() {
        if let Some(state) = filters.state.as_deref() {
            let current_state = item.get("state").and_then(|v| v.as_str()).unwrap_or("");
            if !current_state.eq_ignore_ascii_case(state) {
                continue;
            }
        }
        if let Some(prefix) = filters.kind_prefix.as_deref() {
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !kind.starts_with(prefix) {
                continue;
            }
        }
        if let Some(cutoff) = filters.updated_since {
            if let Some(updated_raw) = item.get("updated").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(updated_raw) {
                    if ts.with_timezone(&Utc) <= cutoff {
                        continue;
                    }
                }
            }
        }
        selected.push(item.clone());
        if let Some(limit) = filters.limit {
            if selected.len() >= limit {
                break;
            }
        }
    }

    Ok(json!({
        "version": version,
        "items": selected,
    }))
}

fn render_actions_text(
    body: &JsonValue,
    args: &StateActionsArgs,
    update_note: Option<&str>,
) -> Result<()> {
    let version = body.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(note) = update_note {
        println!();
        println!(
            "[{}] Actions update ({} items, version {})",
            note,
            items.len(),
            version
        );
    } else {
        println!(
            "Actions snapshot ({} items, version {})",
            items.len(),
            version
        );
        let mut filters: Vec<String> = Vec::new();
        push_filter_str(&mut filters, "state=", args.state.as_deref());
        push_filter_str(&mut filters, "kind_prefix=", args.kind_prefix.as_deref());
        push_filter_usize(&mut filters, "limit=", args.limit);
        push_filter_str(
            &mut filters,
            "updated_relative=",
            args.updated_relative.as_deref(),
        );
        push_filter_str(&mut filters, "updated>", args.updated_since.as_deref());
        if !filters.is_empty() {
            println!("Filters: {}", filters.join(", "));
        }
    }

    if items.is_empty() {
        println!("(no actions matched the filters)");
        return Ok(());
    }

    let kind_width = args.kind_width.max(8);
    println!(
        "{:<28} {:<10} {:<10} {:<width$} Id",
        "Updated",
        "Age",
        "State",
        "Kind",
        width = kind_width
    );

    let now_utc = Utc::now();

    for item in items {
        let updated_raw = item.get("updated").and_then(|v| v.as_str()).unwrap_or("");
        let updated_display = if updated_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(updated_raw)
        };
        let age_display = if updated_raw.is_empty() {
            "-".to_string()
        } else {
            format_elapsed_since_with_now(updated_raw, now_utc).unwrap_or_else(|| "-".to_string())
        };
        let state_display = item
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let kind_display = item
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|k| ellipsize_str(k, kind_width))
            .unwrap_or_else(|| "-".to_string());
        let id_display = item.get("id").and_then(|v| v.as_str()).unwrap_or("-");

        println!(
            "{:<28} {:<10} {:<10} {:<width$} {}",
            updated_display,
            age_display,
            state_display,
            kind_display,
            id_display,
            width = kind_width
        );
    }

    io::stdout().flush().ok();
    Ok(())
}

fn watch_actions(
    base: &str,
    token: Option<&str>,
    filters: &ActionFilters,
    args: &StateActionsArgs,
    snapshot: &mut JsonValue,
) -> Result<()> {
    let mut last_event_id: Option<String> = None;
    let mut backoff_secs = 1u64;
    loop {
        match stream_actions_once(
            base,
            token,
            last_event_id.as_deref(),
            snapshot,
            filters,
            args,
        ) {
            Ok(next_id) => {
                if let Some(id) = next_id {
                    last_event_id = Some(id);
                }
                backoff_secs = 1;
            }
            Err(err) => {
                eprintln!("watch error: {err:?}");
                backoff_secs = (backoff_secs * 2).min(30);
            }
        }
        thread::sleep(Duration::from_secs(backoff_secs));
    }
}

fn stream_actions_once(
    base: &str,
    token: Option<&str>,
    last_event_id: Option<&str>,
    snapshot: &mut JsonValue,
    filters: &ActionFilters,
    args: &StateActionsArgs,
) -> Result<Option<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .context("building SSE client")?;

    let mut req = client
        .get(format!("{}/events", base))
        .query(&[("prefix", "state.read.model.patch"), ("replay", "0")])
        .header(ACCEPT, "text/event-stream");
    if let Some(id) = last_event_id {
        req = req.header("Last-Event-ID", id);
    }
    req = with_admin_headers(req, token);

    let resp = req.send().context("connecting to /events stream")?;
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        bail!("events stream failed with status {}", status);
    }

    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut event_name = String::new();
    let mut data_buf = String::new();
    let mut event_id_line: Option<String> = None;
    let mut latest_id = last_event_id.map(|s| s.to_string());

    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(latest_id);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if line.is_empty() {
            if event_name == "state.read.model.patch" && !data_buf.is_empty() {
                if let Err(err) = handle_actions_patch(&data_buf, snapshot, filters, args) {
                    eprintln!("failed to process patch: {err:?}");
                } else if let Some(id_val) = event_id_line.as_ref() {
                    latest_id = Some(id_val.clone());
                }
            }
            event_name.clear();
            data_buf.clear();
            event_id_line = None;
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = line.strip_prefix("id:") {
            event_id_line = Some(rest.trim().to_string());
            continue;
        }
    }
}

fn handle_actions_patch(
    data: &str,
    snapshot: &mut JsonValue,
    filters: &ActionFilters,
    args: &StateActionsArgs,
) -> Result<()> {
    let env: JsonValue = serde_json::from_str(data).context("decoding SSE payload")?;
    let payload = env.get("payload").cloned().unwrap_or(env.clone());
    let rm = payload.get("payload").cloned().unwrap_or(payload.clone());
    let read_model_id = rm
        .get("id")
        .or_else(|| rm.get("read_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if read_model_id != "actions" {
        return Ok(());
    }
    let patch_value = match rm.get("patch") {
        Some(v) if v.is_array() => v.clone(),
        _ => return Ok(()),
    };
    let patch: JsonPatch =
        serde_json::from_value(patch_value).context("decoding JSON Patch for actions")?;
    apply_json_patch(snapshot, &patch).context("applying actions patch")?;
    let view = build_filtered_actions_view(snapshot, filters)?;
    let version = view.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let note = format!("{} (version {})", Local::now().format("%H:%M:%S"), version);
    render_actions_text(&view, args, Some(&note))?;
    Ok(())
}

fn resolve_updated_since(args: &StateActionsArgs) -> Result<Option<DateTime<Utc>>> {
    resolve_updated_since_with_now(
        args.updated_since.as_deref(),
        args.updated_relative.as_deref(),
        Utc::now(),
    )
}

fn resolve_updated_since_with_now(
    absolute: Option<&str>,
    relative: Option<&str>,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    if let Some(raw) = relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--updated-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        return Ok(Some(now - duration));
    }

    if let Some(raw) = absolute {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--updated-since cannot be empty");
        }
        let parsed = DateTime::parse_from_rfc3339(trimmed)
            .with_context(|| format!("failed to parse updated_since='{}'", trimmed))?;
        return Ok(Some(parsed.with_timezone(&Utc)));
    }

    Ok(None)
}

#[derive(Clone)]
struct ActionFilters {
    limit: Option<usize>,
    state: Option<String>,
    kind_prefix: Option<String>,
    updated_since: Option<DateTime<Utc>>,
}

impl ActionFilters {
    fn from_args(args: &StateActionsArgs) -> Result<Self> {
        let state = args
            .state
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let kind_prefix = args
            .kind_prefix
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let updated_since = resolve_updated_since(args)?;
        Ok(Self {
            limit: args.limit.map(|v| v.clamp(1, 2000)),
            state,
            kind_prefix,
            updated_since,
        })
    }
}

pub(crate) fn push_filter_str(filters: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(raw) = value {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            filters.push(format!("{}{}", label, trimmed));
        }
    }
}

pub(crate) fn push_filter_usize(filters: &mut Vec<String>, label: &str, value: Option<usize>) {
    if let Some(v) = value {
        filters.push(format!("{}{}", label, v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn resolve_updated_since_handles_relative_window() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let resolved = resolve_updated_since_with_now(None, Some("45m"), now)
            .expect("relative updated timestamp")
            .expect("timestamp");
        assert_eq!(resolved, now - chrono::Duration::minutes(45));
    }

    #[test]
    fn resolve_updated_since_handles_absolute_cursor() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let target = "2025-10-02T11:30:00Z";
        let resolved = resolve_updated_since_with_now(Some(target), None, now)
            .expect("absolute updated timestamp")
            .expect("timestamp");
        let expected = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 11, 30, 0)
            .single()
            .expect("construct expected timestamp");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_updated_since_rejects_empty_inputs() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        assert!(resolve_updated_since_with_now(Some("   "), None, now).is_err());
        assert!(resolve_updated_since_with_now(None, Some(""), now).is_err());
    }

    #[test]
    fn push_filter_str_trims_and_skips_empty() {
        let mut filters = Vec::new();
        push_filter_str(&mut filters, "state=", Some(" queued "));
        assert_eq!(filters, vec!["state=queued".to_string()]);

        push_filter_str(&mut filters, "state=", Some("   "));
        assert_eq!(filters, vec!["state=queued".to_string()]);

        push_filter_str(&mut filters, "state=", None);
        assert_eq!(filters, vec!["state=queued".to_string()]);
    }

    #[test]
    fn push_filter_usize_records_values() {
        let mut filters = Vec::new();
        push_filter_usize(&mut filters, "limit=", Some(25));
        assert_eq!(filters, vec!["limit=25".to_string()]);

        push_filter_usize(&mut filters, "limit=", None);
        assert_eq!(filters, vec!["limit=25".to_string()]);
    }
}
