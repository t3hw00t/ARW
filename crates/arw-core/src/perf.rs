use sysinfo::{System, CpuRefreshKind, RefreshKind};
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
    pub context_scan_limit: usize,
    pub rehydrate_file_head_kb: u64,
    pub route_stats_coalesce_ms: u64,
    pub route_stats_publish_ms: u64,
    pub models_metrics_coalesce_ms: u64,
    pub models_metrics_publish_ms: u64,
}

impl PerfPreset {
    pub fn from_env() -> Option<Self> {
        match std::env::var("ARW_PERF_PRESET").ok().as_deref().map(|s| s.trim().to_lowercase()) {
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
    let sys = System::new_with_specifics(RefreshKind::new().with_cpu(CpuRefreshKind::new()));
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
            context_scan_limit: 120,
            rehydrate_file_head_kb: 32,
            route_stats_coalesce_ms: 350,
            route_stats_publish_ms: 2500,
            models_metrics_coalesce_ms: 350,
            models_metrics_publish_ms: 2500,
        },
        PerfPreset::Balanced => PerfTuning {
            http_max_conc: 1024,
            actions_queue_max: 1024,
            context_scan_limit: 200,
            rehydrate_file_head_kb: 64,
            route_stats_coalesce_ms: 250,
            route_stats_publish_ms: 2000,
            models_metrics_coalesce_ms: 250,
            models_metrics_publish_ms: 2000,
        },
        PerfPreset::Performance => PerfTuning {
            http_max_conc: 4096,
            actions_queue_max: 4096,
            context_scan_limit: 300,
            rehydrate_file_head_kb: 96,
            route_stats_coalesce_ms: 150,
            route_stats_publish_ms: 1500,
            models_metrics_coalesce_ms: 150,
            models_metrics_publish_ms: 1500,
        },
        PerfPreset::Turbo => PerfTuning {
            http_max_conc: 16384,
            actions_queue_max: 16384,
            context_scan_limit: 500,
            rehydrate_file_head_kb: 128,
            route_stats_coalesce_ms: 100,
            route_stats_publish_ms: 1000,
            models_metrics_coalesce_ms: 100,
            models_metrics_publish_ms: 1000,
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
    set_if_unset("ARW_CONTEXT_SCAN_LIMIT", t.context_scan_limit.to_string());
    set_if_unset("ARW_REHYDRATE_FILE_HEAD_KB", t.rehydrate_file_head_kb.to_string());
    set_if_unset("ARW_ROUTE_STATS_COALESCE_MS", t.route_stats_coalesce_ms.to_string());
    set_if_unset("ARW_ROUTE_STATS_PUBLISH_MS", t.route_stats_publish_ms.to_string());
    set_if_unset("ARW_MODELS_METRICS_COALESCE_MS", t.models_metrics_coalesce_ms.to_string());
    set_if_unset("ARW_MODELS_METRICS_PUBLISH_MS", t.models_metrics_publish_ms.to_string());

    // Provide a canonical computed tier for observability
    std::env::set_var("ARW_PERF_PRESET_TIER", match preset {
        PerfPreset::Eco => "eco",
        PerfPreset::Balanced => "balanced",
        PerfPreset::Performance => "performance",
        PerfPreset::Turbo => "turbo",
    });

    info!(
        tier = std::env::var("ARW_PERF_PRESET_TIER").unwrap_or_default().as_str(),
        http_max_conc = t.http_max_conc,
        actions_queue_max = t.actions_queue_max,
        context_scan_limit = t.context_scan_limit,
        rehydrate_file_head_kb = t.rehydrate_file_head_kb,
        "Applied performance preset defaults (env-seeded if unset)"
    );
    preset
}
