use crate::{ContractError, Validate};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EconomyContract {
    pub autonomy: AutonomyLevel,
    pub allowed_tools: Vec<ToolAllowance>,
    #[serde(default)]
    pub spending: Option<SpendingCaps>,
    pub payout: PayoutPolicy,
    #[serde(default)]
    pub compliance: Option<ComplianceEnvelope>,
    #[serde(default)]
    pub audit: Option<AuditPolicy>,
}

impl Validate for EconomyContract {
    fn validate(&self) -> Result<(), ContractError> {
        for tool in &self.allowed_tools {
            tool.validate()?;
        }
        if self.autonomy == AutonomyLevel::Full && self.allowed_tools.is_empty() {
            return Err(ContractError::AssertionFailed(
                "full autonomy requires declared tools",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyLevel {
    HumanInTheLoop,
    Guarded,
    Full,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAllowance {
    pub id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub limits_per_hour: Option<u32>,
}

impl Validate for ToolAllowance {
    fn validate(&self) -> Result<(), ContractError> {
        if self.id.trim().is_empty() {
            return Err(ContractError::AssertionFailed("tool id missing"));
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpendingCaps {
    pub currency: Option<String>,
    #[serde(default)]
    pub soft_limit: Option<f64>,
    #[serde(default)]
    pub hard_limit: Option<f64>,
    #[serde(default)]
    pub require_human_approval_over: Option<f64>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PayoutPolicy {
    pub mode: PayoutMode,
    #[serde(default)]
    pub ledger: Option<String>,
    #[serde(default)]
    pub stakeholders: Vec<StakeholderShare>,
    #[serde(default)]
    pub escrow: Option<EscrowConfig>,
}

impl Validate for PayoutPolicy {
    fn validate(&self) -> Result<(), ContractError> {
        if let Some(stakeholders) = self
            .stakeholders
            .iter()
            .map(|s| s.share)
            .reduce(|a, b| a + b)
        {
            if stakeholders > 1.0001 {
                return Err(ContractError::AssertionFailed(
                    "stakeholder shares exceed 100%",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PayoutMode {
    None,
    LocalLedger,
    ExternalAdapter,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StakeholderShare {
    pub id: String,
    pub share: f32,
    #[serde(default)]
    pub role: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EscrowConfig {
    pub provider: Option<String>,
    pub release_policy: Option<EscrowReleasePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EscrowReleasePolicy {
    Manual,
    Milestone,
    Auto,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComplianceEnvelope {
    #[serde(default)]
    pub kyb_required: Option<bool>,
    #[serde(default)]
    pub kyc_required: Option<bool>,
    #[serde(default)]
    pub tax_responsibility: Option<TaxResponsibility>,
    #[serde(default)]
    pub jurisdictions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TaxResponsibility {
    Operator,
    User,
    Split,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditPolicy {
    #[serde(default)]
    pub log_actions: Option<bool>,
    #[serde(default)]
    pub retain_days: Option<u32>,
    #[serde(default)]
    pub redact_pii: Option<bool>,
}
