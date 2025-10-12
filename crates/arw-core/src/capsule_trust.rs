use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::resolve_config_path;

pub const DEFAULT_TRUST_FILE: &str = "configs/trust_capsules.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrustEntry {
    pub id: String,
    pub alg: String,
    pub key_b64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct TrustConfig {
    #[serde(default)]
    issuers: Vec<TrustEntry>,
}

fn env_trust_path() -> Option<PathBuf> {
    match std::env::var("ARW_TRUST_CAPSULES") {
        Ok(path) if !path.trim().is_empty() => Some(PathBuf::from(path)),
        _ => None,
    }
}

/// Resolve the trust store path, falling back to the default config path.
pub fn trust_store_path() -> PathBuf {
    if let Some(env_path) = env_trust_path() {
        return env_path;
    }
    if let Some(resolved) = resolve_config_path(DEFAULT_TRUST_FILE) {
        return resolved;
    }
    PathBuf::from(DEFAULT_TRUST_FILE)
}

fn read_config(path: &Path) -> Result<TrustConfig, io::Error> {
    if !path.exists() {
        return Ok(TrustConfig::default());
    }
    let raw = fs::read_to_string(path)?;
    let cfg = serde_json::from_str(&raw).unwrap_or_default();
    Ok(cfg)
}

/// Load trust issuers (id + algorithm + key) from disk.
pub fn load_trust_entries() -> Result<Vec<TrustEntry>, io::Error> {
    let path = trust_store_path();
    Ok(read_config(&path)?.issuers)
}

/// Persist trust issuers back to disk (overwrites file).
pub fn save_trust_entries(entries: &[TrustEntry]) -> Result<(), io::Error> {
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    let cfg = TrustConfig {
        issuers: entries.to_vec(),
    };
    let serialized = serde_json::to_string_pretty(&cfg).map_err(io::Error::other)?;
    fs::write(path, serialized)
}
