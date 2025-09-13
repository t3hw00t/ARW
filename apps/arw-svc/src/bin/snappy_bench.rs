use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, AUTHORIZATION};
use std::time::{Duration, Instant};

#[derive(Default, Debug, Clone)]
struct Budgets {
    i2f_p95_ms: u64,
    first_partial_p95_ms: u64,
    cadence_ms: u64,
}

fn budgets_from_env() -> Budgets {
    let get = |k: &str, d: u64| -> u64 {
        std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(d)
    };
    Budgets {
        i2f_p95_ms: get("ARW_SNAPPY_I2F_P95_MS", 50),
        first_partial_p95_ms: get("ARW_SNAPPY_FIRST_PARTIAL_P95_MS", 150),
        cadence_ms: get("ARW_SNAPPY_CADENCE_MS", 250),
    }
}

#[derive(Debug, Clone)]
struct Evt {
    kind: String,
    data: String,
    t: Instant,
}

async fn sse_connect(base: &str, admin: Option<&str>, prefixes: &[&str]) -> Result<(reqwest::Response, tokio::sync::mpsc::Receiver<Evt>)> {
    let url = if prefixes.is_empty() {
        format!("{}/admin/events", base.trim_end_matches('/'))
    } else {
        let mut u = format!("{}/admin/events?", base.trim_end_matches('/'));
        let mut first = true;
        for p in prefixes.iter() {
            if !first { u.push('&'); }
            first = false;
            u.push_str(&format!("prefix={}", p));
        }
        u
    };
    let client = reqwest::Client::builder().build()?;
    let mut req = client.get(&url).header(ACCEPT, "text/event-stream");
    if let Some(tok) = admin {
        if !tok.is_empty() {
            req = req.header(AUTHORIZATION, format!("Bearer {}", tok));
        }
    }
    let resp = req.send().await.context("sse send")?;
    if !resp.status().is_success() {
        return Err(anyhow!("sse status {}", resp.status()));
    }
    let (tx, rx) = tokio::sync::mpsc::channel::<Evt>(64);
    tokio::spawn(async move {
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let mut cur_event: Option<String> = None;
        let mut cur_data: Vec<String> = Vec::new();
        let t0 = Instant::now();
        while let Some(chunk) = stream.next().await {
            let Ok(bytes) = chunk else { break };
            for &b in bytes.iter() {
                buf.push(b);
                if b == b'\n' {
                    // process one line
                    let line = String::from_utf8_lossy(&buf).trim_end_matches('\n').to_string();
                    buf.clear();
                    if line.is_empty() {
                        // dispatch
                        if let Some(kind) = cur_event.take() {
                            let data = cur_data.join("\n");
                            let _ = tx.try_send(Evt { kind, data, t: Instant::now() });
                        }
                        cur_data.clear();
                        continue;
                    }
                    if let Some(rest) = line.strip_prefix("event:") {
                        cur_event = Some(rest.trim().to_string());
                    } else if let Some(rest) = line.strip_prefix("data:") {
                        cur_data.push(rest.trim().to_string());
                    } else {
                        // ignore other fields (id, retry)
                    }
                }
            }
            // Safety: in case event ends without trailing blank line, flush on small idle
            if buf.len() > 8192 { buf.clear(); }
            let _ = t0; // keep
        }
    });
    // We cannot return resp by value after spawned borrowing, so refetch
    let resp2 = reqwest::Client::new().get(&url).header(ACCEPT, "text/event-stream").send().await?; // placeholder (unused)
    Ok((resp2, rx))
}

fn p95(mut v: Vec<u64>) -> u64 {
    if v.is_empty() { return 0; }
    v.sort_unstable();
    let idx = ((v.len() as f64) * 0.95).ceil() as usize;
    let idx = idx.saturating_sub(1).min(v.len() - 1);
    v[idx]
}

#[tokio::main]
async fn main() -> Result<()> {
    let base = std::env::var("ARW_BENCH_BASE").unwrap_or_else(|_| "http://127.0.0.1:8090".to_string());
    let admin = std::env::var("ARW_ADMIN_TOKEN").ok();
    let budgets = budgets_from_env();

    // I2F (connect → first event)
    let t0 = Instant::now();
    let (_resp_unused, mut rx) = sse_connect(&base, admin.as_deref(), &["Service."]).await?;
    let i2f_ms = match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
        Ok(Some(_ev)) => t0.elapsed().as_millis() as u64,
        _ => 2_000,
    };

    // First partial (emit test → first Service.Test)
    let client = reqwest::Client::new();
    let url_emit = format!("{}/admin/emit/test", base.trim_end_matches('/'));
    let mut req = client.get(&url_emit);
    if let Some(tok) = &admin { req = req.header(AUTHORIZATION, format!("Bearer {}", tok)); }
    let t1 = Instant::now();
    let _ = req.send().await;
    let mut first_partial_ms: u64 = 2_000;
    let start = Instant::now();
    let deadline = Duration::from_millis(1500);
    while start.elapsed() < deadline {
        if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            if ev.kind == "Service.Test" {
                first_partial_ms = t1.elapsed().as_millis() as u64;
                break;
            }
        }
    }

    // Cadence: emit N events and collect inter-arrival deltas
    let n = std::env::var("ARW_BENCH_EVENTS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut times: Vec<Instant> = Vec::new();
    let mut deltas_ms: Vec<u64> = Vec::new();
    for _ in 0..n {
        let mut req = client.get(&url_emit);
        if let Some(tok) = &admin { req = req.header(AUTHORIZATION, format!("Bearer {}", tok)); }
        let _ = req.send().await;
        if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(1000), rx.recv()).await {
            if ev.kind == "Service.Test" { times.push(ev.t); }
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    for w in times.windows(2) {
        if let [a, b] = w { deltas_ms.push(b.duration_since(*a).as_millis() as u64); }
    }
    let cadence_p95 = p95(deltas_ms);

    println!("i2f_ms={} first_partial_ms={} cadence_p95_ms={}", i2f_ms, first_partial_ms, cadence_p95);
    let mut ok = true;
    if i2f_ms > budgets.i2f_p95_ms { ok = false; eprintln!("FAIL: i2f {}ms > budget {}ms", i2f_ms, budgets.i2f_p95_ms); }
    if first_partial_ms > budgets.first_partial_p95_ms { ok = false; eprintln!("FAIL: first_partial {}ms > budget {}ms", first_partial_ms, budgets.first_partial_p95_ms); }
    if cadence_p95 > budgets.cadence_ms { ok = false; eprintln!("FAIL: cadence_p95 {}ms > budget {}ms", cadence_p95, budgets.cadence_ms); }
    if !ok && std::env::var("ARW_BENCH_STRICT").ok().as_deref() == Some("1") {
        std::process::exit(1);
    }
    Ok(())
}
