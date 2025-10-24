//! Core compression abstractions and adapters.

mod llmlingua;
mod noop;

pub use llmlingua::{LlmlinguaCompressor, LlmlinguaDetectError};
pub use noop::NoopCompressor;

use anyhow::{Context, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Supported compression domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    /// Prompt/body text slated for model consumption.
    Prompt,
    /// Long-term memory overlays and corpora.
    Memory,
    /// Runtime KV cache payloads.
    Kv,
}

/// Compression mode hints for backends that support multiple strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionMode {
    /// Preserve input spans verbatim; usually extractive summarisation.
    Extractive,
    /// Allow generative rephrasing.
    Abstractive,
}

impl CompressionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CompressionMode::Extractive => "extractive",
            CompressionMode::Abstractive => "abstractive",
        }
    }
}

/// Budget constraint for a compression request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Budget {
    /// Upper bound on output tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_tokens: Option<usize>,
    /// Desired output ratio relative to the original token count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f32>,
    /// Preferred strategy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<CompressionMode>,
    /// Backend-specific hints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extras: Option<Value>,
}

impl Budget {
    /// Clamp ratio into a safe operating window.
    pub fn with_ratio(mut self, ratio: f32) -> Self {
        self.ratio = Some(ratio.clamp(0.01, 1.0));
        self
    }

    /// Clamp target token budget to a sensible minimum.
    pub fn with_target_tokens(mut self, tokens: usize) -> Self {
        self.target_tokens = Some(tokens.max(1));
        self
    }

    /// Ensure the budget is internally consistent.
    pub fn validate(&self) -> Result<()> {
        if let Some(ratio) = self.ratio {
            anyhow::ensure!(
                (0.0..=1.0).contains(&ratio),
                "compression ratio must be between 0 and 1 (inclusive)"
            );
        }
        if let Some(target) = self.target_tokens {
            anyhow::ensure!(target > 0, "target_tokens must be greater than zero");
        }
        Ok(())
    }

    /// Provide a default ratio when none is supplied.
    pub fn fallback_ratio(&self, default_ratio: f32) -> f32 {
        self.ratio.unwrap_or(default_ratio).clamp(0.01, 1.0)
    }
}

/// Supported KV compression backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KvMethod {
    None,
    SnapKv,
    Kivi2Bit,
    CacheGen,
}

impl KvMethod {
    pub fn requires_ratio(self) -> bool {
        matches!(self, KvMethod::SnapKv | KvMethod::CacheGen)
    }

    pub fn requires_bits(self) -> bool {
        matches!(self, KvMethod::Kivi2Bit)
    }
}

/// Declarative runtime KV cache policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KvPolicy {
    pub method: KvMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bits: Option<u32>,
}

impl Default for KvPolicy {
    fn default() -> Self {
        Self {
            method: KvMethod::None,
            ratio: None,
            bits: None,
        }
    }
}

impl KvPolicy {
    pub fn with_method(method: KvMethod) -> Self {
        Self {
            method,
            ..Self::default()
        }
    }

    pub fn normalise(mut self) -> Result<Self> {
        match self.method {
            KvMethod::None => {
                self.ratio = None;
                self.bits = None;
            }
            KvMethod::SnapKv | KvMethod::CacheGen => {
                let mut ratio = self.ratio.unwrap_or(0.25);
                anyhow::ensure!(ratio.is_finite(), "kv ratio must be finite");
                if ratio <= 0.0 {
                    ratio = 0.01;
                }
                if ratio > 1.0 {
                    ratio = 1.0;
                }
                self.ratio = Some(ratio);
                self.bits = None;
            }
            KvMethod::Kivi2Bit => {
                let bits = self.bits.unwrap_or(2);
                anyhow::ensure!((1..=8).contains(&bits), "kv bits must be between 1 and 8");
                self.bits = Some(bits);
                self.ratio = None;
            }
        }
        Ok(self)
    }
}

/// Result payload for compression backends.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compressed {
    pub domain: Domain,
    pub payload: Bytes,
    #[serde(default)]
    pub meta: Value,
}

impl Compressed {
    /// Construct a `Compressed` blob from UTF-8 text.
    pub fn from_text(domain: Domain, text: impl Into<String>, meta: Value) -> Self {
        let text = text.into();
        Self {
            domain,
            payload: Bytes::from(text.clone()),
            meta,
        }
    }

    /// Interpret the payload as UTF-8 text.
    pub fn as_text(&self) -> Result<&str> {
        std::str::from_utf8(&self.payload)
            .with_context(|| format!("{} payload is not valid UTF-8", self.domain_name()))
    }

    /// Convert the payload into an owned UTF-8 string.
    pub fn to_text(&self) -> Result<String> {
        Ok(self.as_text()?.to_owned())
    }

    fn domain_name(&self) -> &'static str {
        match self.domain {
            Domain::Prompt => "prompt",
            Domain::Memory => "memory",
            Domain::Kv => "kv",
        }
    }
}

impl Default for Compressed {
    fn default() -> Self {
        Self {
            domain: Domain::Prompt,
            payload: Bytes::new(),
            meta: json!({}),
        }
    }
}

/// Compression engine contract.
pub trait Compressor: Send + Sync {
    /// Stable identifier for telemetry and routing.
    fn id(&self) -> &'static str;

    /// Domain implemented by this compressor.
    fn domain(&self) -> Domain;

    /// Compress `input` subject to `budget`.
    fn compress(&self, input: &str, budget: Budget) -> Result<Compressed>;

    /// Rehydrate a compressed blob into UTF-8 text.
    fn decompress(&self, blob: &Compressed) -> Result<String> {
        anyhow::ensure!(
            blob.domain == self.domain(),
            "decompress: domain mismatch (expected {:?}, got {:?})",
            self.domain(),
            blob.domain
        );
        blob.to_text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_policy_defaults() {
        let policy = KvPolicy::default().normalise().expect("normalise");
        assert_eq!(policy.method, KvMethod::None);
        assert!(policy.ratio.is_none());
        assert!(policy.bits.is_none());
    }

    #[test]
    fn kv_policy_ratio_defaults() {
        let policy = KvPolicy {
            method: KvMethod::SnapKv,
            ratio: None,
            bits: None,
        }
        .normalise()
        .expect("normalise");
        assert_eq!(policy.method, KvMethod::SnapKv);
        assert_eq!(policy.ratio, Some(0.25));
        assert!(policy.bits.is_none());

        let clamped = KvPolicy {
            method: KvMethod::CacheGen,
            ratio: Some(2.0),
            bits: Some(3),
        }
        .normalise()
        .expect("normalise");
        assert_eq!(clamped.ratio, Some(1.0));
        assert!(clamped.bits.is_none());
    }

    #[test]
    fn kv_policy_bits_defaults() {
        let policy = KvPolicy {
            method: KvMethod::Kivi2Bit,
            ratio: Some(0.1),
            bits: None,
        }
        .normalise()
        .expect("normalise");
        assert!(policy.ratio.is_none());
        assert_eq!(policy.bits, Some(2));

        let too_high = KvPolicy {
            method: KvMethod::Kivi2Bit,
            ratio: None,
            bits: Some(32),
        }
        .normalise();
        assert!(too_high.is_err());
    }
}
