use anyhow::{bail, Context, Result};
use chrono::Local;
use clap::Parser;
use json_patch::Patch;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde_json::Value as JsonValue;
use std::io::{BufRead, BufReader};
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "arw-mini-dashboard",
    version,
    about = "Tiny read-model watcher for offline/terminal use"
)]
struct Args {
    #[arg(long, env = "BASE", default_value = "http://127.0.0.1:8091")]
    base: String,
    #[arg(long, env = "ARW_ADMIN_TOKEN")]
    admin_token: Option<String>,
    #[arg(long, default_value = "economy_ledger")]
    id: String,
    #[arg(long, default_value_t = 25)]
    limit: usize,
    /// Optional explicit snapshot route (e.g., /state/actions?state=completed)
    #[arg(long)]
    snapshot: Option<String>,
    /// Print full snapshot JSON on every update instead of a one-line summary
    #[arg(long, default_value_t = false)]
    json: bool,
    /// Emit only the initial snapshot/summary and exit
    #[arg(long, default_value_t = false)]
    once: bool,
    /// Periodically print SSE counters from /metrics (connections/sent/errors)
    #[arg(long, default_value_t = false)]
    sse: bool,
    /// Optional substring filter for route_stats rendering (e.g., /state/)
    #[arg(long)]
    filter: Option<String>,
    /// Optional Last-Event-ID to resume from
    #[arg(long)]
    last_event_id: Option<String>,
}

fn with_admin_headers(
    mut req: reqwest::blocking::RequestBuilder,
    token: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    if let Some(t) = token {
        if !t.trim().is_empty() {
            req = req.header(AUTHORIZATION, format!("Bearer {}", t.trim()));
        }
    }
    req
}

fn fetch_economy_snapshot(
    client: &Client,
    base: &str,
    token: Option<&str>,
    limit: usize,
) -> Result<JsonValue> {
    let url = format!("{}/state/economy/ledger", base.trim_end_matches('/'));
    let req = with_admin_headers(client.get(url).query(&[("limit", limit)]), token)
        .header(ACCEPT, "application/json");
    let resp = req.send().context("fetching economy snapshot")?;
    if !resp.status().is_success() {
        bail!("economy snapshot failed: {}", resp.status());
    }
    resp.json().context("decoding economy snapshot json")
}

fn fetch_snapshot_via_route(
    client: &Client,
    base: &str,
    token: Option<&str>,
    route: &str,
) -> Result<JsonValue> {
    let url = if route.starts_with('/') {
        format!("{}{}", base.trim_end_matches('/'), route)
    } else {
        format!("{}/{}", base.trim_end_matches('/'), route)
    };
    let req = with_admin_headers(client.get(url), token).header(ACCEPT, "application/json");
    let resp = req.send().context("fetching snapshot via route")?;
    if !resp.status().is_success() {
        bail!("snapshot via route failed: {}", resp.status());
    }
    resp.json().context("decoding snapshot json")
}

fn stream_patches_once(
    client: &Client,
    base: &str,
    token: Option<&str>,
    last_id: Option<&str>,
    id: &str,
    snapshot: &mut JsonValue,
    json_out: bool,
) -> Result<Option<String>> {
    let mut req = client
        .get(format!("{}/events", base.trim_end_matches('/')))
        .query(&[("prefix", "state.read.model.patch"), ("replay", "0")])
        .header(ACCEPT, "text/event-stream");
    if let Some(since) = last_id {
        req = req.header("Last-Event-ID", since);
    }
    req = with_admin_headers(req, token);
    let resp = req.send().context("connecting to events stream")?;
    if !resp.status().is_success() {
        bail!("events stream failed: {}", resp.status());
    }
    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut event_name = String::new();
    let mut data_buf = String::new();
    let mut event_id_line: Option<String> = None;
    let mut latest = last_id.map(|s| s.to_string());
    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(latest);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if line.is_empty() {
            if event_name == "state.read.model.patch" && !data_buf.is_empty() {
                if let Err(err) = handle_patch(&data_buf, id, snapshot, json_out, None) {
                    eprintln!("[mini] failed to process patch: {err:?}");
                } else if let Some(ev) = event_id_line.as_ref() {
                    latest = Some(ev.clone());
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

fn handle_patch(
    data: &str,
    id: &str,
    snapshot: &mut JsonValue,
    json_out: bool,
    filter: Option<&str>,
) -> Result<()> {
    let env: JsonValue = serde_json::from_str(data).context("decode SSE payload")?;
    // Handle both {kind,payload} and raw payload shapes.
    let payload = env.get("payload").cloned().unwrap_or(env.clone());
    let rm = payload.get("payload").cloned().unwrap_or(payload.clone());
    let read_model_id = rm
        .get("id")
        .or_else(|| rm.get("read_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if read_model_id != id {
        return Ok(());
    }
    let patch_value = match rm.get("patch") {
        Some(v) if v.is_array() => v.clone(),
        _ => return Ok(()),
    };
    let patch: Patch = serde_json::from_value(patch_value).context("decode JSON Patch")?;
    // Ensure target container exists for common economy_ledger patches (e.g., /entries/0 on empty snapshot)
    if id == "economy_ledger" {
        if !snapshot.is_object() {
            *snapshot = serde_json::json!({});
        }
        let has_entries_array = snapshot
            .get("entries")
            .map(|v| v.is_array())
            .unwrap_or(false);
        if !has_entries_array {
            if let Some(obj) = snapshot.as_object_mut() {
                obj.insert("entries".to_string(), serde_json::json!([]));
            }
        }
    }
    json_patch::patch(snapshot, &patch).context("apply patch")?;
    if json_out {
        println!(
            "{}",
            serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        render(id, snapshot, filter);
    }
    Ok(())
}

#[allow(dead_code)]
fn apply_patch_to_snapshot(data: &str, id: &str, snapshot: &mut JsonValue) -> Result<()> {
    handle_patch(data, id, snapshot, false, None)
}

fn render(id: &str, snapshot: &JsonValue, filter: Option<&str>) {
    let now = Local::now().format("%H:%M:%S");
    if id == "economy_ledger" {
        let version = snapshot
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let entries = snapshot
            .get("entries")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let totals = snapshot
            .get("totals")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "[{}] economy v{} entries={} totals={}",
            now, version, entries, totals
        );
    } else if id == "route_stats" {
        println!("[{}] {}", now, route_stats_summary(snapshot, filter));
    } else {
        let version = snapshot
            .get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if let Some(n) = snapshot
            .get("items")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
        {
            println!("[{}] {} v{} items={} ", now, id, version, n);
        } else {
            println!("[{}] {} v{} updated ", now, id, version);
        }
    }
}

fn route_stats_summary(snapshot: &JsonValue, filter: Option<&str>) -> String {
    let by_path = snapshot.get("by_path").and_then(|v| v.as_object());
    let mut p95_list: Vec<(&str, f64, f64)> = Vec::new();
    let mut hits_list: Vec<(&str, u64)> = Vec::new();
    let mut errs_list: Vec<(&str, u64)> = Vec::new();
    if let Some(map) = by_path {
        for (k, v) in map.iter() {
            if let Some(substr) = filter {
                if !k.contains(substr) {
                    continue;
                }
            }
            let p95 = v.get("p95_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let ewma = v.get("ewma_ms").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let hits = v.get("hits").and_then(|x| x.as_u64()).unwrap_or(0);
            let errs = v.get("errors").and_then(|x| x.as_u64()).unwrap_or(0);
            p95_list.push((k.as_str(), p95, ewma));
            hits_list.push((k.as_str(), hits));
            errs_list.push((k.as_str(), errs));
        }
    }
    p95_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    hits_list.sort_by(|a, b| b.1.cmp(&a.1));
    errs_list.sort_by(|a, b| b.1.cmp(&a.1));
    let total = p95_list.len();
    let top_p95 = p95_list
        .iter()
        .take(3)
        .map(|(k, p, e)| format!("{}:{:.0}/{:.0}", k, p, e))
        .collect::<Vec<_>>()
        .join(", ");
    let top_hits = hits_list
        .iter()
        .take(3)
        .map(|(k, h)| format!("{}:{}", k, h))
        .collect::<Vec<_>>()
        .join(", ");
    let top_errs = errs_list
        .iter()
        .take(3)
        .map(|(k, e)| format!("{}:{}", k, e))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "route_stats routes={} top(p95/ewma)=[{}] top(hits)=[{}] top(errors)=[{}]",
        total, top_p95, top_hits, top_errs
    )
}

fn fetch_sse_counters(client: &Client, base: &str, token: Option<&str>) -> Option<(u64, u64, u64)> {
    let url = format!("{}/metrics", base.trim_end_matches('/'));
    let req = with_admin_headers(client.get(url), token).header(ACCEPT, "text/plain");
    let resp = req.send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().ok()?;
    let mut conn: u64 = 0;
    let mut sent: u64 = 0;
    let mut errs: u64 = 0;
    for line in text.lines() {
        if line.starts_with("arw_events_sse_connections_total")
            || line.starts_with("arw_events_sse_sent_total")
            || line.starts_with("arw_events_sse_errors_total")
        {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(last) = parts.last() {
                if let Ok(v) = last.parse::<f64>() {
                    let vi = v as u64;
                    if line.starts_with("arw_events_sse_connections_total") {
                        conn = conn.saturating_add(vi);
                    } else if line.starts_with("arw_events_sse_sent_total") {
                        sent = sent.saturating_add(vi);
                    } else if line.starts_with("arw_events_sse_errors_total") {
                        errs = errs.saturating_add(vi);
                    }
                }
            }
        }
    }
    Some((conn, sent, errs))
}

fn spawn_sse_poll(client: Client, base: String, token: Option<String>) {
    thread::spawn(move || loop {
        if let Some((c, s, e)) = fetch_sse_counters(&client, &base, token.as_deref()) {
            let now = Local::now().format("%H:%M:%S");
            println!("[{}] sse conn={} sent={} err={}", now, c, s, e);
        }
        thread::sleep(Duration::from_secs(10));
    });
}

fn main() -> Result<()> {
    let args = Args::parse();
    let client = Client::builder()
        .timeout(None)
        .build()
        .context("client build ")?;
    let token = args.admin_token.as_deref();
    let base = args.base.trim_end_matches('/').to_string();
    if args.sse {
        spawn_sse_poll(client.clone(), base.clone(), args.admin_token.clone());
    }
    let mut snapshot: JsonValue = if let Some(route) = args.snapshot.as_deref() {
        fetch_snapshot_via_route(&client, &base, token, route)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else if args.id == "economy_ledger" {
        fetch_economy_snapshot(&client, &base, token, args.limit)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        let route = format!("/state/{}", args.id);
        fetch_snapshot_via_route(&client, &base, token, &route)
            .unwrap_or_else(|_| serde_json::json!({}))
    };
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        render(&args.id, &snapshot, args.filter.as_deref());
    }
    let mut last_event_id: Option<String> = args.last_event_id.clone();
    let mut backoff = 1u64;
    loop {
        match stream_patches_once(
            &client,
            &base,
            token,
            last_event_id.as_deref(),
            &args.id,
            &mut snapshot,
            args.json,
        ) {
            Ok(next) => {
                if let Some(id) = next {
                    last_event_id = Some(id);
                }
                backoff = 1;
            }
            Err(err) => {
                eprintln!("[mini] stream error: {err:?} ");
                backoff = (backoff * 2).min(30);
            }
        }
        thread::sleep(Duration::from_secs(backoff));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_patch_updates_economy_snapshot() {
        let mut snap = serde_json::json!({});
        let patch_env = serde_json::json!({
            "kind": "state.read.model.patch",
            "payload": {
                "payload": {
                    "id": "economy_ledger",
                    "patch": [
                        {"op":"add","path":"/version","value":2},
                        {"op":"add","path":"/entries/0","value":{"id":"a"}}
                    ]
                }
            }
        });
        let data = serde_json::to_string(&patch_env).unwrap();
        apply_patch_to_snapshot(&data, "economy_ledger", &mut snap).expect("apply");
        assert_eq!(snap.get("version").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(
            snap.get("entries")
                .and_then(|v| v.as_array())
                .map(|a| a.len()),
            Some(1)
        );
    }

    #[test]
    fn apply_patch_ignores_other_ids() {
        let mut snap = serde_json::json!({});
        let patch_env = serde_json::json!({
            "payload": {"payload": {"id": "economy_ledger", "patch": []}}
        });
        let data = serde_json::to_string(&patch_env).unwrap();
        apply_patch_to_snapshot(&data, "projects", &mut snap).expect("apply");
        assert!(snap.as_object().unwrap().is_empty());
    }

    #[test]
    fn render_route_stats_safe() {
        let snap = serde_json::json!({
            "by_path": {
                "/state/economy/ledger": {"hits": 10, "errors": 0, "ewma_ms": 12.3, "p95_ms": 45.6},
                "/state/actions": {"hits": 7, "errors": 1, "ewma_ms": 30.0, "p95_ms": 90.0}
            }
        });
        // Should not panic
        render("route_stats", &snap, None);
    }
}
