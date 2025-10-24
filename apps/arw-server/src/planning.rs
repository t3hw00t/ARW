use std::collections::HashSet;

use arw_contracts::{
    CompressionMode, ContractError, MemoryOverlays, PlanRequest, PlanResult, PolicySurface,
    Validate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Planner {
    default_pointer_depth: u8,
    default_pointer_fanout: u16,
}

impl Planner {
    pub fn new() -> Self {
        Self {
            default_pointer_depth: 8,
            default_pointer_fanout: 128,
        }
    }

    #[allow(dead_code)]
    pub fn with_limits(mut self, depth: u8, fanout: u16) -> Self {
        self.default_pointer_depth = depth;
        self.default_pointer_fanout = fanout;
        self
    }

    pub fn plan(&self, request: PlanRequest) -> Result<PlanResponse, PlannerError> {
        request.validate()?;
        let policy = request.policy.clone();
        let mut notes = Vec::new();

        let mut modes = dedupe_modes(policy.compression.modes.clone());
        if modes.is_empty() {
            modes.push(CompressionMode::Transclude);
            notes.push("no compression modes configured; defaulted to transclusion".to_string());
        }
        if policy.runtime.low_spec_profile.unwrap_or(false)
            && !modes.contains(&CompressionMode::Delta)
        {
            modes.push(CompressionMode::Delta);
            notes.push("low-spec profile enabled; forcing delta prompts".into());
        }

        let mut target_tokens = policy.compression.target_tokens;
        if let Some(ctx) = policy.runtime.context_tokens {
            target_tokens = target_tokens.min(ctx);
            notes.push(format!(
                "context window capped target tokens at {target_tokens}"
            ));
        }

        if let Some(memory) = &request.memory {
            enforce_pointer_limits(
                memory,
                &policy,
                self.default_pointer_depth,
                self.default_pointer_fanout,
                &mut notes,
            )?;
        }

        let plan = PlanResult {
            applied_modes: modes,
            target_tokens,
            guard_failures: None,
            planner_notes: notes.clone(),
            runtime: policy.runtime.clone(),
        };
        plan.validate()?;

        Ok(PlanResponse {
            plan,
            policy,
            memory: request.memory,
        })
    }
}

fn dedupe_modes(modes: Vec<CompressionMode>) -> Vec<CompressionMode> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(modes.len());
    for mode in modes {
        if seen.insert(mode.clone()) {
            deduped.push(mode);
        }
    }
    deduped
}

fn enforce_pointer_limits(
    memory: &MemoryOverlays,
    policy: &PolicySurface,
    default_depth: u8,
    default_fanout: u16,
    notes: &mut Vec<String>,
) -> Result<(), PlannerError> {
    let depth_limit = policy.security.pointer_depth_limit.unwrap_or(default_depth);
    let fanout_limit = policy
        .security
        .pointer_fanout_limit
        .unwrap_or(default_fanout);
    let pointer_count = memory.pointers.len() as u16;
    if pointer_count > fanout_limit {
        notes.push(format!(
            "pointer fan-out {} exceeds limit {}; downstream expansion should bucketise",
            pointer_count, fanout_limit
        ));
    }
    if depth_limit == 0 {
        return Err(PlannerError::Contract(ContractError::AssertionFailed(
            "pointer depth limit may not be zero",
        )));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlanResponse {
    pub plan: PlanResult,
    pub policy: PolicySurface,
    #[serde(default)]
    pub memory: Option<MemoryOverlays>,
}

impl PlanResponse {
    #[allow(dead_code)]
    pub fn metrics_snapshot(&self) -> PlanSnapshot {
        PlanSnapshot {
            target_tokens: self.plan.target_tokens,
            applied_modes: self.plan.applied_modes.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct PlanSnapshot {
    pub target_tokens: u32,
    pub applied_modes: Vec<CompressionMode>,
}

#[derive(thiserror::Error, Debug)]
pub enum PlannerError {
    #[error(transparent)]
    Contract(#[from] ContractError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_contracts::{
        AffectContract, AffectGuardrails, AffectInputs, AffectOutputs, AffectProsodyMode,
        AffectRationaleMode, AutonomyLevel, CompressionGuards, CompressionMode, CompressionPolicy,
        EconomyContract, HashAlgorithm, MemoryOverlays, MetricsPolicy, PayoutMode, PayoutPolicy,
        Persona, PlanRequest, PolicySurface, RemoteMode, RuntimeEngine, RuntimePolicy,
        SecurityPolicy, StateDimension, StateDimensionKind, ToneBounds, ToneHumor, ToneLevel,
        ToolAllowance, TraitDescriptor, ValueStatement, Worldview,
    };

    fn sample_persona() -> Persona {
        Persona {
            id: "persona:test".into(),
            name: "Test Persona".into(),
            aliases: vec![],
            disclose: true,
            values: vec![ValueStatement {
                id: "value:focus".into(),
                statement: "Stay focused".into(),
                priority: Some(50),
                tags: vec![],
            }],
            traits: vec![TraitDescriptor {
                id: "trait:curious".into(),
                label: "curious".into(),
                confidence: 0.8,
                provenance: vec![],
                last_updated: None,
            }],
            tone_bounds: Some(ToneBounds {
                politeness: Some(ToneLevel::High),
                formality: Some(ToneLevel::Neutral),
                humor: Some(ToneHumor::Light),
                constraints: vec![],
            }),
            boundaries: None,
            attribution: None,
        }
    }

    fn sample_worldview() -> Worldview {
        Worldview {
            claims: vec![],
            relationships: vec![],
            refresh_days: 30,
            require_citations: false,
            last_audited_at: None,
            notes: vec![],
        }
    }

    fn sample_affect() -> AffectContract {
        AffectContract {
            inputs: AffectInputs {
                text: true,
                prosody: Some(AffectProsodyMode::Off),
                physiological: None,
                custom_channels: vec![],
            },
            detectors: vec![],
            outputs: AffectOutputs {
                state_vector: vec![StateDimension {
                    dimension: StateDimensionKind::Valence,
                    value: 0.0,
                    confidence: Some(0.9),
                    detector: None,
                }],
                rationale: AffectRationaleMode::Short,
                explanations: vec![],
            },
            guardrails: Some(AffectGuardrails {
                consent_required: Some(true),
                redaction_policy: None,
                retention_seconds: Some(3600),
                explainability_required: Some(true),
            }),
        }
    }

    fn sample_economy() -> EconomyContract {
        EconomyContract {
            autonomy: AutonomyLevel::Guarded,
            allowed_tools: vec![ToolAllowance {
                id: "tool:test".into(),
                capabilities: vec!["read".into()],
                limits_per_hour: Some(10),
            }],
            spending: None,
            payout: PayoutPolicy {
                mode: PayoutMode::None,
                ledger: None,
                stakeholders: vec![],
                escrow: None,
            },
            compliance: None,
            audit: None,
        }
    }

    fn sample_policy() -> PolicySurface {
        PolicySurface {
            compression: CompressionPolicy {
                target_tokens: 2048,
                modes: vec![CompressionMode::Transclude],
                entropy_gate: Some(0.25),
                guards: CompressionGuards {
                    enabled: true,
                    require_asserts: Some(true),
                    max_guard_failures: Some(1),
                },
            },
            persona: sample_persona(),
            worldview: sample_worldview(),
            affect: sample_affect(),
            runtime: RuntimePolicy {
                engine: RuntimeEngine::LlamaCpp,
                kv_policy: arw_contracts::KvPolicy::None,
                speculative: false,
                max_batch: Some(1),
                context_tokens: Some(4096),
                low_spec_profile: Some(true),
            },
            economy: sample_economy(),
            security: SecurityPolicy {
                consent_gate: true,
                hash_algo: HashAlgorithm::Sha256,
                remote_mode: RemoteMode::PointersOnly,
                pointer_depth_limit: Some(8),
                pointer_fanout_limit: Some(64),
                prompt_injection_wrappers: Some(true),
            },
            metrics: MetricsPolicy {
                enabled: true,
                labels: vec!["tenant".into()],
                sample_percent: Some(1.0),
            },
        }
    }

    #[test]
    fn planner_applies_delta_for_low_spec() {
        let planner = Planner::new();
        let request = PlanRequest {
            policy: sample_policy(),
            memory: None,
        };
        let response = planner.plan(request).expect("plan ok");
        assert!(response
            .plan
            .applied_modes
            .contains(&CompressionMode::Delta));
        assert_eq!(response.plan.target_tokens, 2048);
    }

    #[test]
    fn planner_notes_pointer_overflow() {
        let planner = Planner::new();
        let mut policy = sample_policy();
        policy.security.pointer_fanout_limit = Some(1);
        let memory = MemoryOverlays {
            pointers: vec![
                arw_contracts::PointerRecord {
                    pointer: "<@blob:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef>".into(),
                    domain: arw_contracts::PointerDomain::Blob,
                    consent: None,
                    hash: None,
                    bytes: None,
                    created_at: None,
                },
                arw_contracts::PointerRecord {
                    pointer: "<@claim:some-claim>".into(),
                    domain: arw_contracts::PointerDomain::Claim,
                    consent: None,
                    hash: None,
                    bytes: None,
                    created_at: None,
                },
            ],
            ..Default::default()
        };
        let request = PlanRequest {
            policy,
            memory: Some(memory),
        };
        let response = planner.plan(request).expect("plan ok");
        assert!(response
            .plan
            .planner_notes
            .iter()
            .any(|note| note.contains("pointer fan-out")));
    }
}
