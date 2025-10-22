use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use clap::{Args, Subcommand};
use json_patch::{patch as apply_json_patch, Patch as JsonPatch};
use reqwest::{blocking::Client, header::ACCEPT, StatusCode};
use serde_json::{json, Value as JsonValue};

use super::state::{push_filter_str, push_filter_usize};
use super::util::{
    ellipsize_str, format_elapsed_since_with_now, format_observation_timestamp,
    format_payload_snippet, parse_relative_duration, resolve_admin_token, truncate_payload,
    with_admin_headers,
};

#[derive(Subcommand)]
pub enum EventsCmd {
    /// Snapshot the observations read-model via /state/observations
    Observations(EventsObservationsArgs),
    /// Tail the journal via /admin/events/journal
    Journal(EventsJournalArgs),
    /// Tail modular events (modular.agent/tool accepted) with sensible defaults
    Modular(ModularTailArgs),
}

#[derive(Args)]
pub struct EventsObservationsArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,
    /// Maximum number of items to request (defaults to server window when omitted)
    #[arg(long)]
    pub limit: Option<usize>,
    /// Filter to observation kinds starting with this prefix (e.g., actions.)
    #[arg(long)]
    pub kind_prefix: Option<String>,
    /// Only include observations newer than this RFC3339 timestamp
    #[arg(long, conflicts_with = "since_relative")]
    pub since: Option<String>,
    /// Relative lookback (e.g., 15m, 2h30m) converted to an absolute `since`
    #[arg(long, value_name = "WINDOW", conflicts_with = "since")]
    pub since_relative: Option<String>,
    /// Emit raw JSON instead of a formatted summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Maximum characters of payload JSON to show per row (set 0 to hide)
    #[arg(long, default_value_t = 120)]
    pub payload_width: usize,
    /// Include policy metadata if present on events
    #[arg(long)]
    pub show_policy: bool,
    /// Stream live updates via state.read.model.patch SSE
    #[arg(long, conflicts_with = "json")]
    pub watch: bool,
}

#[derive(Args, Clone)]
pub struct EventsJournalArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,
    /// Maximum number of entries to request (server caps at 1000)
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
    /// CSV of event prefixes to include (dot.case)
    #[arg(long)]
    pub prefix: Option<String>,
    /// Emit raw JSON instead of text summary
    #[arg(long, conflicts_with = "follow")]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Show journal source files in text mode
    #[arg(long)]
    pub show_sources: bool,
    /// Poll continuously for new entries
    #[arg(long)]
    pub follow: bool,
    /// Poll interval in seconds when following (default 5)
    #[arg(long, default_value_t = 5, requires = "follow")]
    pub interval: u64,
    /// Skip entries at or before this RFC3339 timestamp on the first fetch
    #[arg(long = "after")]
    pub after_cursor: Option<String>,
    /// Skip entries older than this relative window on the first fetch (e.g. 15m, 2h30m)
    #[arg(
        long = "after-relative",
        value_name = "WINDOW",
        conflicts_with = "after_cursor"
    )]
    pub after_relative: Option<String>,
    /// Maximum characters to display for payload/policy lines (0 hides them)
    #[arg(long, default_value_t = 160)]
    pub payload_width: usize,
}

#[derive(Args, Clone)]
pub struct ModularTailArgs {
    #[command(flatten)]
    pub journal: EventsJournalArgs,
}

pub(crate) fn execute(cmd: EventsCmd) -> Result<()> {
    match cmd {
        EventsCmd::Observations(args) => cmd_events_observations(&args),
        EventsCmd::Journal(args) => cmd_events_journal(&args),
        EventsCmd::Modular(args) => cmd_events_modular(&args),
    }
}

fn cmd_events_observations(args: &EventsObservationsArgs) -> Result<()> {
    if args.watch && args.json {
        bail!("--watch cannot be combined with --json output");
    }

    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let since_resolution = resolve_since_param(args)?;
    let filters = ObservationFilters::from_args(args, &since_resolution)?;

    let mut full_snapshot = fetch_full_observations(&client, base, token.as_deref())?;
    let view = build_filtered_observations_view(&full_snapshot, &filters)?;

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

    render_observations_text(&view, args, &since_resolution, None)?;

    if args.watch {
        eprintln!("watching observations; press Ctrl-C to exit");
        watch_observations(
            base,
            token.as_deref(),
            &filters,
            args,
            &since_resolution,
            &mut full_snapshot,
        )?;
    }

    Ok(())
}

#[derive(Clone)]
struct ObservationFilters {
    limit: Option<usize>,
    kind_prefix: Option<String>,
    since_cutoff: Option<DateTime<Utc>>,
}

impl ObservationFilters {
    fn from_args(args: &EventsObservationsArgs, since: &SinceResolution) -> Result<Self> {
        let kind_prefix = args
            .kind_prefix
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let since_cutoff = match since.query {
            Some(ref iso) => {
                let parsed = DateTime::parse_from_rfc3339(iso)
                    .with_context(|| format!("failed to parse since='{}'", iso))?;
                Some(parsed.with_timezone(&Utc))
            }
            None => None,
        };
        Ok(Self {
            limit: args.limit,
            kind_prefix,
            since_cutoff,
        })
    }
}

fn fetch_full_observations(client: &Client, base: &str, token: Option<&str>) -> Result<JsonValue> {
    let url = format!("{}/state/observations", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing observations response")?;
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}
fn build_filtered_observations_view(
    snapshot: &JsonValue,
    filters: &ObservationFilters,
) -> Result<JsonValue> {
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
    for item in items.iter().rev() {
        if let Some(prefix) = filters.kind_prefix.as_deref() {
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !kind.starts_with(prefix) {
                continue;
            }
        }
        if let Some(cutoff) = filters.since_cutoff {
            if let Some(time_raw) = item.get("time").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(time_raw) {
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
    selected.reverse();

    Ok(json!({
        "version": version,
        "items": selected,
    }))
}

fn render_observations_text(
    body: &JsonValue,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
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
            "[{}] Observations update ({} items, version {})",
            note,
            items.len(),
            version
        );
    } else {
        println!(
            "Observations snapshot ({} items, version {})",
            items.len(),
            version
        );
        let mut filters: Vec<String> = Vec::new();
        push_filter_str(&mut filters, "prefix=", args.kind_prefix.as_deref());
        push_filter_usize(&mut filters, "limit=", args.limit);
        if let Some(ref label) = since_resolution.relative_display {
            if !label.is_empty() {
                filters.push(label.clone());
            }
        }
        if let Some(ref label) = since_resolution.display {
            if !label.is_empty() {
                filters.push(label.clone());
            }
        }
        if args.payload_width == 0 {
            filters.push("payload hidden".to_string());
        } else if args.payload_width != 120 {
            filters.push(format!("payload_width={}", args.payload_width));
        }
        if !filters.is_empty() {
            println!("Filters: {}", filters.join(", "));
        }
    }

    if items.is_empty() {
        if update_note.is_some() {
            println!("(no observations matched filters)");
        }
        return Ok(());
    }

    println!("{:<28} {:<10} {:<36} Payload", "Time", "Age", "Kind");

    let now_utc = Utc::now();

    for item in items {
        let time_raw = item.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let when = if time_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(time_raw)
        };
        let age_display = if time_raw.is_empty() {
            "-".to_string()
        } else {
            format_elapsed_since_with_now(time_raw, now_utc).unwrap_or_else(|| "-".to_string())
        };
        let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("-");
        let kind_display = ellipsize_str(kind, 36);

        let payload_display = if args.payload_width == 0 {
            "-".to_string()
        } else if let Some(payload) = item.get("payload") {
            format_payload_snippet(payload, args.payload_width)
        } else {
            "-".to_string()
        };

        let mut extras: Vec<String> = Vec::new();
        if args.show_policy {
            if let Some(policy) = item.get("policy") {
                let snippet = format_payload_snippet(policy, args.payload_width.max(48));
                if snippet != "-" {
                    extras.push(format!("policy={}", snippet));
                }
            }
            if let Some(ce) = item.get("ce") {
                let snippet = format_payload_snippet(ce, args.payload_width.max(48));
                if snippet != "-" {
                    extras.push(format!("ce={}", snippet));
                }
            }
        }
        let extra_str = if extras.is_empty() {
            String::new()
        } else {
            format!(" {}", extras.join(" "))
        };

        println!(
            "{:<28} {:<10} {:<36} {}{}",
            when, age_display, kind_display, payload_display, extra_str
        );
    }

    io::stdout().flush().ok();
    Ok(())
}
fn watch_observations(
    base: &str,
    token: Option<&str>,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
    snapshot: &mut JsonValue,
) -> Result<()> {
    let mut last_event_id: Option<String> = None;
    let mut backoff_secs = 1u64;
    loop {
        match stream_observations_once(
            base,
            token,
            last_event_id.as_deref(),
            snapshot,
            filters,
            args,
            since_resolution,
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

fn stream_observations_once(
    base: &str,
    token: Option<&str>,
    last_event_id: Option<&str>,
    snapshot: &mut JsonValue,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
) -> Result<Option<String>> {
    let client = Client::builder()
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
                if let Err(err) =
                    handle_observations_patch(&data_buf, snapshot, filters, args, since_resolution)
                {
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

fn handle_observations_patch(
    data: &str,
    snapshot: &mut JsonValue,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
) -> Result<()> {
    let env: JsonValue = serde_json::from_str(data).context("decoding SSE payload")?;
    let payload = env.get("payload").cloned().unwrap_or(env.clone());
    let rm = payload.get("payload").cloned().unwrap_or(payload.clone());
    let read_model_id = rm
        .get("id")
        .or_else(|| rm.get("read_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if read_model_id != "observations" {
        return Ok(());
    }
    let patch_value = match rm.get("patch") {
        Some(v) if v.is_array() => v.clone(),
        _ => return Ok(()),
    };
    let patch: JsonPatch =
        serde_json::from_value(patch_value).context("decoding JSON Patch for observations")?;
    apply_json_patch(snapshot, &patch).context("applying observations patch")?;
    let view = build_filtered_observations_view(snapshot, filters)?;
    let version = view.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let note = format!("{} (version {})", Local::now().format("%H:%M:%S"), version);
    render_observations_text(&view, args, since_resolution, Some(&note))?;
    Ok(())
}
struct SinceResolution {
    query: Option<String>,
    display: Option<String>,
    relative_display: Option<String>,
}

fn resolve_since_param(args: &EventsObservationsArgs) -> Result<SinceResolution> {
    if let Some(ref raw) = args.since_relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--since-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        let ts = (Utc::now() - duration).to_rfc3339_opts(SecondsFormat::Millis, true);
        return Ok(SinceResolution {
            query: Some(ts.clone()),
            display: Some(format!("since>{}", ts)),
            relative_display: Some(format!("since_relative={}", trimmed)),
        });
    }

    if let Some(ref raw) = args.since {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--since cannot be empty");
        }
        return Ok(SinceResolution {
            query: Some(trimmed.to_string()),
            display: Some(format!("since>{}", trimmed)),
            relative_display: None,
        });
    }

    Ok(SinceResolution {
        query: None,
        display: None,
        relative_display: None,
    })
}

fn resolve_after_timestamp(
    args: &EventsJournalArgs,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    resolve_after_timestamp_with_now(
        args.after_cursor.as_deref(),
        args.after_relative.as_deref(),
        Utc::now(),
    )
}

fn resolve_after_timestamp_with_now(
    absolute: Option<&str>,
    relative: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    if let Some(raw) = relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("--after-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        return Ok(Some(now - duration));
    }

    if let Some(cursor) = absolute {
        let trimmed = cursor.trim();
        if trimmed.is_empty() {
            bail!("--after cannot be empty");
        }
        return match chrono::DateTime::parse_from_rfc3339(trimmed) {
            Ok(dt) => Ok(Some(dt.with_timezone(&chrono::Utc))),
            Err(_) => bail!("--after must be an RFC3339 timestamp (e.g. 2025-10-02T17:15:00Z)"),
        };
    }

    Ok(None)
}

fn cmd_events_journal(args: &EventsJournalArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let after_time = resolve_after_timestamp(args)?;

    let mut body = fetch_journal_snapshot(
        &client,
        base,
        token.as_deref(),
        args.limit,
        args.prefix.as_deref(),
    )?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
            );
        } else {
            println!("{}", body);
        }
        return Ok(());
    }

    let mut filter_summaries: Vec<String> = Vec::new();
    push_filter_str(&mut filter_summaries, "prefix=", args.prefix.as_deref());
    push_filter_str(
        &mut filter_summaries,
        "after_relative=",
        args.after_relative.as_deref(),
    );
    push_filter_str(
        &mut filter_summaries,
        "after>",
        args.after_cursor.as_deref(),
    );
    if args.limit != 200 {
        filter_summaries.push(format!("limit={}", args.limit));
    }
    if args.payload_width == 0 {
        filter_summaries.push("payload hidden".to_string());
    } else if args.payload_width != 160 {
        filter_summaries.push(format!("payload_width={}", args.payload_width));
    }
    if args.after_relative.is_some() {
        if let Some(ref cursor) = after_time {
            filter_summaries.push(format!(
                "after>= {}",
                cursor.to_rfc3339_opts(SecondsFormat::Secs, true)
            ));
        }
    }
    if !filter_summaries.is_empty() {
        println!("Filters: {}", filter_summaries.join(", "));
    }

    let mut first_pass = true;
    let mut state = if args.follow {
        Some(JournalPrintState::new(args.limit.max(512)))
    } else {
        None
    };

    loop {
        let apply_after = if first_pass {
            after_time.as_ref()
        } else {
            None
        };
        let _printed = render_journal_text(
            &body,
            args.show_sources,
            first_pass,
            apply_after,
            args.payload_width,
            state.as_mut(),
        );
        if !args.follow {
            return Ok(());
        }
        first_pass = false;
        thread::sleep(Duration::from_secs(args.interval.max(1)));
        body = fetch_journal_snapshot(
            &client,
            base,
            token.as_deref(),
            args.limit,
            args.prefix.as_deref(),
        )?;
    }
}

fn cmd_events_modular(args: &ModularTailArgs) -> Result<()> {
    let mut journal = args.journal.clone();
    if journal
        .prefix
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        journal.prefix = Some("modular.".to_string());
    }
    if !journal.json {
        journal.follow = true;
        if journal.interval == 5 {
            journal.interval = 3;
        }
        if journal.payload_width == 160 {
            journal.payload_width = 200;
        }
    }
    if journal.limit == 200 {
        journal.limit = 100;
    }
    cmd_events_journal(&journal)
}
fn fetch_journal_snapshot(
    client: &Client,
    base: &str,
    token: Option<&str>,
    limit: usize,
    prefix: Option<&str>,
) -> Result<JsonValue> {
    let url = format!("{}/admin/events/journal", base);
    let mut params: Vec<(String, String)> = vec![("limit".into(), limit.to_string())];
    if let Some(pref) = prefix {
        let trimmed = pref.trim();
        if !trimmed.is_empty() {
            params.push(("prefix".into(), trimmed.to_string()));
        }
    }
    let mut req = client.get(&url);
    if !params.is_empty() {
        req = req.query(&params);
    }
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if status == StatusCode::NOT_FOUND {
        bail!("journal disabled: ensure the server runs with ARW_KERNEL_ENABLE=1");
    }
    let body: JsonValue = resp.json().context("parsing journal response")?;
    if !status.is_success() {
        bail!("journal request failed: {} {}", status, body);
    }
    Ok(body)
}

fn render_journal_text(
    body: &JsonValue,
    show_sources: bool,
    first_pass: bool,
    after: Option<&chrono::DateTime<chrono::Utc>>,
    payload_width: usize,
    mut state: Option<&mut JournalPrintState>,
) -> usize {
    let limit = body
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let total = body
        .get("total_matched")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let truncated = body
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let skipped = body
        .get("skipped_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let prefixes: Vec<String> = body
        .get("prefixes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let entries = body
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let source_files: Vec<String> = body
        .get("source_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut printable: Vec<(JsonValue, String)> = Vec::new();
    for entry in entries {
        let key = entry_identity(&entry);
        if let Some(st) = state.as_ref() {
            if st.seen(&key) {
                continue;
            }
        }
        if let Some(after_ts) = after {
            if let Some(entry_ts) = entry_timestamp(&entry) {
                if entry_ts <= *after_ts {
                    if let Some(st) = state.as_mut() {
                        st.record(key);
                    }
                    continue;
                }
            }
        }
        printable.push((entry, key));
    }

    if first_pass {
        let prefix_label = if prefixes.is_empty() {
            "(none)".to_string()
        } else {
            prefixes.join(", ")
        };
        println!(
            "Journal entries: returned {} (limit {}), total matches {}, truncated: {}, skipped lines {}",
            printable.len(),
            limit,
            total,
            truncated,
            skipped
        );
        println!("Prefixes: {}", prefix_label);
        if show_sources && !source_files.is_empty() {
            println!("Sources:");
            for path in source_files {
                println!("  {}", path);
            }
        }
        if printable.is_empty() {
            println!("No journal entries matched the query.");
            return 0;
        }
    } else if printable.is_empty() {
        return 0;
    } else {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S");
        println!("-- poll @ {}: {} new entries --", now, printable.len());
    }

    let mut new_count = 0usize;
    let now_utc = Utc::now();
    for (entry, key) in printable {
        let time_raw = entry.get("time").and_then(|v| v.as_str());
        let kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let (time_display, age_display) = if let Some(raw) = time_raw {
            let display = format_observation_timestamp(raw);
            let age =
                format_elapsed_since_with_now(raw, now_utc).unwrap_or_else(|| "-".to_string());
            (display, age)
        } else {
            ("-".to_string(), "-".to_string())
        };
        println!("[{} | {}] {}", time_display, age_display, kind);
        if payload_width > 0 {
            let payload = entry.get("payload").cloned().unwrap_or(JsonValue::Null);
            let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "null".into());
            println!(
                "  payload: {}",
                truncate_payload(&payload_str, payload_width)
            );
            if let Some(policy) = entry.get("policy") {
                if !policy.is_null() {
                    let policy_str = serde_json::to_string(policy).unwrap_or_else(|_| "{}".into());
                    println!("  policy: {}", truncate_payload(&policy_str, payload_width));
                }
            }
            if let Some(ce) = entry.get("ce") {
                if !ce.is_null() {
                    let ce_str = serde_json::to_string(ce).unwrap_or_else(|_| "{}".into());
                    println!("  ce: {}", truncate_payload(&ce_str, payload_width));
                }
            }
        }
        if let Some(st) = state.as_mut() {
            st.record(key);
        }
        new_count += 1;
    }

    new_count
}
struct JournalPrintState {
    seen: HashSet<String>,
    order: VecDeque<String>,
    cap: usize,
}

impl JournalPrintState {
    fn new(cap: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            cap: cap.max(64),
        }
    }

    fn seen(&self, key: &str) -> bool {
        self.seen.contains(key)
    }

    fn record(&mut self, key: String) {
        if self.seen.insert(key.clone()) {
            self.order.push_back(key);
            if self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }
}

fn entry_identity(entry: &JsonValue) -> String {
    let payload = entry.get("payload").cloned().unwrap_or(JsonValue::Null);
    let policy = entry.get("policy").cloned().unwrap_or(JsonValue::Null);
    let ce = entry.get("ce").cloned().unwrap_or(JsonValue::Null);
    format!(
        "{}|{}|{}|{}|{}",
        entry.get("time").and_then(|v| v.as_str()).unwrap_or(""),
        entry.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
        serde_json::to_string(&payload).unwrap_or_default(),
        serde_json::to_string(&policy).unwrap_or_default(),
        serde_json::to_string(&ce).unwrap_or_default()
    )
}

fn entry_timestamp(entry: &JsonValue) -> Option<chrono::DateTime<chrono::Utc>> {
    let time_str = entry.get("time")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(time_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn resolve_after_timestamp_handles_relative_window() {
        let now = Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let resolved = resolve_after_timestamp_with_now(None, Some("15m"), now)
            .expect("relative after timestamp")
            .expect("timestamp");
        assert_eq!(resolved, now - chrono::Duration::minutes(15));
    }

    #[test]
    fn resolve_after_timestamp_handles_absolute_cursor() {
        let now = Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let target = "2025-10-02T11:59:00Z";
        let resolved = resolve_after_timestamp_with_now(Some(target), None, now)
            .expect("absolute after timestamp")
            .expect("timestamp");
        let expected = Utc
            .with_ymd_and_hms(2025, 10, 2, 11, 59, 0)
            .single()
            .expect("construct expected timestamp");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_after_timestamp_rejects_empty_inputs() {
        let now = Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        assert!(resolve_after_timestamp_with_now(Some("  \t"), None, now).is_err());
        assert!(resolve_after_timestamp_with_now(None, Some(""), now).is_err());
    }
}
