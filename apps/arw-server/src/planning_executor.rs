use arw_compress::{KvMethod, KvPolicy as RuntimeKvPolicy};
use arw_contracts::{CompressionMode, KvPolicy as ContractKvPolicy, RuntimeEngine};
use serde::Serialize;
use tracing::warn;

use crate::{metrics::PlanMetricsSample, planning::PlanResponse, AppState};

#[derive(Clone, Serialize, serde::Deserialize)]
pub struct PlanApplicationReport {
    pub kv_policy: Option<String>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct PlanExecutor;

impl PlanExecutor {
    pub async fn apply(state: &AppState, plan: &PlanResponse) -> PlanApplicationReport {
        let mut notes = Vec::new();
        let mut warnings = Vec::new();
        let kv_label = match map_kv_policy(&plan.plan.runtime.kv_policy) {
            KvAction::Apply(policy, label) => {
                let compression = state.compression();
                match compression.set_kv_policy(policy).await {
                    Ok(applied) => {
                        notes.push(format!(
                            "kv cache policy set to {}",
                            kv_method_name(applied.method)
                        ));
                        Some(label)
                    }
                    Err(err) => {
                        warn!(
                            target: "arw::planning",
                            error = %err,
                            "failed to apply kv cache policy"
                        );
                        warnings.push(format!("kv policy apply failed: {}", err));
                        None
                    }
                }
            }
            KvAction::Skip(label, reason) => {
                notes.push(reason);
                Some(label)
            }
        };

        state
            .metrics()
            .record_plan_sample(build_metrics_sample(plan, kv_label.as_deref()));

        PlanApplicationReport {
            kv_policy: kv_label,
            notes,
            warnings,
        }
    }

    pub fn record_plan_metrics(state: &AppState, plan: &PlanResponse) {
        state
            .metrics()
            .record_plan_sample(build_metrics_sample(plan, None));
    }
}

enum KvAction {
    Apply(RuntimeKvPolicy, String),
    Skip(String, String),
}

fn map_kv_policy(policy: &ContractKvPolicy) -> KvAction {
    match policy {
        ContractKvPolicy::None => KvAction::Apply(RuntimeKvPolicy::default(), "none".into()),
        ContractKvPolicy::Anchors => KvAction::Skip(
            "anchors".into(),
            "kv anchors policy handled by runtime engine; no cache compression applied".into(),
        ),
        ContractKvPolicy::TwoBit => {
            let mut kv = RuntimeKvPolicy::with_method(KvMethod::Kivi2Bit);
            kv.bits = Some(2);
            KvAction::Apply(kv, "2bit".into())
        }
        ContractKvPolicy::Snapkv => {
            let kv = RuntimeKvPolicy::with_method(KvMethod::SnapKv);
            KvAction::Apply(kv, "snapkv".into())
        }
        ContractKvPolicy::Cachegen => {
            let kv = RuntimeKvPolicy::with_method(KvMethod::CacheGen);
            KvAction::Apply(kv, "cachegen".into())
        }
    }
}

fn compression_mode_name(mode: &CompressionMode) -> &'static str {
    match mode {
        CompressionMode::Transclude => "transclude",
        CompressionMode::Delta => "delta",
        CompressionMode::Sigil => "sigil",
        CompressionMode::Ocr => "ocr",
        CompressionMode::Graph => "graph",
        CompressionMode::Claims => "claims",
    }
}

fn runtime_engine_name(engine: &RuntimeEngine) -> &'static str {
    match engine {
        RuntimeEngine::LlamaCpp => "llama.cpp",
        RuntimeEngine::Ollama => "ollama",
        RuntimeEngine::Vllm => "vllm",
        RuntimeEngine::TensorrtLlm => "tensorrt-llm",
        RuntimeEngine::Remote => "remote",
    }
}

fn kv_method_name(method: KvMethod) -> &'static str {
    match method {
        KvMethod::None => "none",
        KvMethod::SnapKv => "snapkv",
        KvMethod::Kivi2Bit => "2bit",
        KvMethod::CacheGen => "cachegen",
    }
}

fn build_metrics_sample(plan: &PlanResponse, kv_policy: Option<&str>) -> PlanMetricsSample {
    PlanMetricsSample {
        target_tokens: plan.plan.target_tokens,
        engine: runtime_engine_name(&plan.plan.runtime.engine).to_string(),
        applied_modes: plan
            .plan
            .applied_modes
            .iter()
            .map(|mode| compression_mode_name(mode).to_string())
            .collect(),
        kv_policy: kv_policy.map(|label| label.to_string()),
        guard_failures: plan.plan.guard_failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::contracts;
    use arw_contracts::{KvPolicy, PlanResult, PolicySurface};

    fn sample_policy() -> PolicySurface {
        contracts::sample_policy_surface()
    }

    fn sample_plan_response() -> PlanResponse {
        let policy = sample_policy();
        PlanResponse {
            plan: PlanResult {
                applied_modes: vec![CompressionMode::Transclude, CompressionMode::Delta],
                target_tokens: 2048,
                guard_failures: Some(2),
                planner_notes: vec![],
                runtime: policy.runtime.clone(),
            },
            policy,
            memory: None,
        }
    }

    #[test]
    fn map_kv_policy_handles_each_variant() {
        match map_kv_policy(&KvPolicy::None) {
            KvAction::Apply(policy, label) => {
                assert_eq!(policy.method, KvMethod::None);
                assert_eq!(label, "none");
            }
            KvAction::Skip(_, _) => panic!("expected apply for none"),
        }

        match map_kv_policy(&KvPolicy::Anchors) {
            KvAction::Skip(label, reason) => {
                assert_eq!(label, "anchors");
                assert!(
                    reason.contains("no cache compression"),
                    "unexpected reason: {reason}"
                );
            }
            KvAction::Apply(_, _) => panic!("anchors should skip"),
        }

        match map_kv_policy(&KvPolicy::TwoBit) {
            KvAction::Apply(policy, label) => {
                assert_eq!(policy.method, KvMethod::Kivi2Bit);
                assert_eq!(policy.bits, Some(2));
                assert_eq!(label, "2bit");
            }
            KvAction::Skip(_, _) => panic!("2bit should apply"),
        }

        match map_kv_policy(&KvPolicy::Snapkv) {
            KvAction::Apply(policy, label) => {
                assert_eq!(policy.method, KvMethod::SnapKv);
                assert_eq!(label, "snapkv");
            }
            KvAction::Skip(_, _) => panic!("snapkv should apply"),
        }

        match map_kv_policy(&KvPolicy::Cachegen) {
            KvAction::Apply(policy, label) => {
                assert_eq!(policy.method, KvMethod::CacheGen);
                assert_eq!(label, "cachegen");
            }
            KvAction::Skip(_, _) => panic!("cachegen should apply"),
        }
    }

    #[test]
    fn metrics_sample_reflects_plan_result() {
        let plan = sample_plan_response();
        let sample = build_metrics_sample(&plan, Some("snapkv"));

        assert_eq!(sample.target_tokens, 2048);
        assert_eq!(sample.engine, "llama.cpp");
        assert_eq!(sample.applied_modes.len(), 2);
        assert_eq!(sample.kv_policy.as_deref(), Some("snapkv"));
        assert_eq!(sample.guard_failures, Some(2));
        assert!(sample.applied_modes.iter().any(|m| m == "transclude"));
        assert!(sample.applied_modes.iter().any(|m| m == "delta"));
    }

    #[test]
    fn record_plan_metrics_uses_default_kv_label() {
        let plan = sample_plan_response();
        let sample = build_metrics_sample(&plan, None);

        assert!(sample.kv_policy.is_none());
    }
}
