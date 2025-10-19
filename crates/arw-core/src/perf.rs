use sysinfo::{CpuRefreshKind, RefreshKind, System};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfPreset {
    Eco,
    Balanced,
    Performance,
    Turbo,
}

#[derive(Debug, Clone)]
pub struct PerfTuning {
    pub http_max_conc: usize,
    pub actions_queue_max: i64,
    pub context_k: usize,
    pub context_expand: usize,
    pub context_lambda: f32,
    pub context_min_score: f32,
    pub context_lanes: &'static str,
    pub context_lane_bonus: f32,
    pub context_expand_query: bool,
    pub context_expand_query_top_k: usize,
    pub context_scorer: &'static str,
    pub context_stream_default: bool,
    pub context_max_iters: usize,
    pub rehydrate_file_head_kb: u64,
    pub route_stats_coalesce_ms: u64,
    pub route_stats_publish_ms: u64,
    pub models_metrics_coalesce_ms: u64,
    pub models_metrics_publish_ms: u64,
    pub workers_max: Option<usize>,
    pub tools_cache_ttl_secs: Option<u64>,
    pub tools_cache_cap: Option<u64>,
    pub disable_heavy_telemetry: bool,
}

impl PerfPreset {
    pub fn from_env() -> Option<Self> {
        match std::env::var("ARW_PERF_PRESET")
            .ok()
            .as_deref()
            .map(|s| s.trim().to_lowercase())
        {
            Some(ref s) if s == "eco" => Some(PerfPreset::Eco),
            Some(ref s) if s == "balanced" => Some(PerfPreset::Balanced),
            Some(ref s) if s == "performance" || s == "perf" => Some(PerfPreset::Performance),
            Some(ref s) if s == "turbo" || s == "max" => Some(PerfPreset::Turbo),
            _ => None,
        }
    }
}

/// Detect a reasonable preset from the host machine.
/// Very coarse: based on logical cores and memory size.
pub fn detect_preset() -> PerfPreset {
    let sys =
        System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()));
    let cpus = sys.cpus().len().max(1);
    let mut sys_full = System::new_all();
    sys_full.refresh_memory();
    let mem_gb = (sys_full.total_memory() as f64 / (1024.0 * 1024.0)).max(1.0) / 1024.0; // GiB

    // Heuristics tuned for local-first dev laptops
    match (cpus, mem_gb) {
        (c, m) if c <= 4 || m <= 8.0 => PerfPreset::Eco,
        (c, m) if c <= 8 || m <= 16.0 => PerfPreset::Balanced,
        (c, m) if c <= 16 || m <= 32.0 => PerfPreset::Performance,
        _ => PerfPreset::Turbo,
    }
}

pub fn tuning_for(preset: PerfPreset) -> PerfTuning {
    match preset {
        PerfPreset::Eco => PerfTuning {
            http_max_conc: 256,
            actions_queue_max: 256,
            context_k: 12,
            context_expand: 2,
            context_lambda: 0.65,
            context_min_score: 0.08,
            context_lanes: "semantic,procedural",
            context_lane_bonus: 0.08,
            context_expand_query: false,
            context_expand_query_top_k: 3,
            context_scorer: "mmrd",
            context_stream_default: false,
            context_max_iters: 2,
            rehydrate_file_head_kb: 32,
            route_stats_coalesce_ms: 350,
            route_stats_publish_ms: 2500,
            models_metrics_coalesce_ms: 350,
            models_metrics_publish_ms: 2500,
            workers_max: Some(4),
            tools_cache_ttl_secs: Some(300),
            tools_cache_cap: Some(256),
            disable_heavy_telemetry: true,
        },
        PerfPreset::Balanced => PerfTuning {
            http_max_conc: 1024,
            actions_queue_max: 1024,
            context_k: 18,
            context_expand: 3,
            context_lambda: 0.70,
            context_min_score: 0.1,
            context_lanes: "semantic,procedural,episodic",
            context_lane_bonus: 0.06,
            context_expand_query: true,
            context_expand_query_top_k: 4,
            context_scorer: "mmrd",
            context_stream_default: true,
            context_max_iters: 2,
            rehydrate_file_head_kb: 64,
            route_stats_coalesce_ms: 250,
            route_stats_publish_ms: 2000,
            models_metrics_coalesce_ms: 250,
            models_metrics_publish_ms: 2000,
            workers_max: None,
            tools_cache_ttl_secs: None,
            tools_cache_cap: None,
            disable_heavy_telemetry: false,
        },
        PerfPreset::Performance => PerfTuning {
            http_max_conc: 4096,
            actions_queue_max: 4096,
            context_k: 24,
            context_expand: 4,
            context_lambda: 0.74,
            context_min_score: 0.12,
            context_lanes: "semantic,procedural,episodic",
            context_lane_bonus: 0.05,
            context_expand_query: true,
            context_expand_query_top_k: 5,
            context_scorer: "mmrd",
            context_stream_default: true,
            context_max_iters: 3,
            rehydrate_file_head_kb: 96,
            route_stats_coalesce_ms: 150,
            route_stats_publish_ms: 1500,
            models_metrics_coalesce_ms: 150,
            models_metrics_publish_ms: 1500,
            workers_max: None,
            tools_cache_ttl_secs: None,
            tools_cache_cap: None,
            disable_heavy_telemetry: false,
        },
        PerfPreset::Turbo => PerfTuning {
            http_max_conc: 16384,
            actions_queue_max: 16384,
            context_k: 32,
            context_expand: 4,
            context_lambda: 0.78,
            context_min_score: 0.15,
            context_lanes: "semantic,procedural,episodic,insight",
            context_lane_bonus: 0.04,
            context_expand_query: true,
            context_expand_query_top_k: 6,
            context_scorer: "mmrd",
            context_stream_default: true,
            context_max_iters: 3,
            rehydrate_file_head_kb: 128,
            route_stats_coalesce_ms: 100,
            route_stats_publish_ms: 1000,
            models_metrics_coalesce_ms: 100,
            models_metrics_publish_ms: 1000,
            workers_max: None,
            tools_cache_ttl_secs: None,
            tools_cache_cap: None,
            disable_heavy_telemetry: false,
        },
    }
}

fn set_if_unset(key: &str, val: impl Into<String>) {
    if std::env::var_os(key).is_none() {
        std::env::set_var(key, val.into());
    }
}

/// Apply the selected performance preset by seeding environment defaults
/// for hot-path tunables if they are not already set by the user.
///
/// Order of precedence:
/// - Explicit env vars win
/// - ARW_PERF_PRESET selects a tier; when missing, auto-detected
/// - We log the effective tier and any defaults applied
pub fn apply_performance_preset() -> PerfPreset {
    let preset = PerfPreset::from_env().unwrap_or_else(detect_preset);
    let t = tuning_for(preset);

    // Seed widely used tunables only if unset
    set_if_unset("ARW_HTTP_MAX_CONC", t.http_max_conc.to_string());
    set_if_unset("ARW_ACTIONS_QUEUE_MAX", t.actions_queue_max.to_string());
    set_if_unset("ARW_CONTEXT_K", t.context_k.to_string());
    set_if_unset("ARW_CONTEXT_EXPAND_PER_SEED", t.context_expand.to_string());
    set_if_unset(
        "ARW_CONTEXT_DIVERSITY_LAMBDA",
        format!("{:.3}", t.context_lambda),
    );
    set_if_unset(
        "ARW_CONTEXT_MIN_SCORE",
        format!("{:.3}", t.context_min_score),
    );
    set_if_unset("ARW_CONTEXT_LANES_DEFAULT", t.context_lanes.to_string());
    set_if_unset(
        "ARW_CONTEXT_LANE_BONUS",
        format!("{:.3}", t.context_lane_bonus),
    );
    set_if_unset(
        "ARW_CONTEXT_EXPAND_QUERY",
        if t.context_expand_query { "1" } else { "0" },
    );
    set_if_unset(
        "ARW_CONTEXT_EXPAND_QUERY_TOP_K",
        t.context_expand_query_top_k.to_string(),
    );
    set_if_unset("ARW_CONTEXT_SCORER", t.context_scorer.to_string());
    set_if_unset(
        "ARW_CONTEXT_STREAM_DEFAULT",
        if t.context_stream_default { "1" } else { "0" },
    );
    set_if_unset(
        "ARW_CONTEXT_COVERAGE_MAX_ITERS",
        t.context_max_iters.to_string(),
    );
    set_if_unset(
        "ARW_REHYDRATE_FILE_HEAD_KB",
        t.rehydrate_file_head_kb.to_string(),
    );
    set_if_unset(
        "ARW_ROUTE_STATS_COALESCE_MS",
        t.route_stats_coalesce_ms.to_string(),
    );
    set_if_unset(
        "ARW_ROUTE_STATS_PUBLISH_MS",
        t.route_stats_publish_ms.to_string(),
    );
    set_if_unset(
        "ARW_MODELS_METRICS_COALESCE_MS",
        t.models_metrics_coalesce_ms.to_string(),
    );
    set_if_unset(
        "ARW_MODELS_METRICS_PUBLISH_MS",
        t.models_metrics_publish_ms.to_string(),
    );
    if let Some(max_workers) = t.workers_max {
        set_if_unset("ARW_WORKERS_MAX", max_workers.to_string());
    }
    if let Some(ttl) = t.tools_cache_ttl_secs {
        set_if_unset("ARW_TOOLS_CACHE_TTL_SECS", ttl.to_string());
    }
    if let Some(cap) = t.tools_cache_cap {
        set_if_unset("ARW_TOOLS_CACHE_CAP", cap.to_string());
    }
    if t.disable_heavy_telemetry {
        set_if_unset("ARW_OTEL", "0");
        set_if_unset("ARW_OTEL_METRICS", "0");
    }

    // Provide a canonical computed tier for observability
    std::env::set_var(
        "ARW_PERF_PRESET_TIER",
        match preset {
            PerfPreset::Eco => "eco",
            PerfPreset::Balanced => "balanced",
            PerfPreset::Performance => "performance",
            PerfPreset::Turbo => "turbo",
        },
    );

    info!(
        tier = std::env::var("ARW_PERF_PRESET_TIER")
            .unwrap_or_default()
            .as_str(),
        http_max_conc = t.http_max_conc,
        actions_queue_max = t.actions_queue_max,
        context_k = t.context_k,
        context_expand = t.context_expand,
        context_lambda = t.context_lambda,
        context_min_score = t.context_min_score,
        context_lanes = t.context_lanes,
        context_lane_bonus = t.context_lane_bonus,
        rehydrate_file_head_kb = t.rehydrate_file_head_kb,
        "Applied performance preset defaults (env-seeded if unset)"
    );
    preset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env;

    #[test]
    fn eco_preset_applies_low_power_defaults() {
        let mut guard = env::guard();
        guard.set("ARW_PERF_PRESET", "eco");
        guard.clear_keys(&[
            "ARW_PERF_PRESET_TIER",
            "ARW_HTTP_MAX_CONC",
            "ARW_ACTIONS_QUEUE_MAX",
            "ARW_CONTEXT_K",
            "ARW_CONTEXT_EXPAND_PER_SEED",
            "ARW_CONTEXT_DIVERSITY_LAMBDA",
            "ARW_CONTEXT_MIN_SCORE",
            "ARW_CONTEXT_LANES_DEFAULT",
            "ARW_CONTEXT_LANE_BONUS",
            "ARW_CONTEXT_EXPAND_QUERY",
            "ARW_CONTEXT_EXPAND_QUERY_TOP_K",
            "ARW_CONTEXT_SCORER",
            "ARW_CONTEXT_STREAM_DEFAULT",
            "ARW_CONTEXT_COVERAGE_MAX_ITERS",
            "ARW_REHYDRATE_FILE_HEAD_KB",
            "ARW_ROUTE_STATS_COALESCE_MS",
            "ARW_ROUTE_STATS_PUBLISH_MS",
            "ARW_MODELS_METRICS_COALESCE_MS",
            "ARW_MODELS_METRICS_PUBLISH_MS",
            "ARW_WORKERS_MAX",
            "ARW_TOOLS_CACHE_TTL_SECS",
            "ARW_TOOLS_CACHE_CAP",
            "ARW_OTEL",
            "ARW_OTEL_METRICS",
        ]);

        let preset = apply_performance_preset();
        assert_eq!(preset, PerfPreset::Eco);
        assert_eq!(std::env::var("ARW_WORKERS_MAX").unwrap(), "4");
        assert_eq!(std::env::var("ARW_TOOLS_CACHE_TTL_SECS").unwrap(), "300");
        assert_eq!(std::env::var("ARW_TOOLS_CACHE_CAP").unwrap(), "256");
        assert_eq!(std::env::var("ARW_OTEL").unwrap(), "0");
        assert_eq!(std::env::var("ARW_OTEL_METRICS").unwrap(), "0");
    }

    #[test]
    fn balanced_preset_leaves_optional_overrides_unset() {
        let mut guard = env::guard();
        guard.set("ARW_PERF_PRESET", "balanced");
        guard.clear_keys(&[
            "ARW_PERF_PRESET_TIER",
            "ARW_WORKERS_MAX",
            "ARW_TOOLS_CACHE_TTL_SECS",
            "ARW_TOOLS_CACHE_CAP",
            "ARW_OTEL",
            "ARW_OTEL_METRICS",
        ]);

        let preset = apply_performance_preset();
        assert_eq!(preset, PerfPreset::Balanced);
        assert!(std::env::var("ARW_WORKERS_MAX").is_err());
        assert!(std::env::var("ARW_TOOLS_CACHE_TTL_SECS").is_err());
        assert!(std::env::var("ARW_TOOLS_CACHE_CAP").is_err());
        assert!(std::env::var("ARW_OTEL").is_err());
        assert!(std::env::var("ARW_OTEL_METRICS").is_err());
    }
}
