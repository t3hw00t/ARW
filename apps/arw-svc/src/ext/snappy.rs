use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Budgets {
    pub i2f_p95_ms: u64,
    pub first_partial_p95_ms: u64,
    pub cadence_ms: u64,
    pub full_result_p95_ms: u64,
}

fn budgets() -> Budgets {
    let env = |k: &str, d: u64| -> u64 {
        std::env::var(k)
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(d)
    };
    Budgets {
        i2f_p95_ms: env("ARW_SNAPPY_I2F_P95_MS", 50),
        first_partial_p95_ms: env("ARW_SNAPPY_FIRST_PARTIAL_P95_MS", 150),
        cadence_ms: env("ARW_SNAPPY_CADENCE_MS", 250),
        full_result_p95_ms: env("ARW_SNAPPY_FULL_RESULT_P95_MS", 2000),
    }
}

fn protected_endpoints() -> Vec<String> {
    if let Ok(v) = std::env::var("ARW_SNAPPY_PROTECTED_ENDPOINTS") {
        v.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![
            "/debug".into(),
            "/state/".into(),
            "/chat/".into(),
            "/admin/events".into(),
        ]
    }
}

fn path_matches(path: &str, pat: &str) -> bool {
    if pat.ends_with("/*") {
        let p = pat.trim_end_matches("/*");
        return path.starts_with(p);
    }
    if pat.ends_with('/') {
        return path.starts_with(pat);
    }
    path == pat
}

async fn summarize_interactive() -> (u64, HashMap<String, u64>) {
    let p95_map = crate::ext::stats::routes_p95_by_path().await;
    let mut sel: HashMap<String, u64> = HashMap::new();
    let mut max_p95: u64 = 0;
    let pats = protected_endpoints();
    for (path, p95) in p95_map.into_iter() {
        if pats.iter().any(|pat| path_matches(&path, pat)) {
            max_p95 = max_p95.max(p95);
            sel.insert(path, p95);
        }
    }
    (max_p95, sel)
}

pub async fn start_snappy_publisher(bus: arw_events::Bus) {
    let idle_ms: u64 = std::env::var("ARW_SNAPPY_PUBLISH_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000)
        .max(200);
    let mut intv = tokio::time::interval(std::time::Duration::from_millis(idle_ms));
    loop {
        intv.tick().await;
        publish_snappy(&bus).await;
    }
}

async fn publish_snappy(bus: &arw_events::Bus) {
    let budgets = budgets();
    let (interactive_p95, p95_by_path) = summarize_interactive().await;
    let breach = interactive_p95 > budgets.full_result_p95_ms;
    // Compact read-model: budgets + current measured p95 for interactive slice
    let cur = json!({
        "budgets": budgets,
        "interactive": {
            "p95_max_ms": interactive_p95,
        },
        "breach": breach,
    });
    crate::ext::read_model::emit_patch_dual(
        bus,
        "State.Snappy.Patch",
        "State.ReadModel.Patch",
        "snappy",
        &cur,
    );
    if breach {
        let mut notice = json!({"p95_max_ms": interactive_p95, "budget_ms": budgets.full_result_p95_ms});
        crate::ext::corr::ensure_corr(&mut notice);
        bus.publish("Snappy.Notice", &notice);
    }
    // For deeper introspection (opt-in), publish a detailed but infrequent map
    let detail_every = std::env::var("ARW_SNAPPY_DETAIL_EVERY")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    if detail_every > 0 {
        use std::time::SystemTime;
        static LAST: once_cell::sync::OnceCell<std::sync::Mutex<u64>> = once_cell::sync::OnceCell::new();
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let m = LAST.get_or_init(|| std::sync::Mutex::new(0));
        let mut last = m.lock().unwrap();
        if now.saturating_sub(*last) >= detail_every {
            *last = now;
            let mut pl = json!({"p95_by_path": p95_by_path});
            crate::ext::corr::ensure_corr(&mut pl);
            bus.publish("Snappy.Detail", &pl);
        }
    }
}

/// Pre‑warm hot lookups (schemas/tools/models); best‑effort/fast.
pub async fn prewarm() {
    // Touch models list and default to populate OnceLocks
    let _ = crate::ext::models().read().await;
    let _ = crate::ext::default_model().read().await;
    // Touch tool schemas (small) to populate code‑generated schema cache
    let _ = arw_core::introspect_schema("memory.probe");
    let _ = arw_core::introspect_schema("introspect.tools");
}

