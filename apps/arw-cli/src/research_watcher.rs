use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use serde_json::{json, Map as JsonMap, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::{resolve_admin_token, with_admin_headers};

#[derive(Subcommand)]
pub enum ResearchWatcherCmd {
    /// List research watcher items (Suggested logic units)
    List(ResearchWatcherListArgs),
    /// Approve research watcher items
    Approve(ResearchWatcherDecideArgs),
    /// Archive research watcher items
    Archive(ResearchWatcherDecideArgs),
}

#[derive(Args, Clone)]
pub struct ResearchWatcherBaseArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Request timeout (seconds)
    #[arg(long, default_value_t = 10)]
    timeout: u64,
}

impl ResearchWatcherBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args, Clone)]
pub struct ResearchWatcherListArgs {
    #[command(flatten)]
    base: ResearchWatcherBaseArgs,
    /// Filter by status (pending, approved, archived)
    #[arg(long)]
    status: Option<String>,
    /// Limit number of items returned (1-500)
    #[arg(long)]
    limit: Option<i64>,
    /// Emit JSON response from the server
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
pub struct ResearchWatcherDecideArgs {
    #[command(flatten)]
    base: ResearchWatcherBaseArgs,
    /// Item ids to update (repeatable)
    #[arg(value_name = "ID")]
    ids: Vec<String>,
    /// Pull ids from the specified status bucket
    #[arg(long, value_name = "STATUS")]
    from_status: Option<String>,
    /// Limit items fetched via --from-status (1-500)
    #[arg(long)]
    limit: Option<i64>,
    /// Restrict to items with this source value (case-insensitive)
    #[arg(long)]
    filter_source: Option<String>,
    /// Restrict to items whose title/summary/payload text contains this substring
    #[arg(long)]
    filter_contains: Option<String>,
    /// Optional note recorded on the item
    #[arg(long)]
    note: Option<String>,
    /// Print actions without sending requests
    #[arg(long)]
    dry_run: bool,
    /// Emit JSON summary for updated items
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Clone, Copy)]
enum Decision {
    Approve,
    Archive,
}

impl Decision {
    fn endpoint(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Archive => "archive",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Archive => "archive",
        }
    }

    fn past_tense(self) -> &'static str {
        match self {
            Decision::Approve => "Approved",
            Decision::Archive => "Archived",
        }
    }

    fn verb_ing(self) -> &'static str {
        match self {
            Decision::Approve => "Approving",
            Decision::Archive => "Archiving",
        }
    }
}

pub fn run(cmd: ResearchWatcherCmd) -> Result<()> {
    match cmd {
        ResearchWatcherCmd::List(args) => list(args),
        ResearchWatcherCmd::Approve(args) => decide(args, Decision::Approve),
        ResearchWatcherCmd::Archive(args) => decide(args, Decision::Archive),
    }
}

fn list(args: ResearchWatcherListArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = build_client(args.base.timeout)?;
    let snapshot = fetch_snapshot(
        &client,
        args.base.base_url(),
        token.as_deref(),
        args.status.as_deref(),
        args.limit,
    )?;
    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| "{}".into())
            );
        } else {
            println!("{}", snapshot.to_string());
        }
        return Ok(());
    }
    let items = extract_items(&snapshot);
    if items.is_empty() {
        println!("(no research watcher items)");
        return Ok(());
    }
    print_table(&items);
    Ok(())
}

fn decide(args: ResearchWatcherDecideArgs, decision: Decision) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = build_client(args.base.timeout)?;
    let base = args.base.base_url().to_string();

    let mut targets: Vec<String> = args
        .ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    let mut fetched: HashMap<String, Value> = HashMap::new();

    if let Some(status) = args.from_status.as_deref() {
        let snapshot = fetch_snapshot(&client, &base, token.as_deref(), Some(status), args.limit)?;
        let items = extract_items(&snapshot);
        let filtered = filter_items(
            &items,
            args.filter_source.as_deref(),
            args.filter_contains.as_deref(),
        );
        if filtered.is_empty() {
            bail!("no research watcher items matched filters");
        }
        for item in filtered {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                if !targets.iter().any(|existing| existing == id) {
                    targets.push(id.to_string());
                }
                fetched.insert(id.to_string(), item);
            }
        }
    }

    if targets.is_empty() {
        bail!("provide at least one item id or use --from-status to select items");
    }

    let mut seen = HashSet::new();
    targets.retain(|id| seen.insert(id.clone()));

    if args.dry_run {
        println!(
            "{} {} item{}:",
            decision.verb_ing(),
            targets.len(),
            if targets.len() == 1 { "" } else { "s" }
        );
        for id in &targets {
            println!("  - {}", id);
        }
        if let Some(note) = args.note.as_deref().filter(|s| !s.trim().is_empty()) {
            println!("Note: {}", note);
        }
        return Ok(());
    }

    let note = args
        .note
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    let mut results = Vec::new();

    for id in &targets {
        let mut req = client.post(format!(
            "{}/research_watcher/{}/{}",
            base,
            id,
            decision.endpoint()
        ));
        let mut body = JsonMap::new();
        if let Some(note) = note {
            body.insert("note".to_string(), Value::String(note.to_string()));
        }
        req = req.json(&Value::Object(body));
        let resp = with_admin_headers(req, token.as_deref())
            .send()
            .with_context(|| format!("sending {} request for {}", decision.label(), id))?;
        let status = resp.status();
        let text = resp.text().context("reading response body")?;
        if !status.is_success() {
            bail!(
                "{} {} failed: status {} body {}",
                decision.past_tense(),
                id,
                status,
                text.trim()
            );
        }
        let value: Value =
            serde_json::from_str(&text).context("parsing research watcher decision response")?;
        results.push((id.clone(), value));
    }

    if args.json {
        let payload = json!({
            "action": decision.label(),
            "count": results.len(),
            "items": results.iter().map(|(_, v)| v.get("item").cloned().unwrap_or_else(|| json!({}))).collect::<Vec<_>>()
        });
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
            );
        } else {
            println!("{}", payload.to_string());
        }
    } else {
        for (id, value) in &results {
            let item = value
                .get("item")
                .cloned()
                .or_else(|| fetched.get(id).cloned());
            let status = item
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let title = item
                .as_ref()
                .and_then(|v| v.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            println!("{} {} ({}) — {}", decision.past_tense(), id, status, title);
        }
        println!(
            "{} {} item{}",
            decision.past_tense(),
            results.len(),
            if results.len() == 1 { "" } else { "s" }
        );
    }
    Ok(())
}

fn build_client(timeout_secs: u64) -> Result<Client> {
    let secs = timeout_secs.max(1);
    Client::builder()
        .timeout(Duration::from_secs(secs))
        .build()
        .context("building HTTP client")
}

fn fetch_snapshot(
    client: &Client,
    base: &str,
    token: Option<&str>,
    status: Option<&str>,
    limit: Option<i64>,
) -> Result<Value> {
    let mut req = client.get(format!("{}/state/research_watcher", base));
    if let Some(status) = status {
        req = req.query(&[("status", status)]);
    }
    if let Some(limit) = limit {
        req = req.query(&[("limit", &limit)]);
    }
    let resp = with_admin_headers(req, token)
        .send()
        .with_context(|| format!("requesting watcher snapshot from {base}"))?;
    let status_code = resp.status();
    let text = resp
        .text()
        .with_context(|| format!("reading watcher snapshot from {base}"))?;
    if !status_code.is_success() {
        bail!(
            "research watcher snapshot failed: status {} body {}",
            status_code,
            text.trim()
        );
    }
    serde_json::from_str(&text)
        .with_context(|| "parsing research watcher snapshot JSON".to_string())
}

fn extract_items(snapshot: &Value) -> Vec<Value> {
    snapshot
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().cloned().collect())
        .unwrap_or_default()
}

fn filter_items(
    items: &[Value],
    source_filter: Option<&str>,
    contains_filter: Option<&str>,
) -> Vec<Value> {
    let source_filter = source_filter
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());
    let contains_filter = contains_filter
        .as_ref()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty());

    items
        .iter()
        .filter(|item| {
            let mut matches = true;
            if let Some(ref src_filter) = source_filter {
                let source = item
                    .get("source")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_lowercase());
                matches &= source.as_deref() == Some(src_filter.as_str());
            }
            if let Some(ref needle) = contains_filter {
                matches &= contains_text(item, needle);
            }
            matches
        })
        .cloned()
        .collect()
}

fn contains_text(item: &Value, needle: &str) -> bool {
    let mut haystack = String::new();
    if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
        haystack.push_str(title);
        haystack.push('\n');
    }
    if let Some(summary) = item.get("summary").and_then(|v| v.as_str()) {
        haystack.push_str(summary);
        haystack.push('\n');
    }
    if let Some(note) = item.get("note").and_then(|v| v.as_str()) {
        haystack.push_str(note);
        haystack.push('\n');
    }
    if let Some(payload) = item.get("payload") {
        haystack.push_str(&payload.to_string());
    }
    haystack.to_ascii_lowercase().contains(needle)
}

fn print_table(items: &[Value]) {
    println!(
        "{:<12} {:<9} {:<12} {:<20} {}",
        "ID", "Status", "Source", "Updated", "Title"
    );
    for item in items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .map(short_id)
            .unwrap_or_else(|| "-".into());
        let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("-");
        let source = item.get("source").and_then(|v| v.as_str()).unwrap_or("-");
        let updated = item
            .get("updated")
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 19))
            .unwrap_or_else(|| "-".into());
        let mut title = item
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 60))
            .unwrap_or_else(|| "-".into());
        let tags = extract_tags(item);
        if !tags.is_empty() {
            let joined = tags.join(", ");
            title.push_str(" [");
            title.push_str(&truncate(&joined, 24));
            title.push(']');
        }
        println!(
            "{:<12} {:<9} {:<12} {:<20} {}",
            id, status, source, updated, title
        );
    }
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        let mut out = String::new();
        for (idx, ch) in id.chars().enumerate() {
            if idx >= 11 {
                out.push('…');
                break;
            }
            out.push(ch);
        }
        out
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (idx, ch) in s.chars().enumerate() {
        if idx + 1 >= max {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn extract_tags(item: &Value) -> Vec<String> {
    item.get("payload")
        .and_then(|v| v.get("tags"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filter_items_by_source_and_contains() {
        let items = vec![
            json!({"id":"1","source":"arxiv","title":"Vector search baseline","summary":"Baseline","payload":{"tags":["retrieval","baseline"]}}),
            json!({"id":"2","source":"openreview","title":"Retriever upgrade","summary":"Tagged","payload":{"tags":["retrieval","upgrade"]}}),
        ];
        let filtered = filter_items(&items, Some("arxiv"), None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].get("id").and_then(Value::as_str), Some("1"));

        let filtered_contains = filter_items(&items, None, Some("upgrade"));
        assert_eq!(filtered_contains.len(), 1);
        assert_eq!(
            filtered_contains[0].get("id").and_then(Value::as_str),
            Some("2")
        );
    }

    #[test]
    fn truncate_handles_long_strings() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactlyten", 10), "exactlyten");
        assert_eq!(truncate("abcdefghijklmnop", 10), "abcdefghi…");
    }
}
