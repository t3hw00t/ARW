use arw_protocol::GatingCapsule;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::resolve_config_path;

/// Error while loading capsule presets from disk.
#[derive(Debug)]
pub enum CapsulePresetError {
    /// No preset with the requested id exists.
    NotFound(String),
    /// Underlying IO failure while reading the preset directory or file.
    Io(std::io::Error),
    /// Capsule JSON failed to parse.
    Parse(serde_json::Error),
}

impl std::fmt::Display for CapsulePresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapsulePresetError::NotFound(id) => {
                write!(f, "capsule preset '{}' not found", id)
            }
            CapsulePresetError::Io(err) => write!(f, "capsule preset IO error: {}", err),
            CapsulePresetError::Parse(err) => write!(f, "capsule preset parse error: {}", err),
        }
    }
}

impl std::error::Error for CapsulePresetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CapsulePresetError::Io(err) => Some(err),
            CapsulePresetError::Parse(err) => Some(err),
            CapsulePresetError::NotFound(_) => None,
        }
    }
}

/// Capsule preset metadata packaged with the install.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapsulePresetSummary {
    pub id: String,
    pub file_name: String,
    pub path: String,
    pub version: Option<String>,
    pub issuer: Option<String>,
    pub hop_ttl: Option<u32>,
    pub lease_duration_ms: Option<u64>,
    pub renew_within_ms: Option<u64>,
    pub denies: usize,
    pub contracts: usize,
    pub signature_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<u64>,
}

/// Full preset contents plus metadata.
#[derive(Clone, Debug)]
pub struct CapsulePreset {
    pub summary: CapsulePresetSummary,
    pub capsule: GatingCapsule,
}

/// Return the on-disk directory for capsule presets, if it exists.
fn presets_dir() -> Option<PathBuf> {
    resolve_config_path("configs/capsules").filter(|path| path.exists())
}

/// Compute a lowercase SHA-256 digest of the capsule payload (including signature).
fn capsule_sha256(cap: &GatingCapsule) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(cap).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

fn metadata_from_path(path: &Path) -> Option<u64> {
    let meta = path.metadata().ok()?;
    let modified = meta.modified().ok()?;
    let ms = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    Some(ms)
}

fn read_capsule(path: &Path) -> Result<GatingCapsule, CapsulePresetError> {
    let raw = fs::read_to_string(path).map_err(CapsulePresetError::Io)?;
    serde_json::from_str(&raw).map_err(CapsulePresetError::Parse)
}

fn summary_from_capsule(path: &Path, capsule: &GatingCapsule) -> CapsulePresetSummary {
    CapsulePresetSummary {
        id: capsule.id.clone(),
        file_name: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string(),
        path: path.display().to_string(),
        version: Some(capsule.version.clone()),
        issuer: capsule.issuer.clone(),
        hop_ttl: capsule.hop_ttl,
        lease_duration_ms: capsule.lease_duration_ms,
        renew_within_ms: capsule.renew_within_ms,
        denies: capsule.denies.len(),
        contracts: capsule.contracts.len(),
        signature_present: capsule.signature.is_some(),
        sha256: capsule_sha256(capsule),
        modified_ms: metadata_from_path(path),
    }
}

/// List capsule presets packaged with the install.
pub fn list_capsule_presets() -> Result<Vec<CapsulePreset>, CapsulePresetError> {
    let Some(dir) = presets_dir() else {
        return Ok(Vec::new());
    };
    let mut out: Vec<CapsulePreset> = Vec::new();
    for entry in fs::read_dir(&dir).map_err(CapsulePresetError::Io)? {
        let entry = entry.map_err(CapsulePresetError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        if matches!(path.extension().and_then(|s| s.to_str()), Some(ext) if ext.eq_ignore_ascii_case("json"))
        {
            let capsule = match read_capsule(&path) {
                Ok(cap) => cap,
                Err(err) => {
                    tracing::warn!(
                        target: "arw::capsules",
                        path = %path.display(),
                        error = %err,
                        "failed to parse capsule preset; skipping"
                    );
                    continue;
                }
            };
            let summary = summary_from_capsule(&path, &capsule);
            out.push(CapsulePreset { summary, capsule });
        }
    }
    out.sort_by(|a, b| a.summary.id.cmp(&b.summary.id));
    Ok(out)
}

/// Load a capsule preset by id.
pub fn load_capsule_preset(id: &str) -> Result<CapsulePreset, CapsulePresetError> {
    let presets = list_capsule_presets()?;
    presets
        .into_iter()
        .find(|preset| preset.summary.id == id)
        .ok_or_else(|| CapsulePresetError::NotFound(id.to_string()))
}
