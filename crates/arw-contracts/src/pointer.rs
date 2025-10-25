use crate::ContractError;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

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
        let canonical = canonicalize_token(raw)?;
        let domain = if BLOB_RE.is_match(&canonical) {
            PointerDomain::Blob
        } else if OCR_RE.is_match(&canonical) {
            PointerDomain::Ocr
        } else if SIGIL_RE.is_match(&canonical) {
            PointerDomain::Sigil
        } else if CLAIM_RE.is_match(&canonical) {
            PointerDomain::Claim
        } else if GRAPH_RE.is_match(&canonical) {
            PointerDomain::Graph
        } else {
            return Err(ContractError::InvalidPointer(raw.into()));
        };
        Ok(Self {
            raw: canonical,
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

fn canonicalize_token(raw: &str) -> Result<String, ContractError> {
    let mut normalized = raw.nfkc().collect::<String>();
    normalized = normalized.replace("\r\n", "\n");

    if normalized.len() > 256 {
        return Err(ContractError::InvalidPointer(
            "pointer too long (max 256 chars)".into(),
        ));
    }

    if normalized.contains(char::is_whitespace) {
        return Err(ContractError::InvalidPointer(
            "whitespace not permitted in pointer".into(),
        ));
    }

    if !normalized.starts_with("<@") || !normalized.ends_with('>') {
        return Err(ContractError::InvalidPointer(
            "pointer must begin with '<@' and end with '>'".into(),
        ));
    }

    let prefix_end = normalized[2..]
        .find(':')
        .map(|idx| idx + 2)
        .ok_or_else(|| ContractError::InvalidPointer("pointer missing domain separator".into()))?;

    let prefix = &normalized[2..prefix_end];
    let lowered_prefix = prefix.to_ascii_lowercase();

    let mut canonical = String::with_capacity(normalized.len());
    canonical.push_str("<@");
    canonical.push_str(&lowered_prefix);
    canonical.push(':');
    canonical.push_str(&normalized[prefix_end + 1..]);

    Ok(canonical)
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

    #[test]
    fn canonicalizes_uppercase_prefix() {
        let ptr = Pointer::parse(
            "<@BLOB:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef>",
        )
        .unwrap();
        assert_eq!(
            ptr.raw,
            "<@blob:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef>"
        );
    }

    #[test]
    fn rejects_pointer_with_whitespace() {
        let err = Pointer::parse("<@blob:sha256:abcd 0123>").unwrap_err();
        assert!(matches!(err, ContractError::InvalidPointer(_)));
    }
}
