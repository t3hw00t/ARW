use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use arw_runtime::{RuntimeAccelerator, RuntimeModality};
use serde::{Deserialize, Serialize};

/// Catalog of managed runtime bundles (e.g., llama.cpp, Whisper.cpp).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundleCatalog {
    /// Schema version for the catalog file.
    pub version: u32,
    /// Optional human-readable channel label (e.g., "preview", "stable").
    #[serde(default)]
    pub channel: Option<String>,
    /// Optional free-form notes about the catalog.
    #[serde(default)]
    pub notes: Option<String>,
    /// Bundles included in this catalog.
    #[serde(default)]
    pub bundles: Vec<RuntimeBundle>,
}

/// A single runtime bundle definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundle {
    /// Unique bundle identifier (stable across releases).
    pub id: String,
    /// Display name surfaced to operators.
    pub name: String,
    /// Optional short description.
    #[serde(default)]
    pub description: Option<String>,
    /// Adapter slug the bundle targets (defaults to "process").
    #[serde(default = "default_adapter")]
    pub adapter: String,
    /// Modalities supported by the runtime (text/audio/vision/pointer).
    #[serde(default)]
    pub modalities: Vec<RuntimeModality>,
    /// Preferred accelerator for this bundle.
    #[serde(default)]
    pub accelerator: Option<RuntimeAccelerator>,
    /// Profiles exposed to the user (performance/balanced/silent/custom/etc).
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Supported platforms (OS/arch pairs).
    #[serde(default)]
    pub platforms: Vec<RuntimeBundlePlatform>,
    /// Binary artifacts composing the bundle.
    #[serde(default)]
    pub artifacts: Vec<RuntimeBundleArtifact>,
    /// Additional notes surfaced to operators.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Optional SPDX license identifier or free-form license note.
    #[serde(default)]
    pub license: Option<String>,
    /// Optional support metadata (driver versions, glibc requirement, etc.).
    #[serde(default)]
    pub support: Option<RuntimeBundleSupport>,
    /// Free-form metadata for adapters or tooling.
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundlePlatform {
    pub os: String,
    pub arch: String,
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundleArtifact {
    /// Artifact kind (e.g., "archive", "binary", "signature").
    pub kind: String,
    /// Optional artifact format (e.g., "tar.zst", "zip", "msi").
    #[serde(default)]
    pub format: Option<String>,
    /// Optional download URL (populated once publishing pipeline lands).
    #[serde(default)]
    pub url: Option<String>,
    /// Optional SHA-256 hash for integrity checks.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Optional size in bytes.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// Optional notes for operators (e.g., install instructions).
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeBundleSupport {
    #[serde(default)]
    pub min_glibc: Option<String>,
    #[serde(default)]
    pub min_macos: Option<String>,
    #[serde(default)]
    pub min_windows: Option<String>,
    #[serde(default)]
    pub driver_notes: Option<String>,
    #[serde(default)]
    pub additional: Option<serde_json::Value>,
}

/// Catalog plus the on-disk source path.
#[derive(Debug, Clone)]
pub struct RuntimeBundleCatalogSource {
    pub path: PathBuf,
    pub catalog: RuntimeBundleCatalog,
}

/// Load a catalog from the provided JSON file.
pub fn load_catalog_from_path<P: AsRef<Path>>(path: P) -> Result<RuntimeBundleCatalog> {
    let path_ref = path.as_ref();
    let file = File::open(path_ref)
        .with_context(|| format!("opening runtime bundle catalog {}", path_ref.display()))?;
    let catalog: RuntimeBundleCatalog = serde_json::from_reader(file).with_context(|| {
        format!(
            "parsing runtime bundle catalog JSON from {}",
            path_ref.display()
        )
    })?;
    Ok(catalog)
}

/// Load every `bundles*.json` catalog from the provided directory.
pub fn load_catalogs_from_dir<P: AsRef<Path>>(dir: P) -> Result<Vec<RuntimeBundleCatalogSource>> {
    let dir_ref = dir.as_ref();
    let read_dir = std::fs::read_dir(dir_ref).with_context(|| {
        format!(
            "reading runtime bundle catalog directory {}",
            dir_ref.display()
        )
    })?;

    let mut sources = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !(name_str.starts_with("bundles") && name_str.ends_with(".json")) {
            continue;
        }
        let path = entry.path();
        let catalog = load_catalog_from_path(&path)?;
        sources.push(RuntimeBundleCatalogSource { path, catalog });
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(sources)
}

fn default_adapter() -> String {
    "process".to_string()
}
