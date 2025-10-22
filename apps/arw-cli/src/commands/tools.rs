use std::time::Duration;

use anyhow::{Context, Result};
use arw_core::introspect_tools;
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use super::util::{
    format_bytes, format_bytes_f64, format_duration_ms, format_seconds, format_seconds_f64,
    resolve_admin_token, with_admin_headers,
};

#[derive(Args, Default, Clone, Copy)]
pub struct ToolsListArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Subcommand)]
pub enum ToolsSubcommand {
    /// Print tool list (JSON)
    List(ToolsListArgs),
    /// Fetch tool cache statistics from the server
    Cache(ToolsCacheArgs),
}

#[derive(Args, Clone)]
pub struct ToolsCacheArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 10)]
    pub timeout: u64,
    /// Emit raw JSON instead of a human summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (only with --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
}

pub fn execute(list: ToolsListArgs, cmd: Option<ToolsSubcommand>) -> Result<()> {
    match cmd {
        Some(ToolsSubcommand::Cache(args)) => cmd_tools_cache(&args),
        Some(ToolsSubcommand::List(args)) => {
            print_tools_list(args.pretty);
            Ok(())
        }
        None => {
            print_tools_list(list.pretty);
            Ok(())
        }
    }
}

#[derive(Debug, Deserialize)]
struct ToolCacheSnapshot {
    hit: u64,
    miss: u64,
    coalesced: u64,
    errors: u64,
    bypass: u64,
    payload_too_large: u64,
    capacity: u64,
    ttl_secs: u64,
    entries: u64,
    max_payload_bytes: Option<u64>,
    latency_saved_ms_total: u64,
    latency_saved_samples: u64,
    avg_latency_saved_ms: f64,
    payload_bytes_saved_total: u64,
    payload_saved_samples: u64,
    avg_payload_bytes_saved: f64,
    avg_hit_age_secs: f64,
    hit_age_samples: u64,
    last_hit_age_secs: Option<u64>,
    max_hit_age_secs: Option<u64>,
    stampede_suppression_rate: f64,
    last_latency_saved_ms: Option<u64>,
    last_payload_bytes: Option<u64>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, JsonValue>,
}

fn print_tools_list(pretty: bool) {
    let list = introspect_tools();
    if pretty {
        println!(
            "{}",
            serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".to_string())
        );
    } else {
        match serde_json::to_string(&list) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("[]"),
        }
    }
}

pub fn cmd_tools_cache(args: &ToolsCacheArgs) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/tools/cache_stats", base);
    let resp = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let text = resp.text().context("reading cache stats response")?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow::anyhow!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN"
        ));
    }
    if !status.is_success() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("cache stats request failed: {}", status));
        }
        return Err(anyhow::anyhow!(
            "cache stats request failed: {} {}",
            status,
            trimmed
        ));
    }

    let raw: JsonValue = serde_json::from_str(&text).context("parsing cache stats JSON")?;
    let snapshot: ToolCacheSnapshot =
        serde_json::from_value(raw.clone()).context("deserializing cache stats snapshot")?;

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

    println!("{}", render_tool_cache_summary(&snapshot, base));
    Ok(())
}

fn render_tool_cache_summary(stats: &ToolCacheSnapshot, base: &str) -> String {
    use std::fmt::Write as _;

    let mut buf = String::new();
    let _ = writeln!(buf, "Tool cache @ {}", base);
    let limit_fragment = match stats.max_payload_bytes {
        Some(limit) => format!("limit {}", format_bytes(limit)),
        None => "limit off".to_string(),
    };
    if stats.capacity == 0 {
        let _ = writeln!(
            buf,
            "- status: disabled | capacity 0 | ttl {}s | entries {} | {}",
            stats.ttl_secs, stats.entries, limit_fragment
        );
    } else {
        let _ = writeln!(
            buf,
            "- status: enabled | capacity {} | ttl {}s | entries {} | {}",
            stats.capacity, stats.ttl_secs, stats.entries, limit_fragment
        );
    }

    let mut outcome_parts = vec![
        format!("hit {}", stats.hit),
        format!("miss {}", stats.miss),
        format!("coalesced {}", stats.coalesced),
        format!("bypass {}", stats.bypass),
        format!("errors {}", stats.errors),
    ];
    if stats.payload_too_large > 0 {
        outcome_parts.push(format!("payload>limit {}", stats.payload_too_large));
    }
    let mut outcomes = format!("- outcomes: {}", outcome_parts.join(" | "));
    let total = stats.hit + stats.miss;
    if total > 0 {
        let hit_rate = stats.hit as f64 / total as f64 * 100.0;
        let suppression = stats.stampede_suppression_rate * 100.0;
        outcomes.push_str(&format!(
            " (hit {:.1}%, suppression {:.1}%)",
            hit_rate, suppression
        ));
    }
    let _ = writeln!(buf, "{}", outcomes);

    if stats.latency_saved_samples > 0 {
        let mut line = format!(
            "- latency saved: avg {:.1} ms (samples {}, total {})",
            stats.avg_latency_saved_ms,
            stats.latency_saved_samples,
            format_duration_ms(stats.latency_saved_ms_total)
        );
        if let Some(last) = stats.last_latency_saved_ms {
            line.push_str(&format!(", last {} ms", last));
        }
        let _ = writeln!(buf, "{}", line);
    }

    if stats.payload_saved_samples > 0 {
        let mut line = format!(
            "- payload saved: avg {} (samples {}, total {})",
            format_bytes_f64(stats.avg_payload_bytes_saved),
            stats.payload_saved_samples,
            format_bytes(stats.payload_bytes_saved_total)
        );
        if let Some(last) = stats.last_payload_bytes {
            line.push_str(&format!(", last {}", format_bytes(last)));
        }
        let _ = writeln!(buf, "{}", line);
    }

    if stats.hit_age_samples > 0 {
        let mut line = format!(
            "- hit age: avg {} (samples {})",
            format_seconds_f64(stats.avg_hit_age_secs),
            stats.hit_age_samples
        );
        if let Some(last) = stats.last_hit_age_secs {
            line.push_str(&format!(", last {}", format_seconds(last)));
        }
        if let Some(max) = stats.max_hit_age_secs {
            line.push_str(&format!(", max {}", format_seconds(max)));
        }
        let _ = writeln!(buf, "{}", line);
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_cache_summary_includes_key_metrics() {
        let snapshot = ToolCacheSnapshot {
            hit: 8,
            miss: 2,
            coalesced: 3,
            errors: 1,
            bypass: 4,
            payload_too_large: 2,
            capacity: 128,
            ttl_secs: 600,
            entries: 42,
            max_payload_bytes: Some(1_048_576),
            latency_saved_ms_total: 12_500,
            latency_saved_samples: 5,
            avg_latency_saved_ms: 250.0,
            payload_bytes_saved_total: 512_000,
            payload_saved_samples: 5,
            avg_payload_bytes_saved: 102_400.0,
            avg_hit_age_secs: 18.5,
            hit_age_samples: 3,
            last_hit_age_secs: Some(12),
            max_hit_age_secs: Some(45),
            stampede_suppression_rate: 0.4,
            last_latency_saved_ms: Some(200),
            last_payload_bytes: Some(204_800),
            _extra: serde_json::Map::new(),
        };
        let summary = render_tool_cache_summary(&snapshot, "http://127.0.0.1:8091");
        assert!(summary.contains("Tool cache"));
        assert!(summary.contains("hit 8 | miss 2"));
        assert!(summary.contains("avg 250.0 ms"));
        assert!(summary.contains("avg 100.0 KB"));
        assert!(summary.contains("max 45 s"));
        assert!(summary.contains("limit 1.0 MB"));
        assert!(summary.contains("payload>limit 2"));
    }
}
