use serde::Deserialize;
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
pub async fn allow_apply(action: &str, params: &Value) -> Result<(), String> {
    // Per-hour cap (very small by default)
    {
        let mut bw = bucket().write().await;
        let now = now_secs();
        if now - bw.window_start >= 3600 { bw.window_start = now; bw.count = 0; }
        let cap: u32 = load_cfg().map(|c| c.apply_per_hour.unwrap_or(3)).unwrap_or(3);
        let cap = std::env::var("ARW_FEEDBACK_APPLY_PER_HOUR").ok().and_then(|s| s.parse().ok()).unwrap_or(cap);
        if bw.count >= cap { return Err("rate limit reached".into()); }
        // don't increment yet; only if bounds pass
    }

    // Bounds per action
    let cfg = load_cfg();
    let http_min = std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MIN").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.http_timeout_min)).unwrap_or(5u64);
    let http_max = std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MAX").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.http_timeout_max)).unwrap_or(300u64);
    let mem_min = std::env::var("ARW_FEEDBACK_MEM_LIMIT_MIN").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.mem_limit_min)).unwrap_or(50u64);
    let mem_max = std::env::var("ARW_FEEDBACK_MEM_LIMIT_MAX").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.mem_limit_max)).unwrap_or(2000u64);
    let ok_bounds = match action {
        "hint" => params.get("http_timeout_secs").and_then(|v| v.as_u64()).map(|n| (http_min..=http_max).contains(&n)).unwrap_or(false),
        "mem_limit" => params.get("limit").and_then(|v| v.as_u64()).map(|n| (mem_min..=mem_max).contains(&n)).unwrap_or(false),
        "profile" => params.get("name").and_then(|v| v.as_str()).map(|s| matches!(s, "performance"|"balanced"|"power-saver")).unwrap_or(false),
        _ => false,
    };
    if !ok_bounds { return Err("out of bounds".into()); }

    // Increment cap after passing bounds
    let mut bw = bucket().write().await;
    let now = now_secs();
    if now - bw.window_start >= 3600 { bw.window_start = now; bw.count = 0; }
    let cap: u32 = load_cfg().map(|c| c.apply_per_hour.unwrap_or(3)).unwrap_or(3);
    let cap = std::env::var("ARW_FEEDBACK_APPLY_PER_HOUR").ok().and_then(|s| s.parse().ok()).unwrap_or(cap);
    if bw.count >= cap { return Err("rate limit reached".into()); }
    bw.count += 1;
    Ok(())
}

// Optional config file at configs/feedback.toml
#[derive(Deserialize, Default, Clone)]
struct FbPolicyCfg {
    apply_per_hour: Option<u32>,
    http_timeout_min: Option<u64>,
    http_timeout_max: Option<u64>,
    mem_limit_min: Option<u64>,
    mem_limit_max: Option<u64>,
}
fn load_cfg() -> Option<FbPolicyCfg> {
    static CFG: OnceLock<Option<FbPolicyCfg>> = OnceLock::new();
    CFG.get_or_init(|| {
        let p = std::path::Path::new("configs/feedback.toml");
        if let Ok(s) = std::fs::read_to_string(p) {
            toml::from_str::<FbPolicyCfg>(&s).ok()
        } else { None }
    }).clone()
}

// Public effective policy (for UI/help): merges config + env overrides
pub fn super_effective_policy() -> serde_json::Value {
    let cfg = load_cfg();
    let http_timeout_min = std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MIN").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.http_timeout_min)).unwrap_or(5u64);
    let http_timeout_max = std::env::var("ARW_FEEDBACK_HTTP_TIMEOUT_MAX").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.http_timeout_max)).unwrap_or(300u64);
    let mem_limit_min = std::env::var("ARW_FEEDBACK_MEM_LIMIT_MIN").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.mem_limit_min)).unwrap_or(50u64);
    let mem_limit_max = std::env::var("ARW_FEEDBACK_MEM_LIMIT_MAX").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.mem_limit_max)).unwrap_or(2000u64);
    let apply_per_hour = std::env::var("ARW_FEEDBACK_APPLY_PER_HOUR").ok().and_then(|s| s.parse().ok()).or_else(|| cfg.as_ref().and_then(|c| c.apply_per_hour)).unwrap_or(3u32);
    serde_json::json!({
        "http_timeout_min": http_timeout_min,
        "http_timeout_max": http_timeout_max,
        "mem_limit_min": mem_limit_min,
        "mem_limit_max": mem_limit_max,
        "apply_per_hour": apply_per_hour
    })
}
