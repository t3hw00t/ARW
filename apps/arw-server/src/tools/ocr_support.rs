use std::env;

pub use crate::capability::{CapabilityProfile, GpuKind};
use crate::capability::{CapabilityService, LOW_POWER_ENV_VARS};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const BACKEND_ENV_VARS: &[&str] = &["ARW_OCR_BACKEND", "ARW_VISION_BACKEND"];
const QUALITY_ENV_VARS: &[&str] = &["ARW_OCR_QUALITY", "ARW_VISION_QUALITY"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    LegacyTesseract,
    VisionCompression,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::LegacyTesseract => "legacy",
            BackendKind::VisionCompression => "vision_compression",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "legacy" | "tesseract" | "classic" => Some(BackendKind::LegacyTesseract),
            "compression" | "vision" | "vision_compression" | "vlm" => {
                Some(BackendKind::VisionCompression)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QualityTier {
    Lite,
    Balanced,
    Full,
}

impl QualityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            QualityTier::Lite => "lite",
            QualityTier::Balanced => "balanced",
            QualityTier::Full => "full",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "lite" | "low" | "minimal" => Some(QualityTier::Lite),
            "balanced" | "medium" | "mid" => Some(QualityTier::Balanced),
            "full" | "high" | "max" => Some(QualityTier::Full),
            _ => None,
        }
    }
}

impl Default for QualityTier {
    fn default() -> Self {
        QualityTier::Balanced
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeClass {
    CpuLite,
    CpuBalanced,
    CpuIntensive,
    GpuLite,
    GpuFull,
    External,
}

impl RuntimeClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeClass::CpuLite => "cpu_lite",
            RuntimeClass::CpuBalanced => "cpu_balanced",
            RuntimeClass::CpuIntensive => "cpu_intensive",
            RuntimeClass::GpuLite => "gpu_lite",
            RuntimeClass::GpuFull => "gpu_full",
            RuntimeClass::External => "external",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub backend: BackendKind,
    pub backend_supported: bool,
    pub backend_reason: String,
    pub quality: QualityTier,
    pub quality_reason: String,
    pub runtime: RuntimeClass,
    pub runtime_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_target: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_quality: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_hint: Option<f32>,
    pub profile: CapabilityProfile,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub backend: Option<BackendKind>,
    pub quality: Option<QualityTier>,
    pub prefer_low_power: Option<bool>,
    pub force_refresh: bool,
}

impl Overrides {
    pub fn from_input(value: &Value) -> Self {
        let backend = value
            .get("backend")
            .and_then(Value::as_str)
            .and_then(BackendKind::from_str);
        let quality = value
            .get("quality")
            .and_then(Value::as_str)
            .and_then(QualityTier::from_str);
        let prefer_low_power = value.get("prefer_low_power").and_then(Value::as_bool);
        let force_refresh = value
            .get("refresh_capabilities")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        Overrides {
            backend,
            quality,
            prefer_low_power,
            force_refresh,
        }
    }
}

pub fn compute_run_metadata(
    capability: &CapabilityService,
    input_overrides: &Overrides,
) -> RunMetadata {
    let mut effective = input_overrides.clone();
    if effective.backend.is_none() {
        effective.backend = backend_override_from_env();
    }
    if effective.quality.is_none() {
        effective.quality = quality_override_from_env();
    }
    if effective.prefer_low_power.is_none() {
        effective.prefer_low_power = low_power_override_from_env();
    }

    let profile = capability.maybe_refresh(effective.force_refresh);

    let backend_sel = select_backend(
        &profile,
        effective.backend.unwrap_or(BackendKind::LegacyTesseract),
    );
    let quality_sel = select_quality(
        &profile,
        backend_sel.value,
        effective.quality.unwrap_or_default(),
        effective.prefer_low_power.unwrap_or(profile.low_power_hint),
    );
    let runtime_sel = infer_runtime_class(&profile, backend_sel.value, quality_sel.value);
    let compression_target = compression_target(backend_sel.value, quality_sel.value);
    let (expected_quality, confidence_hint) =
        quality_confidence_hint(backend_sel.value, quality_sel.value);

    RunMetadata {
        backend: backend_sel.value,
        backend_supported: backend_sel.supported,
        backend_reason: backend_sel.reason,
        quality: quality_sel.value,
        quality_reason: quality_sel.reason,
        runtime: runtime_sel.value,
        runtime_reason: runtime_sel.reason,
        compression_target,
        expected_quality,
        confidence_hint,
        profile,
    }
}

fn compression_target(backend: BackendKind, quality: QualityTier) -> Option<f32> {
    if backend != BackendKind::VisionCompression {
        return None;
    }
    let ratio = match quality {
        QualityTier::Lite => 6.0,
        QualityTier::Balanced => 10.0,
        QualityTier::Full => 14.0,
    };
    Some(ratio)
}

pub(crate) fn quality_confidence_hint(
    backend: BackendKind,
    quality: QualityTier,
) -> (Option<f32>, Option<f32>) {
    match backend {
        BackendKind::LegacyTesseract => {
            let expected = match quality {
                QualityTier::Lite => 0.94,
                QualityTier::Balanced => 0.97,
                QualityTier::Full => 0.99,
            };
            let confidence = match quality {
                QualityTier::Lite => 0.9,
                QualityTier::Balanced => 0.95,
                QualityTier::Full => 0.98,
            };
            (Some(expected), Some(confidence))
        }
        BackendKind::VisionCompression => {
            let expected = match quality {
                QualityTier::Lite => 0.82,
                QualityTier::Balanced => 0.92,
                QualityTier::Full => 0.97,
            };
            let confidence = match quality {
                QualityTier::Lite => 0.84,
                QualityTier::Balanced => 0.9,
                QualityTier::Full => 0.94,
            };
            (Some(expected), Some(confidence))
        }
    }
}

struct Selection<T> {
    value: T,
    supported: bool,
    reason: String,
}

fn select_backend(profile: &CapabilityProfile, requested: BackendKind) -> Selection<BackendKind> {
    let _ = profile;
    match requested {
        BackendKind::VisionCompression => {
            if compression_available() {
                Selection {
                    value: BackendKind::VisionCompression,
                    supported: true,
                    reason: "vision compression backend requested/available".into(),
                }
            } else if legacy_available() {
                Selection {
                    value: BackendKind::LegacyTesseract,
                    supported: true,
                    reason: "vision compression backend unavailable; falling back to legacy".into(),
                }
            } else {
                Selection {
                    value: BackendKind::VisionCompression,
                    supported: false,
                    reason: "vision compression backend unavailable and legacy not compiled".into(),
                }
            }
        }
        BackendKind::LegacyTesseract => {
            if legacy_available() {
                Selection {
                    value: BackendKind::LegacyTesseract,
                    supported: true,
                    reason: "legacy backend requested/available".into(),
                }
            } else if compression_available() {
                Selection {
                    value: BackendKind::VisionCompression,
                    supported: true,
                    reason: "legacy backend unavailable; falling back to vision compression".into(),
                }
            } else {
                Selection {
                    value: BackendKind::LegacyTesseract,
                    supported: false,
                    reason: "no OCR backend compiled in".into(),
                }
            }
        }
    }
}

fn select_quality(
    profile: &CapabilityProfile,
    backend: BackendKind,
    requested: QualityTier,
    prefer_low_power: bool,
) -> Selection<QualityTier> {
    if prefer_low_power && requested == QualityTier::Balanced {
        return Selection {
            value: QualityTier::Lite,
            supported: true,
            reason: "low-power preference requested lite quality".into(),
        };
    }

    if requested != QualityTier::Balanced {
        return Selection {
            value: requested,
            supported: true,
            reason: format!("requested {} quality tier", requested.as_str()),
        };
    }

    if prefer_low_power {
        return Selection {
            value: QualityTier::Lite,
            supported: true,
            reason: "low-power preference forcing lite quality".into(),
        };
    }

    let value = if backend == BackendKind::VisionCompression {
        match profile.gpu_vram_mb.unwrap_or(0) {
            vram if vram >= 12000 => QualityTier::Full,
            vram if vram >= 6000 => QualityTier::Balanced,
            vram if vram > 0 => QualityTier::Lite,
            _ => QualityTier::Lite,
        }
    } else if profile.total_mem_mb < 4096 || profile.logical_cpus < 4 {
        QualityTier::Lite
    } else if profile.total_mem_mb >= 12288 && profile.logical_cpus >= 8 {
        QualityTier::Full
    } else {
        QualityTier::Balanced
    };

    let reason = if backend == BackendKind::VisionCompression {
        format!(
            "vision backend heuristics for gpu_vram_mb={}",
            profile.gpu_vram_mb.unwrap_or(0)
        )
    } else {
        format!(
            "legacy backend heuristics for total_mem_mb={} logical_cpus={}",
            profile.total_mem_mb, profile.logical_cpus
        )
    };

    Selection {
        value,
        supported: true,
        reason,
    }
}

fn infer_runtime_class(
    profile: &CapabilityProfile,
    backend: BackendKind,
    quality: QualityTier,
) -> Selection<RuntimeClass> {
    let (value, reason) = match backend {
        BackendKind::LegacyTesseract => match quality {
            QualityTier::Lite => (
                RuntimeClass::CpuLite,
                "legacy backend in lite mode (minimal CPU footprint)".into(),
            ),
            QualityTier::Balanced => (
                RuntimeClass::CpuBalanced,
                "legacy backend in balanced mode".into(),
            ),
            QualityTier::Full => (
                RuntimeClass::CpuIntensive,
                "legacy backend in full quality (CPU intensive)".into(),
            ),
        },
        BackendKind::VisionCompression => match profile.gpu_kind {
            GpuKind::Dedicated => (
                if quality == QualityTier::Full {
                    RuntimeClass::GpuFull
                } else {
                    RuntimeClass::GpuLite
                },
                "vision backend using dedicated GPU".into(),
            ),
            GpuKind::Integrated => (
                RuntimeClass::GpuLite,
                "vision backend using integrated GPU".into(),
            ),
            GpuKind::None => (
                RuntimeClass::CpuBalanced,
                "vision backend falling back to CPU (no GPU detected)".into(),
            ),
        },
    };

    Selection {
        value,
        supported: true,
        reason,
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn low_power_override_from_env() -> Option<bool> {
    for var in LOW_POWER_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Some(parsed) = parse_bool(&value) {
                return Some(parsed);
            }
        }
    }
    None
}

fn backend_override_from_env() -> Option<BackendKind> {
    for var in BACKEND_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Some(parsed) = BackendKind::from_str(&value) {
                return Some(parsed);
            }
        }
    }
    None
}

fn quality_override_from_env() -> Option<QualityTier> {
    for var in QUALITY_ENV_VARS {
        if let Ok(value) = env::var(var) {
            if let Some(parsed) = QualityTier::from_str(&value) {
                return Some(parsed);
            }
        }
    }
    None
}

const fn legacy_available() -> bool {
    cfg!(feature = "ocr_tesseract")
}

const fn compression_available() -> bool {
    cfg!(feature = "ocr_compression")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with(
        total_mem_mb: u64,
        logical_cpus: u16,
        gpu_vram_mb: Option<u64>,
        gpu_kind: GpuKind,
    ) -> CapabilityProfile {
        CapabilityProfile {
            total_mem_mb,
            available_mem_mb: total_mem_mb / 2,
            logical_cpus,
            physical_cpus: logical_cpus / 2,
            gpu_vram_mb,
            gpu_kind,
            low_power_hint: false,
            low_power_hint_source: None,
            gpu_vram_source: None,
            decoder_gpus: None,
            decoder_capacity: None,
            os: "test".into(),
            collected_at: "1970-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn quality_parsing_accepts_aliases() {
        assert_eq!(QualityTier::from_str("lite"), Some(QualityTier::Lite));
        assert_eq!(QualityTier::from_str("LOW"), Some(QualityTier::Lite));
        assert_eq!(
            QualityTier::from_str("balanced"),
            Some(QualityTier::Balanced)
        );
        assert_eq!(QualityTier::from_str("High"), Some(QualityTier::Full));
        assert_eq!(QualityTier::from_str("unknown"), None);
    }

    #[test]
    fn backend_override_respects_availability() {
        let profile = profile_with(8192, 8, Some(8192), GpuKind::Dedicated);
        let selection = select_backend(&profile, BackendKind::VisionCompression);
        if compression_available() {
            assert_eq!(selection.value, BackendKind::VisionCompression);
            assert!(selection.supported);
        } else if legacy_available() {
            assert_eq!(selection.value, BackendKind::LegacyTesseract);
            assert!(selection.supported);
        } else {
            assert!(!selection.supported);
        }
    }

    #[test]
    fn low_power_prefers_lite() {
        let profile = profile_with(16384, 16, Some(16384), GpuKind::Dedicated);
        let selection = select_quality(
            &profile,
            BackendKind::LegacyTesseract,
            QualityTier::Balanced,
            true,
        );
        assert_eq!(selection.value, QualityTier::Lite);
    }

    #[test]
    fn quality_confidence_hint_behaves_for_backends() {
        let (expected, confidence) =
            quality_confidence_hint(BackendKind::VisionCompression, QualityTier::Balanced);
        assert!(expected.unwrap() > 0.9);
        assert!(confidence.unwrap() > 0.85);

        let (legacy_expected, legacy_conf) =
            quality_confidence_hint(BackendKind::LegacyTesseract, QualityTier::Full);
        assert!(legacy_expected.unwrap() >= 0.99);
        assert!(legacy_conf.unwrap() >= 0.95);
    }
}
