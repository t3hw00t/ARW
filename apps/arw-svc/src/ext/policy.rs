use serde_json::Value;
use std::sync::OnceLock;
use tokio::sync::RwLock;

// Very small policy stub with a sliding hour window cap
struct ApplyWindow { window_start: i64, count: u32 }
static APPLY_WINDOW: OnceLock<RwLock<ApplyWindow>> = OnceLock::new();
fn bucket() -> &'static RwLock<ApplyWindow> {
    APPLY_WINDOW.get_or_init(|| RwLock::new(ApplyWindow { window_start: 0, count: 0 }))
}

fn now_secs() -> i64 { (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()) as i64 }

// Public: cheap check if an apply is allowed under current policy
pub async fn allow_apply(action: &str, params: &Value) -> bool {
    // Per-hour cap (very small by default)
    {
        let mut bw = bucket().write().await;
        let now = now_secs();
        if now - bw.window_start >= 3600 { bw.window_start = now; bw.count = 0; }
        if bw.count >= 3 { return false; }
        // don't increment yet; only if bounds pass
    }

    // Bounds per action
    let ok_bounds = match action {
        "hint" => params.get("http_timeout_secs").and_then(|v| v.as_u64()).map(|n| (5..=300).contains(&n)).unwrap_or(false),
        "mem_limit" => params.get("limit").and_then(|v| v.as_u64()).map(|n| (50..=2000).contains(&n)).unwrap_or(false),
        "profile" => params.get("name").and_then(|v| v.as_str()).map(|s| matches!(s, "performance"|"balanced"|"power-saver")).unwrap_or(false),
        _ => false,
    };
    if !ok_bounds { return false; }

    // Increment cap after passing bounds
    let mut bw = bucket().write().await;
    let now = now_secs();
    if now - bw.window_start >= 3600 { bw.window_start = now; bw.count = 0; }
    if bw.count >= 3 { return false; }
    bw.count += 1;
    true
}
