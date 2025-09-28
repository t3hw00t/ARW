use serde_json::json;
use std::{
    fs,
    io::Write as _,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::AppState;
use std::sync::atomic::AtomicU64;

static CRASH_COUNT: AtomicUsize = AtomicUsize::new(0);
static SAFE_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

fn state_dir() -> PathBuf {
    // Avoid a hard dependency on config init here; util::state_dir falls back sensibly.
    crate::util::state_dir()
}

fn crash_dir() -> PathBuf {
    state_dir().join("crash")
}

fn ts_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn install() {
    // Only install once per process.
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let _ = std::panic::take_hook(); // remove default to avoid duplicate writes
        std::panic::set_hook(Box::new(|info| {
            let count = CRASH_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            let when = ts_ms();
            let thread = std::thread::current();
            let tname = thread.name().unwrap_or("unnamed");
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                *s
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.as_str()
            } else {
                "<unknown panic payload>"
            };
            let loc = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "<unknown>".into());

            let _ = fs::create_dir_all(crash_dir());
            let file = crash_dir().join(format!("panic-{}-{}.json", when, tname));
            let payload = json!({
                "ts_ms": when,
                "thread": tname,
                "message": msg,
                "location": loc,
                "count": count,
                "backtrace": std::env::var("RUST_BACKTRACE").ok().filter(|v| v != "0"),
            });
            if let Ok(mut f) = fs::File::create(&file) {
                let _ = writeln!(
                    f,
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or("{}".into())
                );
            }
            eprintln!("panic captured (#{count}) at {loc} on thread '{tname}': {msg}");
        }));
    });
}

pub async fn sweep_on_start(state: &AppState) {
    let dir = crash_dir();
    let mut crashed_files = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                crashed_files.push(path);
            }
        }
    }
    if crashed_files.is_empty() {
        return;
    }

    // Publish a summary health event for observability.
    let count = crashed_files.len();
    let last = crashed_files
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.split('-').nth(1))
        })
        .filter_map(|s| s.parse::<u64>().ok())
        .max()
        .unwrap_or_else(ts_ms);
    state.bus().publish(
        arw_topics::TOPIC_SERVICE_HEALTH,
        &json!({
            "status": "recovered",
            "crash_files": count,
            "last_panic_ms": last,
        }),
    );

    // Best-effort: move files to an archive subdir so subsequent boots don't re-announce.
    let archive = dir.join("archive");
    let _ = fs::create_dir_all(&archive);
    for path in crashed_files {
        if let Some(file) = path.file_name() {
            let _ = fs::rename(&path, archive.join(file));
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn recent_crashes_within(window_ms: u64) -> (usize, Option<u64>) {
    let mut count = 0usize;
    let mut last: Option<u64> = None;
    let window_start = now_ms().saturating_sub(window_ms);
    let dir = crash_dir();
    if let Ok(rd) = fs::read_dir(&dir) {
        for ent in rd.flatten() {
            let name = ent.file_name();
            let name = name.to_string_lossy();
            // Expected: panic-<ts>-<thread>.json
            if let Some(ts_part) = name.split('-').nth(1) {
                if let Ok(ts) = ts_part.parse::<u64>() {
                    if ts >= window_start {
                        count += 1;
                        if last.map(|l| ts > l).unwrap_or(true) {
                            last = Some(ts);
                        }
                    }
                }
            }
        }
    }
    (count, last)
}

/// If enabled, enter a short safe-mode delay when recent crash markers are present.
pub fn maybe_enter_safe_mode(state: &AppState) {
    let on = std::env::var("ARW_SAFE_MODE_ON_CRASH")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(true);
    if !on {
        return;
    }
    let window_ms: u64 = std::env::var("ARW_SAFE_MODE_RECENT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10 * 60 * 1000);
    let min_count: usize = std::env::var("ARW_SAFE_MODE_MIN_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let defer_secs: u64 = std::env::var("ARW_SAFE_MODE_DEFER_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let (count, last) = recent_crashes_within(window_ms);
    if count >= min_count {
        let until = now_ms().saturating_add(defer_secs.saturating_mul(1000));
        SAFE_UNTIL_MS.store(until, std::sync::atomic::Ordering::Relaxed);
        let payload = serde_json::json!({
            "status": "degraded",
            "component": "safe_mode",
            "reason": "recent_crash",
            "restarts_window": count,
            "window_ms": window_ms,
            "delay_ms": defer_secs * 1000,
            "last_panic_ms": last,
        });
        state
            .bus()
            .publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
    }
}

/// Await the initial safe-mode delay once. No-op if not active.
pub async fn await_initial_delay() {
    let until = SAFE_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed);
    if until == 0 {
        return;
    }
    let now = now_ms();
    if until <= now {
        return;
    }
    let dur = std::time::Duration::from_millis(until - now);
    tokio::time::sleep(dur).await;
}

pub fn safe_mode_until_ms() -> u64 {
    SAFE_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed)
}
