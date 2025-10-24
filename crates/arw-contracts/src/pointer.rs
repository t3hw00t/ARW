use crate::ContractError;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

static BLOB_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^<@blob:sha256:[a-f0-9]{64}(?::\d+\.\.\d+)?>$").expect("blob regex"));
static OCR_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^<@ocr:[A-Za-z0-9._-]+#p\d+(?::(line|block)=\d+)?$").expect("ocr regex")
});
static SIGIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^<@sigil:[A-Z0-9][A-Z0-9_.-]{0,63}(?::v=\d+(\.\d+){0,2})?>$").expect("sigil regex")
});
static CLAIM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^<@claim:[A-Za-z0-9._#:-]{1,128}>$").expect("claim regex"));
static GRAPH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^<@graph:[A-Za-z0-9._-]+(?:\?k=\d+(&budget=\d+)?)?>$").expect("graph regex")
});

/// Parsed pointer representation. Stores the original token alongside the detected domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pointer {
    pub raw: String,
    pub domain: PointerDomain,
}

impl Pointer {
    /// Validates and parses a pointer token. Returns a typed pointer or an error.
    pub fn parse(raw: &str) -> Result<Self, ContractError> {
        if raw.len() > 256 {
            return Err(ContractError::InvalidPointer("pointer too long".into()));
        }
        if raw.contains(char::is_whitespace) {
            return Err(ContractError::InvalidPointer(
                "whitespace not permitted in pointer".into(),
            ));
        }
        let domain = if BLOB_RE.is_match(raw) {
            PointerDomain::Blob
        } else if OCR_RE.is_match(raw) {
            PointerDomain::Ocr
        } else if SIGIL_RE.is_match(raw) {
            PointerDomain::Sigil
        } else if CLAIM_RE.is_match(raw) {
            PointerDomain::Claim
        } else if GRAPH_RE.is_match(raw) {
            PointerDomain::Graph
        } else {
            return Err(ContractError::InvalidPointer(raw.into()));
        };
        Ok(Self {
            raw: raw.to_owned(),
            domain,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PointerDomain {
    Blob,
    Ocr,
    Sigil,
    Claim,
    Graph,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_blob_pointer() {
        let ptr = Pointer::parse(
            "<@blob:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef>",
        )
        .unwrap();
        assert_eq!(ptr.domain, PointerDomain::Blob);
    }

    #[test]
    fn reject_invalid_pointer() {
        let err = Pointer::parse("<@blob:sha1:deadbeef>").unwrap_err();
        assert!(matches!(err, ContractError::InvalidPointer(_)));
    }
}
