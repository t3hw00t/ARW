use crate::{ContractError, Validate};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Worldview {
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    pub refresh_days: u16,
    pub require_citations: bool,
    #[serde(default)]
    pub last_audited_at: Option<String>,
    #[serde(default)]
    pub notes: Vec<AuditNote>,
}

impl Validate for Worldview {
    fn validate(&self) -> Result<(), ContractError> {
        if !(1..=365).contains(&self.refresh_days) {
            return Err(ContractError::AssertionFailed(
                "worldview.refresh_days must be between 1 and 365",
            ));
        }
        if self.require_citations {
            for claim in &self.claims {
                if claim.sources.is_empty() {
                    return Err(ContractError::AssertionFailed(
                        "claim missing citations while require_citations=true",
                    ));
                }
            }
        }
        for relation in &self.relationships {
            relation.validate()?;
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Claim {
    pub id: String,
    pub statement: String,
    pub confidence: f32,
    #[serde(default)]
    pub last_checked_at: Option<String>,
    #[serde(default)]
    pub sources: Vec<ClaimSource>,
    #[serde(default)]
    pub policy_tags: Vec<String>,
}

impl Validate for Claim {
    fn validate(&self) -> Result<(), ContractError> {
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(ContractError::AssertionFailed(
                "claim confidence out of range",
            ));
        }
        if self.id.trim().is_empty() || self.statement.trim().is_empty() {
            return Err(ContractError::AssertionFailed(
                "claim id/statement required",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimSource {
    pub pointer: String,
    #[serde(default)]
    pub kind: Option<ClaimSourceKind>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClaimSourceKind {
    #[serde(rename = "primary")]
    Primary,
    #[serde(rename = "secondary")]
    Secondary,
    #[serde(rename = "anecdotal")]
    Anecdotal,
    #[serde(rename = "simulated")]
    Simulated,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Relationship {
    pub source: String,
    pub target: String,
    pub relation: String,
    #[serde(default)]
    pub weight: Option<f32>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

impl Validate for Relationship {
    fn validate(&self) -> Result<(), ContractError> {
        if self.source == self.target {
            return Err(ContractError::AssertionFailed(
                "relationship cannot target itself",
            ));
        }
        if let Some(weight) = self.weight {
            if !(-1.0..=1.0).contains(&weight) {
                return Err(ContractError::AssertionFailed(
                    "relationship weight out of range",
                ));
            }
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditNote {
    pub timestamp: String,
    pub message: String,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
