use crate::{ContractError, Validate};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Persona {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub disclose: bool,
    #[serde(default)]
    pub values: Vec<ValueStatement>,
    pub traits: Vec<TraitDescriptor>,
    #[serde(default)]
    pub tone_bounds: Option<ToneBounds>,
    #[serde(default)]
    pub boundaries: Option<Boundaries>,
    #[serde(default)]
    pub attribution: Option<Attribution>,
}

impl Validate for Persona {
    fn validate(&self) -> Result<(), ContractError> {
        if self.id.trim().is_empty() {
            return Err(ContractError::AssertionFailed("persona.id required"));
        }
        if self.traits.is_empty() {
            return Err(ContractError::AssertionFailed(
                "persona.traits must not be empty",
            ));
        }
        for value in &self.values {
            value.validate()?;
        }
        for t in &self.traits {
            t.validate()?;
        }
        if let Some(bounds) = &self.boundaries {
            bounds.validate()?;
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValueStatement {
    pub id: String,
    pub statement: String,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Validate for ValueStatement {
    fn validate(&self) -> Result<(), ContractError> {
        if self.id.trim().is_empty() || self.statement.trim().is_empty() {
            return Err(ContractError::AssertionFailed("value statement invalid"));
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraitDescriptor {
    pub id: String,
    pub label: String,
    pub confidence: f32,
    #[serde(default)]
    pub provenance: Vec<String>,
    #[serde(default)]
    pub last_updated: Option<String>,
}

impl Validate for TraitDescriptor {
    fn validate(&self) -> Result<(), ContractError> {
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(ContractError::AssertionFailed(
                "trait confidence out of range",
            ));
        }
        if self.id.trim().is_empty() || self.label.trim().is_empty() {
            return Err(ContractError::AssertionFailed("trait id/label required"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToneBounds {
    #[serde(default)]
    pub politeness: Option<ToneLevel>,
    #[serde(default)]
    pub formality: Option<ToneLevel>,
    #[serde(default)]
    pub humor: Option<ToneHumor>,
    #[serde(default)]
    pub constraints: Vec<String>,
}

impl Validate for ToneBounds {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(level) = &self.politeness {
            level.validate()?;
        }
        if let Some(level) = &self.formality {
            level.validate()?;
        }
        if let Some(level) = &self.humor {
            level.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToneLevel {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "neutral")]
    Neutral,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "casual")]
    Casual,
    #[serde(rename = "formal")]
    Formal,
}

impl ToneLevel {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToneHumor {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "playful")]
    Playful,
}

impl ToneHumor {
    fn validate(&self) -> Result<(), ContractError> {
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Boundaries {
    #[serde(default)]
    pub topics_blocked: Vec<String>,
    #[serde(default)]
    pub safe_words: Vec<String>,
    #[serde(default)]
    pub disallowed_behaviors: Vec<String>,
}

impl Validate for Boundaries {
    fn validate(&self) -> Result<(), ContractError> {
        if self
            .safe_words
            .iter()
            .any(|sw| sw.trim().is_empty() || sw.len() > 64)
        {
            return Err(ContractError::AssertionFailed("invalid safe word"));
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attribution {
    pub created_by: Option<String>,
    pub created_at: Option<String>,
    pub last_modified_by: Option<String>,
    pub last_modified_at: Option<String>,
    #[serde(default)]
    pub consent_tags: Vec<String>,
}
