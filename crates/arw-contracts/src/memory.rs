use crate::{
    pointer::{Pointer, PointerDomain},
    ContractError, Validate,
};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MemoryOverlays {
    #[serde(default)]
    pub claims: Vec<ClaimMemory>,
    #[serde(default)]
    pub episodic: Vec<Episode>,
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub raptor_tree: Option<RaptorTree>,
    #[serde(default)]
    pub pointers: Vec<PointerRecord>,
    #[serde(default)]
    pub ocr_map: Vec<OcrBundle>,
    #[serde(default)]
    pub metadata: Option<MemoryMetadata>,
}

impl Validate for MemoryOverlays {
    fn validate(&self) -> Result<(), ContractError> {
        for claim in &self.claims {
            claim.validate()?;
        }
        for episode in &self.episodic {
            episode.validate()?;
        }
        for skill in &self.skills {
            skill.validate()?;
        }
        if let Some(tree) = &self.raptor_tree {
            tree.validate()?;
        }
        for ptr in &self.pointers {
            ptr.validate()?;
        }
        for bundle in &self.ocr_map {
            bundle.validate()?;
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PointerRecord {
    pub pointer: String,
    pub domain: PointerDomain,
    #[serde(default)]
    pub consent: Option<PointerConsent>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub created_at: Option<String>,
}

impl Validate for PointerRecord {
    fn validate(&self) -> Result<(), ContractError> {
        Pointer::parse(&self.pointer)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PointerConsent {
    Private,
    Shared,
    Public,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClaimMemory {
    pub claim_id: String,
    pub summary: String,
    pub evidence: Vec<String>,
    #[serde(default)]
    pub priority: Option<u8>,
    #[serde(default)]
    pub last_refreshed_at: Option<String>,
}

impl Validate for ClaimMemory {
    fn validate(&self) -> Result<(), ContractError> {
        if self.summary.trim().is_empty() {
            return Err(ContractError::AssertionFailed("claim summary empty"));
        }
        if self.evidence.is_empty() {
            return Err(ContractError::AssertionFailed("claim evidence empty"));
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Episode {
    pub id: String,
    pub timestamp: String,
    pub summary: String,
    #[serde(default)]
    pub reflections: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}

impl Validate for Episode {
    fn validate(&self) -> Result<(), ContractError> {
        if self.summary.trim().is_empty() {
            return Err(ContractError::AssertionFailed("episode summary empty"));
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub practice_count: Option<u32>,
    #[serde(default)]
    pub success_rate: Option<f32>,
    #[serde(default)]
    pub last_practiced_at: Option<String>,
}

impl Validate for Skill {
    fn validate(&self) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::AssertionFailed("skill name empty"));
        }
        if let Some(rate) = self.success_rate {
            if !(0.0..=1.0).contains(&rate) {
                return Err(ContractError::AssertionFailed("success rate out of range"));
            }
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RaptorTree {
    pub fanout: u8,
    pub depth: u8,
    pub root: SummaryNode,
}

impl Validate for RaptorTree {
    fn validate(&self) -> Result<(), ContractError> {
        if !(2..=64).contains(&self.fanout) {
            return Err(ContractError::AssertionFailed("fanout out of range"));
        }
        if !(1..=8).contains(&self.depth) {
            return Err(ContractError::AssertionFailed("depth out of range"));
        }
        self.root.validate()
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SummaryNode {
    pub id: String,
    pub level: u8,
    pub summary: String,
    #[serde(default)]
    pub children: Vec<SummaryNode>,
    #[serde(default)]
    pub pointers: Vec<String>,
}

impl Validate for SummaryNode {
    fn validate(&self) -> Result<(), ContractError> {
        if self.summary.trim().is_empty() {
            return Err(ContractError::AssertionFailed("summary node missing text"));
        }
        for child in &self.children {
            child.validate()?;
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrBundle {
    pub id: String,
    pub pages: Vec<OcrPage>,
    pub text_format: OcrTextFormat,
    pub mrc_ratio: f32,
    #[serde(default)]
    pub forbid_lossy_jbig2: Option<bool>,
}

impl Validate for OcrBundle {
    fn validate(&self) -> Result<(), ContractError> {
        if !(0.0..=1.0).contains(&self.mrc_ratio) {
            return Err(ContractError::AssertionFailed("mrc_ratio out of range"));
        }
        for page in &self.pages {
            page.validate()?;
        }
        Ok(())
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OcrPage {
    pub index: u16,
    pub text_pointer: String,
    #[serde(default)]
    pub image_pointer: Option<String>,
    #[serde(default)]
    pub cer: Option<f32>,
}

impl Validate for OcrPage {
    fn validate(&self) -> Result<(), ContractError> {
        Pointer::parse(&self.text_pointer)?;
        if let Some(image) = &self.image_pointer {
            Pointer::parse(image)?;
        }
        if let Some(cer) = self.cer {
            if !(0.0..=1.0).contains(&cer) {
                return Err(ContractError::AssertionFailed("cer out of range"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OcrTextFormat {
    Hocr,
    Alto,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryMetadata {
    #[serde(default)]
    pub consent_tags: Vec<String>,
    #[serde(default)]
    pub retention_days: Option<u32>,
    #[serde(default)]
    pub last_compacted_at: Option<String>,
}
