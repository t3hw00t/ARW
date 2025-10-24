use std::time::Duration;

use crate::capability::{CapabilityProfile, GpuKind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextCapabilityTier {
    Lite,
    Balanced,
    Performance,
}

impl ContextCapabilityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextCapabilityTier::Lite => "lite",
            ContextCapabilityTier::Balanced => "balanced",
            ContextCapabilityTier::Performance => "performance",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ContextCapabilityPlan {
    pub tier: ContextCapabilityTier,
    pub default_limit: usize,
    pub max_limit: usize,
    pub default_expand_per_seed: usize,
    pub max_expand_per_seed: usize,
    pub max_iterations: usize,
    pub fetch_cap: usize,
    pub health_interval: Duration,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PlanApplicationHints {
    pub apply_default_limit: bool,
    pub apply_default_expand: bool,
}

pub fn plan_for_profile(profile: &CapabilityProfile) -> ContextCapabilityPlan {
    let tier = classify_profile(profile);
    build_plan_for_tier(tier)
}

pub fn apply_plan_to_spec(
    spec: &mut crate::working_set::WorkingSetSpec,
    plan: &ContextCapabilityPlan,
    hints: PlanApplicationHints,
) {
    if hints.apply_default_limit {
        spec.limit = plan.default_limit;
    }
    if hints.apply_default_expand {
        spec.expand_per_seed = plan.default_expand_per_seed;
    }

    spec.limit = spec.limit.min(plan.max_limit).max(1);
    spec.expand_per_seed = spec.expand_per_seed.min(plan.max_expand_per_seed).max(1);
}

pub fn clamp_spec_to_plan(
    spec: &mut crate::working_set::WorkingSetSpec,
    plan: &ContextCapabilityPlan,
) {
    spec.limit = spec.limit.min(plan.max_limit).max(1);
    spec.expand_per_seed = spec.expand_per_seed.min(plan.max_expand_per_seed).max(1);
}

fn classify_profile(profile: &CapabilityProfile) -> ContextCapabilityTier {
    let low_power =
        profile.low_power_hint || profile.available_mem_mb < 4096 || profile.logical_cpus <= 4;
    if low_power || profile.total_mem_mb < 8192 {
        return ContextCapabilityTier::Lite;
    }

    let performance_grade = profile.total_mem_mb >= 32768
        || profile.available_mem_mb >= 24576
        || profile.logical_cpus >= 16
        || matches!(profile.gpu_kind, GpuKind::Dedicated);
    if performance_grade {
        return ContextCapabilityTier::Performance;
    }

    ContextCapabilityTier::Balanced
}

fn build_plan_for_tier(tier: ContextCapabilityTier) -> ContextCapabilityPlan {
    match tier {
        ContextCapabilityTier::Lite => ContextCapabilityPlan {
            tier,
            default_limit: 12,
            max_limit: 20,
            default_expand_per_seed: 2,
            max_expand_per_seed: 6,
            max_iterations: 3,
            fetch_cap: 72,
            health_interval: Duration::from_millis(7_000),
        },
        ContextCapabilityTier::Balanced => ContextCapabilityPlan {
            tier,
            default_limit: 18,
            max_limit: 32,
            default_expand_per_seed: 3,
            max_expand_per_seed: 8,
            max_iterations: 5,
            fetch_cap: 128,
            health_interval: Duration::from_millis(5_000),
        },
        ContextCapabilityTier::Performance => ContextCapabilityPlan {
            tier,
            default_limit: 24,
            max_limit: 48,
            default_expand_per_seed: 4,
            max_expand_per_seed: 12,
            max_iterations: 6,
            fetch_cap: 192,
            health_interval: Duration::from_millis(3_000),
        },
    }
}
