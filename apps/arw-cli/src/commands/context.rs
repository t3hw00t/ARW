use std::cmp::Ordering;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{Local, Utc};
use clap::{Args, Subcommand};
use reqwest::{blocking::Client, StatusCode};
use serde_json::Value as JsonValue;

use super::util::{
    append_json_output, append_text_output, format_local_timestamp, format_relative_from_now,
    parse_byte_limit_arg, resolve_admin_token, with_admin_headers,
};

#[derive(Subcommand)]
pub enum ContextCmd {
    /// Fetch /state/training/telemetry and render a summary
    Telemetry(ContextTelemetryArgs),
}

#[derive(Args)]
pub struct ContextTelemetryArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,
    /// Emit raw JSON snapshot
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Poll continuously and print summaries on interval
    #[arg(long, conflicts_with = "json")]
    pub watch: bool,
    /// Seconds between polls when --watch is enabled
    #[arg(long, default_value_t = 15, requires = "watch")]
    pub interval: u64,
    /// Append output to this file (creates directories as needed)
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,
    /// Rotate output file when it reaches this many bytes (requires --output)
    #[arg(
        long,
        value_name = "BYTES",
        requires = "output",
        value_parser = parse_byte_limit_arg,
        help = "Rotate after BYTES (supports K/M/G/T suffixes; min 64KB unless 0)"
    )]
    pub output_rotate: Option<u64>,
}

pub fn execute(cmd: ContextCmd) -> Result<()> {
    match cmd {
        ContextCmd::Telemetry(args) => cmd_context_telemetry(&args),
    }
}

fn cmd_context_telemetry(args: &ContextTelemetryArgs) -> Result<()> {
    if args.watch {
        eprintln!("watching context telemetry; press Ctrl-C to exit");
        return watch_context_telemetry(args);
    }
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let (status, body) = request_context_telemetry(&client, base, token.as_deref())?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access telemetry"
        );
    }
    if !status.is_success() {
        anyhow::bail!("training telemetry request failed: {} {}", status, body);
    }

    if args.json {
        if let Some(path) = args.output.as_ref() {
            append_json_output(path.as_path(), &body, args.pretty, args.output_rotate)?;
        }
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

    let now_ms = Utc::now().timestamp_millis();
    let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
    let summary = render_context_telemetry_summary(&body, now_ms);
    println!("{}", summary.trim_end());
    if let Some(path) = args.output.as_ref() {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Some(limit) = args.output_rotate {
            append_text_output(path.as_path(), Some(stamp.as_str()), &summary, Some(limit))?;
        } else {
            append_context_summary(path, Some(stamp.as_str()), &summary)?;
        }
    }
    Ok(())
}

fn request_context_telemetry(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<(StatusCode, JsonValue)> {
    let url = format!("{}/state/training/telemetry", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("requesting training telemetry snapshot from {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing training telemetry response")?;
    Ok((status, body))
}

fn watch_context_telemetry(args: &ContextTelemetryArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let base_interval = args.interval.max(1);
    let max_backoff = base_interval.max(60);
    let mut sleep_secs = base_interval;

    loop {
        match request_context_telemetry(&client, base, token.as_deref()) {
            Ok((status, body)) => {
                if status == StatusCode::UNAUTHORIZED {
                    anyhow::bail!(
                        "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access telemetry"
                    );
                }
                if !status.is_success() {
                    eprintln!("[context watch] request failed: {} {}", status, body);
                    sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
                } else {
                    let now_ms = Utc::now().timestamp_millis();
                    let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
                    let summary = render_context_telemetry_summary(&body, now_ms);
                    let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    println!("=== {} ===", stamp);
                    println!("{}", summary.trim_end());
                    println!();
                    io::stdout().flush().ok();
                    if let Some(path) = args.output.as_ref() {
                        if let Some(limit) = args.output_rotate {
                            append_text_output(
                                path.as_path(),
                                Some(stamp.as_str()),
                                &summary,
                                Some(limit),
                            )?;
                        } else {
                            append_context_summary(path, Some(stamp.as_str()), &summary)?;
                        }
                    }
                    sleep_secs = base_interval;
                }
            }
            Err(err) => {
                eprintln!("[context watch] error: {err:?}");
                sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
            }
        }

        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

fn append_context_summary(path: &Path, stamp: Option<&str>, summary: &str) -> Result<()> {
    append_text_output(path, stamp, summary, None)
}

#[cfg_attr(not(test), allow(dead_code))]
fn append_context_json(path: &Path, body: &JsonValue, pretty: bool) -> Result<()> {
    append_json_output(path, body, pretty, None)
}

fn render_context_telemetry_summary(snapshot: &JsonValue, now_ms: u64) -> String {
    let mut out = String::new();
    if let Some(ms) = snapshot.get("generated_ms").and_then(JsonValue::as_u64) {
        let _ = writeln!(
            out,
            "Generated: {} ({})",
            format_local_timestamp(ms),
            format_relative_from_now(ms, now_ms)
        );
    } else if let Some(ts) = snapshot.get("generated").and_then(JsonValue::as_str) {
        let _ = writeln!(out, "Generated: {}", clean_text(ts));
    } else {
        let _ = writeln!(out, "Generated: unknown");
    }

    let Some(context) = snapshot.get("context").and_then(JsonValue::as_object) else {
        out.push('\n');
        let _ = writeln!(out, "Coverage:");
        let _ = writeln!(out, "  (no context telemetry)");
        return out;
    };

    out.push('\n');
    summarize_coverage_section(&mut out, context.get("coverage"));
    out.push('\n');
    summarize_recall_section(&mut out, context.get("recall_risk"));
    out.push('\n');
    summarize_working_set_section(&mut out, context.get("assembled"));

    out
}

fn summarize_coverage_section(out: &mut String, coverage: Option<&JsonValue>) {
    let _ = writeln!(out, "Coverage:");
    let Some(obj) = coverage.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no coverage data)");
        return;
    };

    if let Some(latest) = obj.get("latest").and_then(JsonValue::as_object) {
        let needs_more = latest
            .get("needs_more")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let verdict = if needs_more {
            "needs more coverage"
        } else {
            "coverage satisfied"
        };
        let _ = writeln!(out, "  Latest verdict: {}", verdict);

        if let Some(scope) = render_scope(latest.get("project"), latest.get("query")) {
            let _ = writeln!(out, "  Scope: {}", scope);
        }

        if let Some(bias) = latest
            .get("persona_bias")
            .and_then(format_persona_bias_summary)
        {
            let _ = writeln!(out, "  Persona bias: {}", bias);
        }

        if let Some(reasons) = latest.get("reasons").and_then(JsonValue::as_array) {
            let mut labels: Vec<String> = reasons
                .iter()
                .filter_map(JsonValue::as_str)
                .map(format_coverage_reason)
                .collect();
            if !labels.is_empty() {
                labels.sort();
                labels.dedup();
                let _ = writeln!(out, "  Reasons: {}", labels.join(", "));
            }
        }

        if let Some(summary) = latest.get("summary").and_then(JsonValue::as_object) {
            if let Some(slots) = summary.get("slots").and_then(JsonValue::as_object) {
                if let Some(counts) = slots.get("counts").and_then(JsonValue::as_object) {
                    let mut entries: Vec<String> = counts
                        .iter()
                        .filter_map(|(slot, value)| value.as_u64().map(|count| (slot, count)))
                        .map(|(slot, count)| format!("{}={}", format_slot_name(slot), count))
                        .collect();
                    if !entries.is_empty() {
                        entries.sort();
                        let _ = writeln!(out, "  Slot counts: {}", entries.join(", "));
                    }
                }
                if let Some(budgets) = slots.get("budgets").and_then(JsonValue::as_object) {
                    let mut entries: Vec<String> = budgets
                        .iter()
                        .filter_map(|(slot, value)| value.as_u64().map(|count| (slot, count)))
                        .map(|(slot, count)| format!("{}≤{}", format_slot_name(slot), count))
                        .collect();
                    if !entries.is_empty() {
                        entries.sort();
                        let _ = writeln!(out, "  Budgets: {}", entries.join(", "));
                    }
                }
            }
        }
    } else {
        let _ = writeln!(out, "  Latest verdict unavailable");
    }

    if let Some(ratio) = obj.get("needs_more_ratio").and_then(JsonValue::as_f64) {
        let sample = obj
            .get("recent")
            .and_then(JsonValue::as_array)
            .map(|arr| arr.len())
            .unwrap_or(0);
        let window = if sample > 0 {
            format!("last {}", sample)
        } else {
            "recent".to_string()
        };
        let _ = writeln!(
            out,
            "  Needs-more ratio ({}): {}",
            window,
            format_percent(ratio, 0)
        );
    }

    if let Some(reasons) = obj.get("top_reasons").and_then(JsonValue::as_array) {
        let lines: Vec<String> = reasons
            .iter()
            .filter_map(|item| {
                let reason = item.get("reason").and_then(JsonValue::as_str)?;
                let count = item.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                Some(format!(
                    "{} · {}",
                    format_coverage_reason(reason),
                    format_count_label(count, "event")
                ))
            })
            .collect();
        if !lines.is_empty() {
            let _ = writeln!(out, "  Top gaps:");
            for line in lines.iter().take(3) {
                let _ = writeln!(out, "    - {}", line);
            }
        }
    }

    if let Some(slots) = obj.get("top_slots").and_then(JsonValue::as_array) {
        if !slots.is_empty() {
            let _ = writeln!(out, "  Top slot gaps:");
            for slot in slots.iter().take(3) {
                let name = slot
                    .get("slot")
                    .and_then(JsonValue::as_str)
                    .map(format_slot_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let count = slot.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                let _ = writeln!(out, "    - {} · {}", name, format_count_label(count, "gap"));
            }
        }
    }
}

fn summarize_recall_section(out: &mut String, recall: Option<&JsonValue>) {
    let _ = writeln!(out, "Recall risk:");
    let Some(obj) = recall.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no recall telemetry)");
        return;
    };

    if let Some(latest) = obj.get("latest").and_then(JsonValue::as_object) {
        let level = latest
            .get("level")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        let score = latest.get("score").and_then(JsonValue::as_f64);
        let at_risk = latest
            .get("at_risk")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let status = if at_risk { "at risk" } else { "stable" };
        let _ = writeln!(
            out,
            "  Latest level: {} ({}){}",
            level,
            percent_or_dash(score, 0),
            if at_risk { " · investigate" } else { "" }
        );
        let _ = writeln!(out, "  Status: {}", status);

        if let Some(bias) = latest
            .get("persona_bias")
            .and_then(format_persona_bias_summary)
        {
            let _ = writeln!(out, "  Persona bias: {}", bias);
        }

        if let Some(components) = latest.get("components").and_then(JsonValue::as_object) {
            if let Some(value) = components
                .get("coverage_shortfall")
                .and_then(JsonValue::as_f64)
            {
                let _ = writeln!(out, "  Coverage shortfall: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("lane_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Lane gap: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("slot_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Slot gap: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("quality_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Quality gap: {}", format_percent(value, 0));
            }
            if let Some(slots) = components.get("slots").and_then(JsonValue::as_object) {
                let mut entries: Vec<(String, f64)> = slots
                    .iter()
                    .filter_map(|(slot, value)| value.as_f64().map(|gap| (slot.clone(), gap)))
                    .collect();
                entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                entries.retain(|(_, gap)| *gap > 0.0);
                if !entries.is_empty() {
                    let _ = writeln!(out, "  Slot gaps:");
                    for (slot, gap) in entries.into_iter().take(3) {
                        let _ = writeln!(
                            out,
                            "    - {} · {}",
                            format_slot_name(&slot),
                            format_percent(gap, 0)
                        );
                    }
                }
            }
        }
    } else {
        let _ = writeln!(out, "  Latest level unavailable");
    }

    if let Some(avg) = obj.get("avg_score").and_then(JsonValue::as_f64) {
        let samples = obj.get("sampled").and_then(JsonValue::as_u64).unwrap_or(0);
        let label = if samples > 0 {
            format!("avg score ({} samples)", samples)
        } else {
            "avg score".to_string()
        };
        let _ = writeln!(out, "  {}: {}", label, format_percent(avg, 0));
    }
    if let Some(ratio) = obj.get("at_risk_ratio").and_then(JsonValue::as_f64) {
        let _ = writeln!(out, "  At-risk ratio: {}", format_percent(ratio, 0));
    }
    if let Some(levels) = obj.get("levels").and_then(JsonValue::as_array) {
        if !levels.is_empty() {
            let entries: Vec<String> = levels
                .iter()
                .filter_map(|level| {
                    let name = level.get("level").and_then(JsonValue::as_str)?;
                    let count = level.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                    Some(format!("{} {}", name, format_count_label(count, "sample")))
                })
                .collect();
            if !entries.is_empty() {
                let _ = writeln!(out, "  Level distribution: {}", entries.join(", "));
            }
        }
    }
    if let Some(slots) = obj.get("top_slots").and_then(JsonValue::as_array) {
        if !slots.is_empty() {
            let _ = writeln!(out, "  Top slot gaps (avg / max):");
            for entry in slots.iter().take(3) {
                let slot = entry
                    .get("slot")
                    .and_then(JsonValue::as_str)
                    .map(format_slot_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let avg = percent_or_dash(entry.get("avg_gap").and_then(JsonValue::as_f64), 0);
                let max = percent_or_dash(entry.get("max_gap").and_then(JsonValue::as_f64), 0);
                let samples = entry
                    .get("samples")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or(0);
                let _ = writeln!(
                    out,
                    "    - {} · avg {} · max {} · {}",
                    slot,
                    avg,
                    max,
                    format_count_label(samples, "sample")
                );
            }
        }
    }
}

fn summarize_working_set_section(out: &mut String, assembled: Option<&JsonValue>) {
    let _ = writeln!(out, "Working set:");
    let Some(obj) = assembled.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no assembled snapshot)");
        return;
    };

    if let Some(scope) = render_scope(obj.get("project"), obj.get("query")) {
        let _ = writeln!(out, "  Scope: {}", scope);
    }

    if let Some(working) = obj.get("working_set").and_then(JsonValue::as_object) {
        if let Some(counts) = working.get("counts").and_then(JsonValue::as_object) {
            let items = counts.get("items").and_then(JsonValue::as_u64).unwrap_or(0);
            let seeds = counts.get("seeds").and_then(JsonValue::as_u64).unwrap_or(0);
            let expanded = counts
                .get("expanded")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            let _ = writeln!(
                out,
                "  Counts: items {} · seeds {} · expanded {}",
                items, seeds, expanded
            );
        }
        if let Some(spec) = working
            .get("final_spec")
            .or_else(|| obj.get("spec"))
            .and_then(JsonValue::as_object)
        {
            if let Some(lanes) = spec.get("lanes").and_then(JsonValue::as_array) {
                let labels: Vec<String> = lanes
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(clean_text)
                    .collect();
                if !labels.is_empty() {
                    let _ = writeln!(out, "  Lanes: {}", labels.join(", "));
                }
            }
            if let Some(slots) = spec.get("slot_budgets").and_then(JsonValue::as_object) {
                let mut entries: Vec<String> = slots
                    .iter()
                    .filter_map(|(slot, value)| value.as_u64().map(|budget| (slot, budget)))
                    .map(|(slot, budget)| format!("{}≤{}", format_slot_name(slot), budget))
                    .collect();
                if !entries.is_empty() {
                    entries.sort();
                    let _ = writeln!(out, "  Slot budgets: {}", entries.join(", "));
                }
            }
        }
        if let Some(bias) = working
            .get("persona_bias")
            .or_else(|| obj.get("persona_bias"))
            .and_then(format_persona_bias_summary)
        {
            let _ = writeln!(out, "  Persona bias: {}", bias);
        }
    } else {
        let _ = writeln!(out, "  (working set summary unavailable)");
    }
}

fn format_persona_bias_summary(bias: &JsonValue) -> Option<String> {
    let obj = bias.as_object()?;
    let mut sections: Vec<String> = Vec::new();
    if let Some(lanes) = obj.get("lane_priorities").and_then(JsonValue::as_object) {
        let mut entries: Vec<(String, f64)> = lanes
            .iter()
            .filter_map(|(lane, value)| value.as_f64().map(|weight| (lane.clone(), weight)))
            .collect();
        entries.retain(|(_, weight)| weight.is_finite() && weight.abs() >= f64::EPSILON);
        if !entries.is_empty() {
            entries.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
            let formatted = entries
                .into_iter()
                .map(|(lane, weight)| format!("{}:{:+.2}", format_lane_label(&lane), weight))
                .collect::<Vec<_>>()
                .join(" ");
            if !formatted.is_empty() {
                sections.push(format!("lanes {}", formatted));
            }
        }
    }
    if let Some(slots) = obj.get("slot_overrides").and_then(JsonValue::as_object) {
        let mut entries: Vec<(String, u64)> = slots
            .iter()
            .filter_map(|(slot, value)| value.as_u64().map(|limit| (slot.clone(), limit)))
            .collect();
        if !entries.is_empty() {
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let formatted = entries
                .into_iter()
                .map(|(slot, limit)| format!("{}≤{}", format_slot_name(&slot), limit))
                .collect::<Vec<_>>()
                .join(" ");
            if !formatted.is_empty() {
                sections.push(format!("slots {}", formatted));
            }
        }
    }
    if let Some(delta) = obj.get("min_score_delta").and_then(JsonValue::as_f64) {
        if delta.is_finite() && delta.abs() >= f64::EPSILON {
            sections.push(format!("min_score {:+.2}", delta));
        }
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join(" · "))
    }
}

fn format_lane_label(lane: &str) -> String {
    if lane.trim() == "*" {
        "any".to_string()
    } else {
        clean_text(&lane.replace(['_', '-'], " "))
    }
}

fn render_scope(project: Option<&JsonValue>, query: Option<&JsonValue>) -> Option<String> {
    let project = project.and_then(JsonValue::as_str).map(clean_text);
    let query = query.and_then(JsonValue::as_str).map(clean_text);
    match (project, query) {
        (Some(p), Some(q)) if !p.is_empty() && !q.is_empty() => {
            Some(format!("project {} · query {}", p, q))
        }
        (Some(p), _) if !p.is_empty() => Some(format!("project {}", p)),
        (_, Some(q)) if !q.is_empty() => Some(format!("query {}", q)),
        _ => None,
    }
}

fn format_coverage_reason(reason: &str) -> String {
    if let Some(slot) = reason.strip_prefix("slot_underfilled:") {
        format!("Slot underfilled · {}", format_slot_name(slot))
    } else {
        clean_text(&reason.replace('_', " "))
    }
}

fn format_slot_name(slot: &str) -> String {
    clean_text(&slot.replace(['_', '-'], " "))
}

fn format_percent(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let clamped = value.clamp(0.0, 1.0);
    format!("{:.*}%", digits, clamped * 100.0)
}

fn percent_or_dash(value: Option<f64>, digits: usize) -> String {
    match value {
        Some(v) if v.is_finite() => format_percent(v, digits),
        _ => "—".to_string(),
    }
}

fn format_count_label(count: u64, singular: &str) -> String {
    if count == 1 {
        format!("1 {}", singular)
    } else {
        format!("{} {}s", count, singular)
    }
}

fn clean_text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn context_summary_handles_full_snapshot() {
        let snapshot = json!({
            "generated_ms": 1_700_000_000_000u64,
            "context": {
                "coverage": {
                    "latest": {
                        "needs_more": true,
                        "project": "alpha",
                        "query": "sprint review",
                        "reasons": ["slot_underfilled:seeds", "lane_gap"],
                "summary": {
                    "slots": {
                        "counts": {"seeds": 2, "drafts": 1},
                        "budgets": {"seeds": 4}
                    }
                },
                "persona_bias": {
                    "lane_priorities": {"semantic": 0.2, "episodic": 0.1},
                    "slot_overrides": {"evidence": 3},
                    "min_score_delta": 0.05
                }
            },
            "needs_more_ratio": 0.4,
            "recent": [1, 2, 3],
            "top_reasons": [
                {"reason": "slot_underfilled:seeds", "count": 3},
                        {"reason": "lane_gap", "count": 1}
                    ],
                    "top_slots": [
                        {"slot": "seeds", "count": 3},
                        {"slot": "drafts", "count": 1}
                    ]
                },
                "recall_risk": {
                    "latest": {
                        "level": "medium",
                        "score": 0.42,
                        "at_risk": true,
                        "components": {
                            "coverage_shortfall": 0.6,
                            "lane_gap": 0.0,
                            "slot_gap": 0.25,
                            "quality_gap": 0.05,
                            "slots": {"seeds": 0.7, "drafts": 0.2}
                        }
                    },
                "avg_score": 0.33,
                "at_risk_ratio": 0.4,
                "sampled": 5,
                "levels": [
                    {"level": "high", "count": 2},
                    {"level": "medium", "count": 3}
                ],
                "top_slots": [
                    {"slot": "seeds", "avg_gap": 0.7, "max_gap": 0.9, "samples": 3}
                ]
            },
            "assembled": {
                "project": "alpha",
                "query": "sprint review",
                "persona_bias": {
                    "lane_priorities": {"semantic": 0.2, "episodic": 0.1},
                    "slot_overrides": {"evidence": 3},
                    "min_score_delta": 0.05
                },
                "working_set": {
                    "counts": {"items": 8, "seeds": 3, "expanded": 9},
                    "final_spec": {
                        "lanes": ["research", "analysis"],
                        "slot_budgets": {"seeds": 4, "drafts": 2}
                        }
                    }
                }
            }
        });
        let summary = render_context_telemetry_summary(&snapshot, 1_700_000_005_000);
        assert!(summary.contains("Coverage:"));
        assert!(summary.contains("Latest verdict"));
        assert!(summary.contains("Slot underfilled"));
        assert!(summary.contains("Recall risk:"));
        assert!(summary.contains("avg score"));
        assert!(summary.contains("Working set:"));
        assert!(summary.contains("Counts: items"));
        assert!(summary.contains("Persona bias: lanes semantic:+0.20 episodic:+0.10"));
    }

    #[test]
    fn context_summary_handles_missing_sections() {
        let snapshot = json!({
            "generated": "2025-10-02T17:15:00Z"
        });
        let summary = render_context_telemetry_summary(&snapshot, 1_700_000_000_000);
        assert!(summary.contains("Generated"));
        assert!(summary.contains("no context telemetry"));
    }

    #[test]
    fn append_context_summary_creates_dirs_and_appends() {
        let dir = TempDir::new().expect("tempdir");
        let log_path = dir.path().join("logs/2025-10-02/context.log");

        append_context_summary(&log_path, Some("2025-10-02 12:00:00"), "First run").unwrap();
        append_context_summary(&log_path, None, "Second run").unwrap();

        let contents = fs::read_to_string(&log_path).expect("read log");
        assert_eq!(
            contents,
            "=== 2025-10-02 12:00:00 ===\nFirst run\n\nSecond run\n\n"
        );
    }

    #[test]
    fn append_context_json_respects_pretty_flag() {
        let dir = TempDir::new().expect("tempdir");
        let log_path = dir.path().join("logs/context.jsonl");
        let payload = json!({"hello": "world"});

        append_context_json(&log_path, &payload, false).unwrap();
        append_context_json(&log_path, &payload, true).unwrap();

        let contents = fs::read_to_string(&log_path).expect("read log");
        let mut lines = contents.lines();
        assert_eq!(lines.next().unwrap().trim(), "{\"hello\":\"world\"}");
        let pretty_block = lines.collect::<Vec<_>>().join("\n");
        assert!(pretty_block.contains("\"hello\": \"world\""));
    }
}
