use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use arw_otel::init_with_service;
use clap::Parser;
use dashmap::DashMap;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, Notify};

#[derive(Parser, Debug, Clone)]
#[command(
    name = "snappy-bench",
    about = "Interactive performance bench for arw-server"
)]
struct Args {
    /// Base URL of the server
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Number of requests to issue
    #[arg(long, default_value_t = 100)]
    requests: u32,
    /// Concurrent workers
    #[arg(long, default_value_t = 8)]
    concurrency: usize,
    /// Action kind to invoke
    #[arg(long, default_value = "demo.echo")]
    kind: String,
    /// Inline JSON payload for the action input
    #[arg(long)]
    payload: Option<String>,
    /// Path to JSON file for the action input
    #[arg(long)]
    payload_file: Option<String>,
    /// Connect timeout in seconds for HTTP requests
    #[arg(long, default_value_t = 10)]
    connect_timeout_secs: u64,
    /// Request timeout in seconds for HTTP calls
    #[arg(long, default_value_t = 30)]
    request_timeout_secs: u64,
    /// Seconds to wait for all completions before timing out
    #[arg(long, default_value_t = 60)]
    wait_timeout_secs: u64,
    /// Override full-result p95 budget (ms). Defaults to ARW_SNAPPY_FULL_RESULT_P95_MS or 2000ms.
    #[arg(long)]
    budget_full_ms: Option<f64>,
    /// Override queue wait p95 budget (ms). Defaults to ARW_SNAPPY_I2F_P95_MS or 50ms.
    #[arg(long)]
    budget_queue_ms: Option<f64>,
    /// Write a JSON summary report to the provided path
    #[arg(long)]
    json_out: Option<String>,
}

#[derive(Clone, Copy, Debug)]
enum FinishKind {
    Completed,
    Failed,
}

#[derive(Default, Debug, Clone)]
struct Timeline {
    start: Option<Instant>,
    http_accepted: Option<Instant>,
    submitted: Option<Instant>,
    running: Option<Instant>,
    finished: Option<Instant>,
    finish_kind: Option<FinishKind>,
    failure_reason: Option<String>,
}

impl Timeline {
    fn ready(&self) -> bool {
        self.start.is_some() && self.finished.is_some() && self.finish_kind.is_some()
    }

    fn into_result(self, id: Option<String>) -> ActionResult {
        let mut timeline = self;
        // ensure failure reason when finish kind missing
        if timeline.finish_kind.is_none() {
            timeline.finish_kind = Some(FinishKind::Failed);
            timeline.failure_reason = Some("missing finish state".to_string());
        }
        if timeline.finished.is_none() {
            timeline.finished = timeline.start.map(|s| s + Duration::from_millis(0));
            if timeline.failure_reason.is_none() {
                timeline.failure_reason = Some("missing completion timestamp".to_string());
            }
        }
        ActionResult::from_timeline(id, timeline)
    }
}

type TimelineMap = DashMap<String, Timeline>;

#[derive(Debug)]
struct ActionResult {
    id: Option<String>,
    status: ActionStatus,
    total_ms: Option<f64>,
    submit_delay_ms: Option<f64>,
    queue_ms: Option<f64>,
    run_ms: Option<f64>,
    http_ms: Option<f64>,
    raw_reason: Option<String>,
}

#[derive(Debug)]
enum ActionStatus {
    Completed,
    Failed,
}

impl ActionResult {
    fn from_timeline(id: Option<String>, tl: Timeline) -> Self {
        let start = tl.start;
        let finished = tl.finished;
        let status = match tl.finish_kind.unwrap_or(FinishKind::Failed) {
            FinishKind::Completed => ActionStatus::Completed,
            FinishKind::Failed => ActionStatus::Failed,
        };
        let total_ms = match (start, finished) {
            (Some(s), Some(f)) => Some(duration_ms(s, f)),
            _ => None,
        };
        let submit_delay_ms = match (start, tl.submitted) {
            (Some(s), Some(sub)) => Some(duration_ms(s, sub)),
            _ => None,
        };
        let queue_ms = match (tl.submitted, tl.running) {
            (Some(sub), Some(run)) => Some(duration_ms(sub, run)),
            _ => None,
        };
        let run_ms = match (tl.running, finished) {
            (Some(run), Some(fin)) => Some(duration_ms(run, fin)),
            _ => None,
        };
        let http_ms = match (start, tl.http_accepted) {
            (Some(s), Some(h)) => Some(duration_ms(s, h)),
            _ => None,
        };
        Self {
            id,
            status,
            total_ms,
            submit_delay_ms,
            queue_ms,
            run_ms,
            http_ms,
            raw_reason: tl.failure_reason,
        }
    }

    fn failure(id: Option<String>, reason: impl Into<String>, elapsed: Option<f64>) -> Self {
        Self {
            id,
            status: ActionStatus::Failed,
            total_ms: elapsed,
            submit_delay_ms: None,
            queue_ms: None,
            run_ms: None,
            http_ms: elapsed,
            raw_reason: Some(reason.into()),
        }
    }

    fn reason(&self) -> Option<&str> {
        self.raw_reason.as_deref()
    }
}

#[derive(Serialize)]
struct BenchBudgets {
    queue_p95_ms: f64,
    full_p95_ms: f64,
}

#[derive(Serialize, Default)]
struct BenchLatencyStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<StatSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    queue: Option<StatSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<StatSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http: Option<StatSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    submit: Option<StatSummary>,
}

#[derive(Serialize)]
struct BenchFailure {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
struct BenchSummary {
    requests: usize,
    completed: usize,
    failed: usize,
    elapsed_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    throughput_per_sec: Option<f64>,
    budgets_ms: BenchBudgets,
    latency_ms: BenchLatencyStats,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    failures: Vec<BenchFailure>,
}

#[derive(Default)]
struct SseReader {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    id: Option<String>,
}

impl SseReader {
    fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    fn next_event(&mut self) -> Option<SseMessage> {
        loop {
            let newline_pos = self.buffer.iter().position(|&b| b == b'\n')?;
            let mut line: Vec<u8> = self.buffer.drain(..=newline_pos).collect();
            if let Some(b'\n') = line.last() {
                line.pop();
            }
            if let Some(b'\r') = line.last() {
                line.pop();
            }
            if line.is_empty() {
                if self.event.is_none() && self.data.is_empty() && self.id.is_none() {
                    self.reset_current();
                    continue;
                }
                let msg = SseMessage {
                    event: self.event.take(),
                    data: self.data.join("\n"),
                    id: self.id.take(),
                };
                self.reset_current();
                return Some(msg);
            }
            if line.starts_with(b":") {
                continue;
            }
            let mut parts = line.splitn(2, |&b| b == b':');
            let field = parts.next().unwrap();
            let value_bytes = parts.next().unwrap_or(&[]);
            let value = if value_bytes.starts_with(b" ") {
                &value_bytes[1..]
            } else {
                value_bytes
            };
            let value_str = String::from_utf8_lossy(value).to_string();
            match String::from_utf8_lossy(field).as_ref() {
                "event" => self.event = Some(value_str.trim().to_string()),
                "data" => self.data.push(value_str),
                "id" => {
                    if !value_str.is_empty() {
                        self.id = Some(value_str.trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }

    fn reset_current(&mut self) {
        self.event = None;
        self.data.clear();
        self.id = None;
    }
}

struct SseMessage {
    #[allow(dead_code)]
    event: Option<String>,
    data: String,
    #[allow(dead_code)]
    id: Option<String>,
}

#[derive(Deserialize)]
struct Envelope {
    kind: String,
    payload: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_with_service("snappy-bench");
    let args = Args::parse();
    anyhow::ensure!(args.requests > 0, "requests must be > 0");
    anyhow::ensure!(args.concurrency > 0, "concurrency must be > 0");

    let payload = Arc::new(load_payload(&args)?);
    let base = args.base.trim_end_matches('/').to_string();
    let admin_token = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(args.connect_timeout_secs))
        .timeout(Duration::from_secs(args.request_timeout_secs))
        .build()
        .context("build HTTP client")?;

    let timeline = Arc::new(TimelineMap::new());
    let (result_tx, mut result_rx) =
        mpsc::channel::<ActionResult>((args.requests as usize).max(32));
    let ready = Arc::new(Notify::new());
    let stop = Arc::new(Notify::new());

    let sse_handle = {
        let client = client.clone();
        let base = base.clone();
        let token = admin_token.clone();
        let timeline = Arc::clone(&timeline);
        let result_tx = result_tx.clone();
        let ready = Arc::clone(&ready);
        let stop = Arc::clone(&stop);
        tokio::spawn(async move {
            if let Err(err) =
                listen_events(client, base, token, timeline, result_tx, ready, stop).await
            {
                eprintln!("[events] {err:?}");
            }
        })
    };

    // Wait for SSE to be ready (gives up after connect timeout)
    tokio::time::timeout(
        Duration::from_secs(args.connect_timeout_secs.max(5)),
        ready.notified(),
    )
    .await
    .context("waiting for /events stream")?;

    let bench_start = Instant::now();
    dispatch_requests(
        args.requests,
        args.concurrency,
        client.clone(),
        base.clone(),
        admin_token.clone(),
        args.kind.clone(),
        payload.clone(),
        timeline.clone(),
        result_tx.clone(),
    )
    .await?;
    drop(result_tx);

    let wait_deadline = Duration::from_secs(args.wait_timeout_secs);
    let mut results = Vec::new();
    while results.len() < args.requests as usize {
        match tokio::time::timeout(wait_deadline, result_rx.recv()).await {
            Ok(Some(res)) => results.push(res),
            Ok(None) => break,
            Err(_) => {
                eprintln!("timeout waiting for remaining completions");
                break;
            }
        }
    }

    // Drain any still-pending timelines as failures
    if results.len() < args.requests as usize {
        let now = Instant::now();
        let pending: Vec<String> = timeline.iter().map(|entry| entry.key().clone()).collect();
        for id in pending {
            if let Some((_, mut tl)) = timeline.remove(&id) {
                if tl.finished.is_none() {
                    tl.finished = Some(now);
                }
                if tl.finish_kind.is_none() {
                    tl.finish_kind = Some(FinishKind::Failed);
                }
                if tl.failure_reason.is_none() {
                    tl.failure_reason = Some("timed out waiting for completion".to_string());
                }
                results.push(tl.into_result(Some(id)));
            }
        }
    }

    stop.notify_waiters();
    let _ = sse_handle.await;

    let elapsed = bench_start.elapsed();
    let budget_full = args
        .budget_full_ms
        .or_else(|| env_f64("ARW_SNAPPY_FULL_RESULT_P95_MS"))
        .unwrap_or(2000.0);
    let budget_queue = args
        .budget_queue_ms
        .or_else(|| env_f64("ARW_SNAPPY_I2F_P95_MS"))
        .unwrap_or(50.0);

    let (exit_code, summary) = summarize(
        &results,
        args.requests as usize,
        elapsed,
        budget_full,
        budget_queue,
    );
    if let Some(path) = args.json_out.as_deref() {
        let data = serde_json::to_vec_pretty(&summary).context("serialize snappy bench summary")?;
        fs::write(path, &data)
            .with_context(|| format!("write snappy bench summary to {}", path))?;
        println!("- JSON summary written to {}", path);
    }
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_requests(
    total: u32,
    concurrency: usize,
    client: reqwest::Client,
    base: String,
    token: Option<String>,
    kind: String,
    payload: Arc<Value>,
    timeline: Arc<TimelineMap>,
    result_tx: mpsc::Sender<ActionResult>,
) -> Result<()> {
    use std::sync::atomic::{AtomicU32, Ordering};
    let counter = Arc::new(AtomicU32::new(0));
    let mut handles = Vec::new();
    for _ in 0..concurrency {
        let client = client.clone();
        let base = base.clone();
        let token = token.clone();
        let timeline = Arc::clone(&timeline);
        let payload = Arc::clone(&payload);
        let kind = kind.clone();
        let counter = Arc::clone(&counter);
        let result_tx = result_tx.clone();
        let handle = tokio::spawn(async move {
            loop {
                let idx = counter.fetch_add(1, Ordering::Relaxed);
                if idx >= total {
                    break;
                }
                if let Err(err) = send_one(
                    &client,
                    &base,
                    token.as_deref(),
                    &kind,
                    Arc::clone(&payload),
                    &timeline,
                    &result_tx,
                )
                .await
                {
                    let msg = format!("request error: {err}");
                    let _ = result_tx.send(ActionResult::failure(None, msg, None)).await;
                }
            }
        });
        handles.push(handle);
    }
    for h in handles {
        let _ = h.await;
    }
    Ok(())
}

async fn send_one(
    client: &reqwest::Client,
    base: &str,
    token: Option<&str>,
    kind: &str,
    payload: Arc<Value>,
    timeline: &Arc<TimelineMap>,
    result_tx: &mpsc::Sender<ActionResult>,
) -> Result<()> {
    let start = Instant::now();
    let body = serde_json::json!({
        "kind": kind,
        "input": payload.as_ref(),
    });
    let mut req = client.post(format!("{}/actions", base));
    if let Some(tok) = token {
        req = req.bearer_auth(tok);
    }
    let resp = req.json(&body).send().await.context("POST /actions")?;
    let http_accepted = Instant::now();
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let reason = format!("http {} {}", status.as_u16(), truncate(&text, 160));
        let elapsed = duration_ms(start, http_accepted);
        let _ = result_tx
            .send(ActionResult::failure(None, reason, Some(elapsed)))
            .await;
        return Ok(());
    }
    let json: Value = resp.json().await.context("parse /actions response")?;
    let id = json
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("response missing id"))?
        .to_string();
    let ready = {
        let mut entry = timeline.entry(id.clone()).or_default();
        entry.start.get_or_insert(start);
        entry.http_accepted.get_or_insert(http_accepted);
        entry.ready()
    };
    if ready {
        if let Some((_, entry)) = timeline.remove(&id) {
            let result = entry.into_result(Some(id));
            let _ = result_tx.send(result).await;
        }
    }
    Ok(())
}

async fn listen_events(
    client: reqwest::Client,
    base: String,
    token: Option<String>,
    timeline: Arc<TimelineMap>,
    result_tx: mpsc::Sender<ActionResult>,
    ready: Arc<Notify>,
    stop: Arc<Notify>,
) -> Result<()> {
    let mut req = client
        .get(format!("{}/events", base))
        .query(&[("prefix", "actions.")]);
    if let Some(tok) = token.as_deref() {
        req = req.bearer_auth(tok);
    }
    req = req.header("accept", "text/event-stream");
    let resp = req.send().await.context("connect /events")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!(
            "/events failed: {} {}",
            status,
            truncate(&body, 200)
        ));
    }
    ready.notify_waiters();

    let mut reader = SseReader::default();
    let mut stream = resp.bytes_stream();
    loop {
        tokio::select! {
            _ = stop.notified() => break,
            chunk = stream.next() => {
                match chunk {
                    Some(Ok(bytes)) => {
                        reader.push(&bytes);
                        while let Some(msg) = reader.next_event() {
                            if let Err(err) = handle_sse_message(
                                msg,
                                &timeline,
                                &result_tx,
                            ).await {
                                eprintln!("[events] {err}");
                            }
                        }
                    }
                    Some(Err(err)) => {
                        return Err(err.into());
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

async fn handle_sse_message(
    msg: SseMessage,
    timeline: &Arc<TimelineMap>,
    result_tx: &mpsc::Sender<ActionResult>,
) -> Result<()> {
    if msg.data.trim().is_empty() {
        return Ok(());
    }
    let envelope: Envelope = match serde_json::from_str(&msg.data) {
        Ok(env) => env,
        Err(_) => return Ok(()),
    };
    if !envelope.kind.starts_with("actions.") {
        return Ok(());
    }
    let id = match envelope.payload.get("id").and_then(value_to_string) {
        Some(id) => id,
        None => return Ok(()),
    };
    let now = Instant::now();
    let ready = {
        let mut entry = timeline.entry(id.clone()).or_default();
        match envelope.kind.as_str() {
            "actions.submitted" => {
                entry.submitted.get_or_insert(now);
            }
            "actions.running" => {
                entry.running.get_or_insert(now);
            }
            "actions.completed" => {
                entry.finished.get_or_insert(now);
                entry.finish_kind = Some(FinishKind::Completed);
            }
            "actions.failed" => {
                entry.finished.get_or_insert(now);
                entry.finish_kind = Some(FinishKind::Failed);
                if entry.failure_reason.is_none() {
                    entry.failure_reason = envelope
                        .payload
                        .get("error")
                        .and_then(value_to_string)
                        .or_else(|| Some("action failed".to_string()));
                }
            }
            _ => {}
        }
        entry.ready()
    };
    if ready {
        if let Some((_, entry)) = timeline.remove(&id) {
            let result = entry.into_result(Some(id));
            let _ = result_tx.send(result).await;
        }
    }
    Ok(())
}

fn summarize(
    results: &[ActionResult],
    expected: usize,
    elapsed: Duration,
    budget_full_ms: f64,
    budget_queue_ms: f64,
) -> (i32, BenchSummary) {
    let completed: Vec<&ActionResult> = results
        .iter()
        .filter(|r| matches!(r.status, ActionStatus::Completed))
        .collect();
    let failed: Vec<&ActionResult> = results
        .iter()
        .filter(|r| matches!(r.status, ActionStatus::Failed))
        .collect();

    let mut total = Vec::new();
    let mut submit = Vec::new();
    let mut queue = Vec::new();
    let mut run = Vec::new();
    let mut http = Vec::new();
    for r in &completed {
        if let Some(v) = r.total_ms {
            total.push(v);
        }
        if let Some(v) = r.submit_delay_ms {
            submit.push(v);
        }
        if let Some(v) = r.queue_ms {
            queue.push(v);
        }
        if let Some(v) = r.run_ms {
            run.push(v);
        }
        if let Some(v) = r.http_ms {
            http.push(v);
        }
    }

    println!("Snappy Bench Summary");
    let elapsed_secs = elapsed.as_secs_f64();
    println!("- Requests: {}", expected);
    println!("- Completed: {}", completed.len());
    println!("- Failed: {}", failed.len());
    println!("- Elapsed: {:.2}s", elapsed_secs);
    let throughput = if elapsed_secs > 0.0 {
        let value = completed.len() as f64 / elapsed_secs;
        println!("- Throughput: {:.2} actions/s", value);
        Some(value)
    } else {
        None
    };

    let stats_total = compute_stats(&total);
    let stats_queue = compute_stats(&queue);
    let stats_run = compute_stats(&run);
    let stats_http = compute_stats(&http);
    let stats_submit = compute_stats(&submit);

    if let Some(ref stats) = stats_total {
        print_stats("Total", stats);
    }
    if let Some(ref stats) = stats_queue {
        print_stats("Queue wait", stats);
    }
    if let Some(ref stats) = stats_run {
        print_stats("Run", stats);
    }
    if let Some(ref stats) = stats_http {
        print_stats("HTTP ack", stats);
    }

    if !failed.is_empty() {
        println!("- Failure reasons:");
        for (idx, item) in failed.iter().enumerate() {
            if idx >= 5 {
                println!("  * ... {} more", failed.len() - idx);
                break;
            }
            let reason = item
                .reason()
                .map(truncate_reason)
                .unwrap_or_else(|| "unknown".to_string());
            let label = item.id.clone().unwrap_or_else(|| "(no id)".to_string());
            println!("  * {} → {}", label, reason);
        }
    }

    let mut exit_code = 0;
    if let Some(ref stats) = stats_total {
        if stats.p95 > budget_full_ms {
            println!(
                "! Budget breach: total p95 {:.1} ms > {:.1} ms",
                stats.p95, budget_full_ms
            );
            exit_code = exit_code.max(1);
        }
    }
    if let Some(ref stats) = stats_queue {
        if stats.p95 > budget_queue_ms {
            println!(
                "! Budget breach: queue p95 {:.1} ms > {:.1} ms",
                stats.p95, budget_queue_ms
            );
            exit_code = exit_code.max(1);
        }
    }
    if !failed.is_empty() {
        exit_code = exit_code.max(2);
    }
    let failures: Vec<BenchFailure> = failed
        .iter()
        .take(10)
        .map(|item| BenchFailure {
            id: item.id.clone(),
            reason: item.reason().map(|s| s.to_string()),
        })
        .collect();

    let summary = BenchSummary {
        requests: expected,
        completed: completed.len(),
        failed: failed.len(),
        elapsed_seconds: elapsed_secs,
        throughput_per_sec: throughput,
        budgets_ms: BenchBudgets {
            queue_p95_ms: budget_queue_ms,
            full_p95_ms: budget_full_ms,
        },
        latency_ms: BenchLatencyStats {
            total: stats_total.clone(),
            queue: stats_queue.clone(),
            run: stats_run.clone(),
            http: stats_http.clone(),
            submit: stats_submit.clone(),
        },
        failures,
    };
    (exit_code, summary)
}

#[derive(Clone, Serialize)]
struct StatSummary {
    avg: f64,
    p50: f64,
    p95: f64,
    max: f64,
}

fn compute_stats(values: &[f64]) -> Option<StatSummary> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let avg = values.iter().sum::<f64>() / values.len() as f64;
    let p50 = percentile(&sorted, 0.50);
    let p95 = percentile(&sorted, 0.95);
    let max = *sorted.last().unwrap();
    Some(StatSummary { avg, p50, p95, max })
}

fn percentile(sorted: &[f64], ratio: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let idx = ratio * (sorted.len() - 1) as f64;
    let low = idx.floor() as usize;
    let high = idx.ceil() as usize;
    if low == high {
        sorted[low]
    } else {
        let lower = sorted[low];
        let upper = sorted[high];
        lower + (upper - lower) * (idx - low as f64)
    }
}

fn print_stats(label: &str, stats: &StatSummary) {
    println!(
        "- {} latency (ms): avg {:.1} | p50 {:.1} | p95 {:.1} | max {:.1}",
        label, stats.avg, stats.p50, stats.p95, stats.max
    );
}

fn duration_ms(start: Instant, end: Instant) -> f64 {
    end.saturating_duration_since(start).as_secs_f64() * 1000.0
}

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        _ => Some(truncate(&v.to_string(), 120)),
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

fn truncate_reason(s: &str) -> String {
    truncate(s, 160)
}

fn env_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok()?.parse().ok()
}

fn load_payload(args: &Args) -> Result<Value> {
    if let Some(ref path) = args.payload_file {
        let bytes = fs::read(path).with_context(|| format!("read payload file {path}"))?;
        return Ok(serde_json::from_slice(&bytes)?);
    }
    if let Some(ref inline) = args.payload {
        return Ok(serde_json::from_str(inline)?);
    }
    Ok(serde_json::json!({"msg": "snappy-bench"}))
}
