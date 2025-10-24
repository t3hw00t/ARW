use arw_contracts::{
    AffectContract, AffectInputs, AffectOutputs, AffectProsodyMode, AffectRationaleMode,
    AutonomyLevel, CompressionGuards, CompressionMode, CompressionPolicy, EconomyContract,
    HashAlgorithm, KvPolicy, MemoryOverlays, MetricsPolicy, Persona, PlanRequest, PolicySurface,
    RemoteMode, RuntimeEngine, RuntimePolicy, SecurityPolicy, StateDimension, StateDimensionKind,
    ToolAllowance, TraitDescriptor, ValueStatement, Worldview,
};

/// Provide a lightweight persona suitable for tests that require validated contract data.
pub fn sample_persona() -> Persona {
    Persona {
        id: "persona:test-fixture".into(),
        name: "Test Persona".into(),
        aliases: vec![],
        disclose: true,
        values: vec![ValueStatement {
            id: "value:focus".into(),
            statement: "Stay focused".into(),
            priority: Some(50),
            tags: vec!["test".into()],
        }],
        traits: vec![TraitDescriptor {
            id: "trait:curious".into(),
            label: "curious".into(),
            confidence: 0.75,
            provenance: vec![],
            last_updated: None,
        }],
        tone_bounds: None,
        boundaries: None,
        attribution: None,
    }
}

/// Provide a small worldview structure that passes validation but keeps tests lightweight.
pub fn sample_worldview() -> Worldview {
    Worldview {
        claims: vec![],
        relationships: vec![],
        refresh_days: 30,
        require_citations: false,
        last_audited_at: None,
        notes: vec![],
    }
}

/// Fixture for affect contracts with a single dimension populated.
pub fn sample_affect() -> AffectContract {
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
                value: 0.25,
                confidence: Some(0.85),
                detector: Some("detector:fixture".into()),
            }],
            rationale: AffectRationaleMode::Short,
            explanations: vec![],
        },
        guardrails: None,
    }
}

/// Economy fixture with guarded autonomy and a single tool allowance.
pub fn sample_economy() -> EconomyContract {
    EconomyContract {
        autonomy: AutonomyLevel::Guarded,
        allowed_tools: vec![ToolAllowance {
            id: "tool:search".into(),
            capabilities: vec!["search".into()],
            limits_per_hour: Some(5),
        }],
        spending: None,
        payout: arw_contracts::PayoutPolicy {
            mode: arw_contracts::PayoutMode::None,
            ledger: None,
            stakeholders: vec![],
            escrow: None,
        },
        compliance: None,
        audit: None,
    }
}

/// Metrics policy fixture that enables metrics with full sampling.
pub fn sample_metrics_policy() -> MetricsPolicy {
    MetricsPolicy {
        enabled: true,
        labels: vec!["tenant".into()],
        sample_percent: Some(1.0),
    }
}

/// Minimal memory overlays fixture.
#[allow(dead_code)]
pub fn sample_memory_overlays() -> MemoryOverlays {
    MemoryOverlays::default()
}

/// PolicySurface fixture combining the individual helpers above.
pub fn sample_policy_surface() -> PolicySurface {
    PolicySurface {
        compression: CompressionPolicy {
            target_tokens: 1024,
            modes: vec![CompressionMode::Transclude],
            entropy_gate: None,
            guards: CompressionGuards {
                enabled: true,
                require_asserts: Some(true),
                max_guard_failures: None,
            },
        },
        persona: sample_persona(),
        worldview: sample_worldview(),
        affect: sample_affect(),
        runtime: RuntimePolicy {
            engine: RuntimeEngine::LlamaCpp,
            kv_policy: KvPolicy::Snapkv,
            speculative: false,
            max_batch: None,
            context_tokens: None,
            low_spec_profile: None,
        },
        economy: sample_economy(),
        security: SecurityPolicy {
            consent_gate: true,
            hash_algo: HashAlgorithm::Sha256,
            remote_mode: RemoteMode::PointersOnly,
            pointer_depth_limit: None,
            pointer_fanout_limit: None,
            prompt_injection_wrappers: None,
        },
        metrics: sample_metrics_policy(),
    }
}

/// Convenience helper to produce a PlanRequest fixture.
#[allow(dead_code)]
pub fn sample_plan_request(include_memory: bool) -> PlanRequest {
    PlanRequest {
        policy: sample_policy_surface(),
        memory: include_memory.then(sample_memory_overlays),
    }
}
