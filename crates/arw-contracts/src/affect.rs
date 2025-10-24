use crate::{ContractError, Validate};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectContract {
    pub inputs: AffectInputs,
    #[serde(default)]
    pub detectors: Vec<AffectDetector>,
    pub outputs: AffectOutputs,
    #[serde(default)]
    pub guardrails: Option<AffectGuardrails>,
}

impl Validate for AffectContract {
    fn validate(&self) -> Result<(), ContractError> {
        for detector in &self.detectors {
            detector.validate()?;
        }
        Ok(())
    }
}

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectInputs {
    #[serde(default)]
    pub text: bool,
    #[serde(default)]
    pub prosody: Option<AffectProsodyMode>,
    #[serde(default)]
    pub physiological: Option<AffectBinaryMode>,
    #[serde(default)]
    pub custom_channels: Vec<CustomChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AffectProsodyMode {
    Off,
    On,
    OptIn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AffectBinaryMode {
    Off,
    On,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomChannel {
    pub name: String,
    pub sampling_hz: f32,
    pub consent_required: bool,
    #[serde(default)]
    pub retention_seconds: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectDetector {
    pub id: String,
    pub kind: DetectorKind,
    pub version: String,
    #[serde(default)]
    pub confidence_threshold: Option<f32>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Validate for AffectDetector {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(threshold) = self.confidence_threshold {
            if !(0.0..=1.0).contains(&threshold) {
                return Err(ContractError::AssertionFailed(
                    "detector confidence threshold out of range",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DetectorKind {
    Sentiment,
    Vibe,
    Prosody,
    Custom,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectOutputs {
    pub state_vector: Vec<StateDimension>,
    pub rationale: AffectRationaleMode,
    #[serde(default)]
    pub explanations: Vec<AffectExplanation>,
}

impl Validate for AffectOutputs {
    fn validate(&self) -> Result<(), ContractError> {
        for dim in &self.state_vector {
            dim.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AffectRationaleMode {
    None,
    Short,
    Detailed,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateDimension {
    pub dimension: StateDimensionKind,
    pub value: f32,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub detector: Option<String>,
}

impl Validate for StateDimension {
    fn validate(&self) -> Result<(), ContractError> {
        if !(-1.0..=1.0).contains(&self.value) {
            return Err(ContractError::AssertionFailed(
                "state dimension value out of range",
            ));
        }
        if let Some(confidence) = self.confidence {
            if !(0.0..=1.0).contains(&confidence) {
                return Err(ContractError::AssertionFailed(
                    "state dimension confidence out of range",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StateDimensionKind {
    Valence,
    Arousal,
    Empathy,
    Trust,
    Curiosity,
    Custom,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectExplanation {
    pub timestamp: String,
    pub summary: String,
    #[serde(default)]
    pub signals: Vec<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AffectGuardrails {
    #[serde(default)]
    pub consent_required: Option<bool>,
    #[serde(default)]
    pub redaction_policy: Option<RedactionPolicy>,
    #[serde(default)]
    pub retention_seconds: Option<u32>,
    #[serde(default)]
    pub explainability_required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RedactionPolicy {
    Strip,
    Mask,
    Hash,
}
