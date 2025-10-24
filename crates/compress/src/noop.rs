use crate::{Budget, Compressed, Compressor, Domain};
use anyhow::Result;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct NoopCompressor {
    domain: Domain,
}

impl NoopCompressor {
    pub fn new(domain: Domain) -> Self {
        Self { domain }
    }
}

impl Default for NoopCompressor {
    fn default() -> Self {
        Self {
            domain: Domain::Prompt,
        }
    }
}

impl Compressor for NoopCompressor {
    fn id(&self) -> &'static str {
        match self.domain {
            Domain::Prompt => "noop.prompt",
            Domain::Memory => "noop.memory",
            Domain::Kv => "noop.kv",
        }
    }

    fn domain(&self) -> Domain {
        self.domain
    }

    fn compress(&self, input: &str, _budget: Budget) -> Result<Compressed> {
        Ok(Compressed::from_text(
            self.domain,
            input.to_owned(),
            json!({
                "applied_ratio": 1.0,
                "compressor": self.id(),
                "fallback": true
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_roundtrips() {
        let compressor = NoopCompressor::new(Domain::Prompt);
        let blob = compressor
            .compress("hello world", Budget::default())
            .expect("compress");
        assert_eq!(blob.to_text().unwrap(), "hello world");
        assert_eq!(blob.domain, Domain::Prompt);
        assert_eq!(compressor.decompress(&blob).unwrap(), "hello world");
    }
}
