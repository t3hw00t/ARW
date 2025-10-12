use std::collections::HashSet;
use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

const ENV_SIGNERS_PATH: &str = "ARW_RUNTIME_BUNDLE_SIGNERS";

/// Wire format for the runtime bundle signer registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundleSignerRegistryFile {
    pub version: u32,
    #[serde(default)]
    pub signers: Vec<RuntimeBundleSignerEntry>,
}

/// Individual signer entry in the registry JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundleSignerEntry {
    pub key_id: String,
    pub public_key_b64: String,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// Parsed runtime bundle signer registry ready for lookups.
#[derive(Debug, Clone)]
pub struct RuntimeBundleSignerRegistry {
    version: u32,
    signers: Vec<RuntimeBundleSigner>,
}

impl RuntimeBundleSignerRegistry {
    /// Load the signer registry, honoring environment overrides and falling back to the workspace config path.
    pub fn load_default() -> Result<Option<Self>> {
        if let Ok(raw) = std::env::var(ENV_SIGNERS_PATH) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Self::load_from_path(trimmed).map(Some);
            }
        }

        if let Some(default_path) =
            crate::resolve_config_path("configs/runtime/bundle_signers.json")
        {
            return Self::load_from_path(default_path).map(Some);
        }

        Ok(None)
    }

    /// Load the registry from a JSON file.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let file = File::open(path_ref).with_context(|| {
            format!(
                "opening runtime bundle signer registry {}",
                path_ref.display()
            )
        })?;
        let data: RuntimeBundleSignerRegistryFile =
            serde_json::from_reader(file).with_context(|| {
                format!(
                    "parsing runtime bundle signer registry JSON from {}",
                    path_ref.display()
                )
            })?;
        Self::from_entries(data.version, data.signers)
    }

    /// Construct a registry from entries (useful for tests).
    pub fn from_entries(version: u32, entries: Vec<RuntimeBundleSignerEntry>) -> Result<Self> {
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut seen_keys: HashSet<String> = HashSet::new();
        let mut signers: Vec<RuntimeBundleSigner> = Vec::new();

        for entry in entries {
            let signer = RuntimeBundleSigner::try_from(entry)?;
            if !seen_ids.insert(signer.key_id.clone()) {
                return Err(anyhow!(
                    "duplicate runtime bundle signer key_id {}",
                    signer.key_id
                ));
            }
            if !seen_keys.insert(signer.public_key_b64.clone()) {
                return Err(anyhow!(
                    "duplicate runtime bundle signer public key (b64) {}",
                    signer.public_key_b64
                ));
            }
            signers.push(signer);
        }

        signers.sort_by(|a, b| a.key_id.cmp(&b.key_id));
        Ok(Self { version, signers })
    }

    /// Registry schema version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Iterator over configured signers.
    pub fn signers(&self) -> impl Iterator<Item = &RuntimeBundleSigner> {
        self.signers.iter()
    }

    /// Return the first signer that trusts the provided key/channel combination.
    pub fn find_trusted(
        &self,
        key_id: Option<&str>,
        public_key_b64: Option<&str>,
        channel: Option<&str>,
    ) -> Option<&RuntimeBundleSigner> {
        self.signers
            .iter()
            .find(|signer| signer.matches(key_id, public_key_b64, channel))
    }

    /// Convenience helper for boolean checks.
    pub fn is_trusted(
        &self,
        key_id: Option<&str>,
        public_key_b64: Option<&str>,
        channel: Option<&str>,
    ) -> bool {
        self.find_trusted(key_id, public_key_b64, channel).is_some()
    }
}

/// Parsed signer persisted in the registry.
#[derive(Debug, Clone)]
pub struct RuntimeBundleSigner {
    pub key_id: String,
    pub public_key: [u8; 32],
    pub public_key_b64: String,
    pub issuer: Option<String>,
    pub channels: Vec<String>,
    pub notes: Option<String>,
    pub expires_at: Option<String>,
}

impl RuntimeBundleSigner {
    fn matches(
        &self,
        key_id: Option<&str>,
        public_key_b64: Option<&str>,
        channel: Option<&str>,
    ) -> bool {
        let key_match = match (key_id, public_key_b64) {
            (Some(id), Some(pk)) => {
                (!id.trim().is_empty() && id == self.key_id) || pk == self.public_key_b64
            }
            (Some(id), None) => !id.trim().is_empty() && id == self.key_id,
            (None, Some(pk)) => pk == self.public_key_b64,
            (None, None) => false,
        };
        if !key_match {
            return false;
        }
        self.matches_channel(channel)
    }

    fn matches_channel(&self, channel: Option<&str>) -> bool {
        if self.channels.is_empty() {
            return true;
        }
        if let Some(ch) = channel {
            let trimmed = ch.trim();
            if trimmed.is_empty() {
                return false;
            }
            self.channels.iter().any(|allowed| {
                allowed == "*"
                    || allowed.eq_ignore_ascii_case("all")
                    || allowed.eq_ignore_ascii_case(trimmed)
            })
        } else {
            false
        }
    }
}

impl TryFrom<RuntimeBundleSignerEntry> for RuntimeBundleSigner {
    type Error = anyhow::Error;

    fn try_from(entry: RuntimeBundleSignerEntry) -> Result<Self> {
        let key_id_raw = entry.key_id.trim().to_string();
        if key_id_raw.is_empty() {
            return Err(anyhow!("runtime bundle signer key_id cannot be empty"));
        }

        let pk_b64 = entry.public_key_b64.trim().to_string();
        if pk_b64.is_empty() {
            return Err(anyhow!(
                "runtime bundle signer public_key_b64 cannot be empty"
            ));
        }
        let pk_bytes = base64::engine::general_purpose::STANDARD
            .decode(pk_b64.as_bytes())
            .context("decoding runtime bundle signer public_key_b64")?;
        let public_key: [u8; 32] = pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("runtime bundle signer public key must be 32 bytes"))?;

        let issuer = entry.issuer.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let notes = entry.notes.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let expires_at = entry.expires_at.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
        let channels: Vec<String> = entry
            .channels
            .into_iter()
            .filter_map(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .collect();

        Ok(Self {
            key_id: key_id_raw,
            public_key,
            public_key_b64: pk_b64,
            issuer,
            channels,
            notes,
            expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry() -> RuntimeBundleSignerEntry {
        RuntimeBundleSignerEntry {
            key_id: "preview-bundle-signing".to_string(),
            public_key_b64: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
            issuer: Some("ci@example.com".to_string()),
            channels: vec!["Preview".to_string()],
            notes: Some("preview channel signer".to_string()),
            expires_at: None,
        }
    }

    #[test]
    fn registry_rejects_duplicate_keys() {
        let entry = sample_entry();
        let dup = RuntimeBundleSignerEntry {
            public_key_b64: entry.public_key_b64.clone(),
            ..sample_entry()
        };
        let result = RuntimeBundleSignerRegistry::from_entries(1, vec![entry, dup]);
        assert!(result.is_err(), "duplicate entries should fail");
    }

    #[test]
    fn registry_trusts_matching_channel() -> Result<()> {
        let registry = RuntimeBundleSignerRegistry::from_entries(1, vec![sample_entry()])?;
        assert!(
            registry.is_trusted(
                Some("preview-bundle-signing"),
                Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                Some("preview")
            ),
            "channel comparison should be case-insensitive"
        );
        assert!(
            !registry.is_trusted(
                Some("preview-bundle-signing"),
                Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                Some("stable")
            ),
            "signer limited to preview channel should not match stable"
        );
        assert!(
            !registry.is_trusted(
                Some("preview-bundle-signing"),
                Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                None
            ),
            "channel-restricted signer should not match when channel context is absent"
        );
        Ok(())
    }

    #[test]
    fn registry_supports_wildcard_channel() -> Result<()> {
        let mut entry = sample_entry();
        entry.channels = vec!["*".to_string()];
        let registry = RuntimeBundleSignerRegistry::from_entries(1, vec![entry])?;
        assert!(
            registry.is_trusted(
                Some("preview-bundle-signing"),
                Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                Some("stable")
            ),
            "wildcard channel should match any channel"
        );
        Ok(())
    }

    #[test]
    fn registry_matches_when_channel_not_restricted() -> Result<()> {
        let mut entry = sample_entry();
        entry.channels.clear();
        let registry = RuntimeBundleSignerRegistry::from_entries(1, vec![entry])?;
        assert!(
            registry.is_trusted(
                Some("preview-bundle-signing"),
                Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                None
            ),
            "signer without channel constraint should match regardless of channel context"
        );
        Ok(())
    }
}
