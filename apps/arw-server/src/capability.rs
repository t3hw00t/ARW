use std::cmp::min;
use std::env;
use std::time::{Duration, Instant};

use chrono::Utc;
use metrics::{counter, gauge};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use sysinfo::System;
use tracing::info;

const PROFILE_REFRESH_SECS: u64 = 300;

pub const LOW_POWER_ENV_VARS: &[&str] = &[
    "ARW_OCR_LOW_POWER",
    "ARW_OCR_PREFER_LOW_POWER",
    "ARW_PREFER_LOW_POWER",
    "ARW_LOW_POWER",
];

const GPU_VRAM_ENV_VARS_MB: &[&str] = &["ARW_GPU_VRAM_MB", "ARW_OCR_GPU_VRAM_MB"];
const GPU_VRAM_ENV_VARS_BYTES: &[&str] = &["ARW_GPU_VRAM_BYTES", "ARW_OCR_GPU_VRAM_BYTES"];
const DECODER_GPU_ENV_VARS: &[&str] = &["ARW_OCR_DECODER_GPUS", "ARW_DECODER_GPUS"];
const DECODER_CAPACITY_ENV_VARS: &[&str] = &["ARW_OCR_DECODER_CAPACITY", "ARW_DECODER_CAPACITY"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GpuKind {
    None,
    Integrated,
    Dedicated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityProfile {
    pub total_mem_mb: u64,
    pub available_mem_mb: u64,
    pub logical_cpus: u16,
    pub physical_cpus: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_vram_mb: Option<u64>,
    pub gpu_kind: GpuKind,
    pub low_power_hint: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_power_hint_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_vram_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_gpus: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_capacity: Option<f32>,
    pub os: String,
    pub collected_at: String,
}

impl CapabilityProfile {
    pub fn detect() -> Self {
        let mut sys = System::new_all();
        sys.refresh_memory();
        let total_mem_kb = sys.total_memory();
        let available_mem_kb = sys.available_memory();
        let total_mem_mb = total_mem_kb / 1024;
        let available_mem_mb = available_mem_kb / 1024;
        let logical_cpus = min(sys.cpus().len(), u16::MAX as usize) as u16;
        let physical_cpus = System::physical_core_count()
            .map(|cnt| min(cnt, u16::MAX as usize) as u16)
            .unwrap_or(0);
        let gpu_hints = gpu_hints_from_env();
        let gpu_vram_mb = gpu_hints.vram_mb;
        let gpu_vram_source = gpu_hints.vram_source.clone();
        let gpu_kind = match gpu_vram_mb {
            Some(vram) if vram >= 4096 => GpuKind::Dedicated,
            Some(vram) if vram > 0 => GpuKind::Integrated,
            _ => GpuKind::None,
        };
        let (low_power_hint, low_power_hint_source) = low_power_hint_from_env();

        CapabilityProfile {
            total_mem_mb,
            available_mem_mb,
            logical_cpus,
            physical_cpus,
            gpu_vram_mb,
            gpu_kind,
            low_power_hint,
            low_power_hint_source,
            gpu_vram_source,
            decoder_gpus: gpu_hints.decoder_cards,
            decoder_capacity: gpu_hints.decoder_capacity,
            os: env::consts::OS.to_string(),
            collected_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Default)]
struct GpuHints {
    vram_mb: Option<u64>,
    vram_source: Option<String>,
    decoder_cards: Option<u16>,
    decoder_capacity: Option<f32>,
}

pub struct CapabilityService {
    cache: RwLock<CapabilityProfile>,
    last_refresh: Mutex<Instant>,
}

impl CapabilityService {
    pub fn new() -> Self {
        let profile = CapabilityProfile::detect();
        Self {
            cache: RwLock::new(profile),
            last_refresh: Mutex::new(Instant::now()),
        }
    }

    #[allow(dead_code)]
    pub fn profile(&self) -> CapabilityProfile {
        self.cache.read().clone()
    }

    pub fn refresh(&self) -> CapabilityProfile {
        let fresh = CapabilityProfile::detect();
        observe_profile(&fresh, "explicit");
        *self.cache.write() = fresh.clone();
        *self.last_refresh.lock() = Instant::now();
        fresh
    }

    pub fn maybe_refresh(&self, force: bool) -> CapabilityProfile {
        if force {
            return self.refresh();
        }

        let should_refresh = {
            let guard = self.last_refresh.lock();
            guard.elapsed() >= Duration::from_secs(PROFILE_REFRESH_SECS)
        };
        if should_refresh {
            let fresh = CapabilityProfile::detect();
            observe_profile(&fresh, "interval");
            *self.cache.write() = fresh.clone();
            *self.last_refresh.lock() = Instant::now();
            return fresh;
        }

        self.cache.read().clone()
    }
}

fn gpu_hints_from_env() -> GpuHints {
    let mut hints = GpuHints::default();

    for var in GPU_VRAM_ENV_VARS_MB {
        if let Ok(value) = env::var(var) {
            if let Ok(parsed) = value.trim().parse::<u64>() {
                hints.vram_mb = Some(parsed);
                hints.vram_source = Some(format!("env:{} (mb)", var));
                break;
            }
        }
    }

    if hints.vram_mb.is_none() {
        for var in GPU_VRAM_ENV_VARS_BYTES {
            if let Ok(value) = env::var(var) {
                if let Ok(parsed) = value.trim().parse::<u64>() {
                    hints.vram_mb = Some(parsed / (1024 * 1024));
                    hints.vram_source = Some(format!("env:{} (bytes)", var));
                    break;
                }
            }
        }
    }

    for var in DECODER_GPU_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Ok(parsed) = value.trim().parse::<u16>() {
                hints.decoder_cards = Some(parsed);
                break;
            }
        }
    }

    for var in DECODER_CAPACITY_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Ok(parsed) = value.trim().parse::<f32>() {
                hints.decoder_capacity = Some(parsed);
                break;
            }
        }
    }

    hints
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn low_power_hint_from_env() -> (bool, Option<String>) {
    for var in LOW_POWER_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Some(parsed) = parse_bool(&value) {
                return (parsed, Some(format!("env:{}", var)));
            }
        }
    }
    (false, None)
}

fn observe_profile(profile: &CapabilityProfile, trigger: &'static str) {
    counter!(
        "arw_capability_refresh_total",
        "trigger" => trigger,
        "gpu_kind" => gpu_kind_label(&profile.gpu_kind),
        "low_power" => bool_label(profile.low_power_hint),
    )
    .increment(1);
    gauge!("arw_capability_total_mem_mb").set(profile.total_mem_mb as f64);
    gauge!("arw_capability_available_mem_mb").set(profile.available_mem_mb as f64);
    gauge!("arw_capability_logical_cpus").set(profile.logical_cpus as f64);
    gauge!("arw_capability_physical_cpus").set(profile.physical_cpus as f64);
    if let Some(vram) = profile.gpu_vram_mb {
        gauge!("arw_capability_gpu_vram_mb").set(vram as f64);
    }
    gauge!("arw_capability_low_power_hint").set(if profile.low_power_hint { 1.0 } else { 0.0 });
    info!(
        target = "arw::capability",
        trigger = trigger,
        total_mem_mb = profile.total_mem_mb,
        available_mem_mb = profile.available_mem_mb,
        logical_cpus = profile.logical_cpus,
        physical_cpus = profile.physical_cpus,
        gpu_kind = gpu_kind_label(&profile.gpu_kind),
        gpu_vram_mb = profile.gpu_vram_mb,
        low_power = profile.low_power_hint,
        "capability profile refreshed"
    );
}

fn gpu_kind_label(kind: &GpuKind) -> &'static str {
    match kind {
        GpuKind::None => "none",
        GpuKind::Integrated => "integrated",
        GpuKind::Dedicated => "dedicated",
    }
}

fn bool_label(flag: bool) -> &'static str {
    if flag {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_detect_sets_gpu_kind() {
        let profile = CapabilityProfile::detect();
        match profile.gpu_vram_mb {
            Some(vram) if vram >= 4096 => assert_eq!(profile.gpu_kind, GpuKind::Dedicated),
            Some(vram) if vram > 0 => assert_eq!(profile.gpu_kind, GpuKind::Integrated),
            _ => assert_eq!(profile.gpu_kind, GpuKind::None),
        }
    }

    #[test]
    fn maybe_refresh_honours_force() {
        let service = CapabilityService::new();
        let first = service.profile();
        let refreshed = service.maybe_refresh(true);
        assert_ne!(first.collected_at, refreshed.collected_at);
    }
}
