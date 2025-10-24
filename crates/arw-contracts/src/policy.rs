use crate::{
    affect::AffectContract, economy::EconomyContract, memory::MemoryOverlays, persona::Persona,
    worldview::Worldview, ContractError, Validate,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicySurface {
    pub compression: CompressionPolicy,
    pub persona: Persona,
    pub worldview: Worldview,
    pub affect: AffectContract,
    pub runtime: RuntimePolicy,
    pub economy: EconomyContract,
    pub security: SecurityPolicy,
    pub metrics: MetricsPolicy,
}

impl Validate for PolicySurface {
    fn validate(&self) -> Result<(), ContractError> {
        self.persona.validate()?;
        self.worldview.validate()?;
        self.affect.validate()?;
        self.economy.validate()?;
        self.compression.validate()?;
        self.runtime.validate()?;
        self.security.validate()?;
        self.metrics.validate()?;
        Ok(())
    }
}

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressionPolicy {
    pub target_tokens: u32,
    #[serde(default)]
    pub modes: Vec<CompressionMode>,
    #[serde(default)]
    pub entropy_gate: Option<f32>,
    pub guards: CompressionGuards,
}

impl Validate for CompressionPolicy {
    fn validate(&self) -> Result<(), ContractError> {
        if !(256..=32768).contains(&self.target_tokens) {
            return Err(ContractError::AssertionFailed(
                "target_tokens must be between 256 and 32768",
            ));
        }
        if let Some(gate) = self.entropy_gate {
            if !(0.0..=1.0).contains(&gate) {
                return Err(ContractError::AssertionFailed("entropy_gate out of range"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum CompressionMode {
    Transclude,
    Delta,
    Sigil,
    Ocr,
    Graph,
    Claims,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressionGuards {
    pub enabled: bool,
    #[serde(default)]
    pub require_asserts: Option<bool>,
    #[serde(default)]
    pub max_guard_failures: Option<u8>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimePolicy {
    pub engine: RuntimeEngine,
    pub kv_policy: KvPolicy,
    pub speculative: bool,
    #[serde(default)]
    pub max_batch: Option<u8>,
    #[serde(default)]
    pub context_tokens: Option<u32>,
    #[serde(default)]
    pub low_spec_profile: Option<bool>,
}

impl Validate for RuntimePolicy {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(batch) = self.max_batch {
            if !(1..=128).contains(&batch) {
                return Err(ContractError::AssertionFailed("max_batch out of range"));
            }
        }
        if let Some(ctx) = self.context_tokens {
            if !(512..=262_144).contains(&ctx) {
                return Err(ContractError::AssertionFailed(
                    "context_tokens out of range",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeEngine {
    #[serde(rename = "llama.cpp")]
    LlamaCpp,
    Ollama,
    Vllm,
    #[serde(rename = "tensorrt-llm")]
    TensorrtLlm,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum KvPolicy {
    None,
    Anchors,
    #[serde(rename = "2bit")]
    TwoBit,
    Snapkv,
    Cachegen,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityPolicy {
    pub consent_gate: bool,
    pub hash_algo: HashAlgorithm,
    pub remote_mode: RemoteMode,
    #[serde(default)]
    pub pointer_depth_limit: Option<u8>,
    #[serde(default)]
    pub pointer_fanout_limit: Option<u16>,
    #[serde(default)]
    pub prompt_injection_wrappers: Option<bool>,
}

impl Validate for SecurityPolicy {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(depth) = self.pointer_depth_limit {
            if !(1..=32).contains(&depth) {
                return Err(ContractError::AssertionFailed(
                    "pointer depth limit out of range",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha256,
    Blake3,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteMode {
    PointersOnly,
    ExpandRemote,
    ForbidRemote,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricsPolicy {
    pub enabled: bool,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub sample_percent: Option<f32>,
}

impl Validate for MetricsPolicy {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(sample) = self.sample_percent {
            if !(0.0..=1.0).contains(&sample) {
                return Err(ContractError::AssertionFailed(
                    "metrics sample out of range",
                ));
            }
        }
        Ok(())
    }
}

/// Planner input that bundles user policy with runtime memory overlays.
#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanRequest {
    pub policy: PolicySurface,
    #[serde(default)]
    pub memory: Option<MemoryOverlays>,
}

impl Validate for PlanRequest {
    fn validate(&self) -> Result<(), ContractError> {
        self.policy.validate()?;
        if let Some(memory) = &self.memory {
            memory.validate()?;
        }
        Ok(())
    }
}

/// Planned execution result capturing applied compression settings and guardrails.
#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanResult {
    #[serde(default)]
    pub applied_modes: Vec<CompressionMode>,
    pub target_tokens: u32,
    #[serde(default)]
    pub guard_failures: Option<u8>,
    #[serde(default)]
    pub planner_notes: Vec<String>,
    pub runtime: RuntimePolicy,
}

impl Validate for PlanResult {
    fn validate(&self) -> Result<(), ContractError> {
        self.runtime.validate()?;
        if self.applied_modes.is_empty() {
            return Err(ContractError::AssertionFailed(
                "planner must apply at least one compression mode",
            ));
        }
        Ok(())
    }
}
