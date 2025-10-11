use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::Value;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use arw_core::runtime_bundles::{
    load_catalogs_from_dir, RuntimeBundle, RuntimeBundleCatalog, RuntimeBundleCatalogSource,
};
use arw_runtime::{RuntimeAccelerator, RuntimeModality};

const DEFAULT_SCAN_MSG: &str = "runtime bundle catalog scan";

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBundleCatalogView {
    pub path: String,
    pub version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub bundles: Vec<RuntimeBundle>,
}

impl RuntimeBundleCatalogView {
    fn from_source(src: RuntimeBundleCatalogSource) -> Self {
        let RuntimeBundleCatalogSource { path, catalog } = src;
        Self::from_catalog(path.to_string_lossy().into_owned(), catalog)
    }

    fn from_catalog(path: String, catalog: RuntimeBundleCatalog) -> Self {
        Self {
            path,
            version: catalog.version,
            channel: catalog.channel,
            notes: catalog.notes,
            bundles: catalog.bundles,
        }
    }
}

pub struct RuntimeBundleStore {
    roots: Vec<PathBuf>,
    catalogs: RwLock<Vec<RuntimeBundleCatalogView>>,
    installations: RwLock<Vec<RuntimeBundleInstallation>>,
}

impl RuntimeBundleStore {
    pub async fn load_default() -> Arc<Self> {
        let roots = discover_roots();
        let store = Arc::new(Self {
            roots,
            catalogs: RwLock::new(Vec::new()),
            installations: RwLock::new(Vec::new()),
        });
        if let Err(err) = store.reload().await {
            warn!(
                target: "arw::runtime",
                error = %err,
                "failed to load initial runtime bundle catalogs"
            );
        }
        store
    }

    pub async fn reload(&self) -> Result<()> {
        let mut collected: Vec<RuntimeBundleCatalogView> = Vec::new();
        for root in &self.roots {
            let dir = root.clone();
            let exists = dir.is_dir();
            let display_path = dir.display().to_string();
            if !exists {
                debug!(
                    target: "arw::runtime",
                    path = %display_path,
                    "runtime bundle directory not present; skipping"
                );
                continue;
            }
            let load_dir = dir.clone();
            let catalogs = tokio::task::spawn_blocking(move || load_catalogs_from_dir(&load_dir))
                .await
                .map_err(|err| anyhow!("{} join error: {err}", DEFAULT_SCAN_MSG))??;
            if catalogs.is_empty() {
                debug!(
                    target: "arw::runtime",
                    path = %display_path,
                    "no runtime bundle catalogs found"
                );
            }
            for catalog in catalogs {
                collected.push(RuntimeBundleCatalogView::from_source(catalog));
            }
        }

        collected.sort_by(|a, b| a.path.cmp(&b.path));

        let mut installations: Vec<RuntimeBundleInstallation> = Vec::new();
        for root in &self.roots {
            let mut items = match load_installations_from_root(root).await {
                Ok(list) => list,
                Err(err) => {
                    warn!(
                        target: "arw::runtime",
                        root = %root.display(),
                        error = %err,
                        "failed to inspect runtime bundle installations"
                    );
                    continue;
                }
            };
            installations.append(&mut items);
        }
        installations.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.root.cmp(&b.root)));

        let catalog_count = collected.len();
        let installation_count = installations.len();
        {
            let mut guard = self.catalogs.write().await;
            *guard = collected;
        }
        {
            let mut guard = self.installations.write().await;
            *guard = installations;
        }
        let roots_list = self
            .roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();

        info!(
            target: "arw::runtime",
            roots = ?roots_list,
            catalogs = catalog_count,
            installations = installation_count,
            "runtime bundle catalogs loaded"
        );
        Ok(())
    }

    pub async fn catalogs(&self) -> Vec<RuntimeBundleCatalogView> {
        self.catalogs.read().await.clone()
    }

    pub async fn installations(&self) -> Vec<RuntimeBundleInstallation> {
        self.installations.read().await.clone()
    }

    pub async fn snapshot(&self) -> serde_json::Value {
        let catalogs = self.catalogs().await;
        let installations = self.installations().await;
        let roots = self
            .roots
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let now = Utc::now();
        let generated = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let generated_ms = now.timestamp_millis().max(0) as u64;
        serde_json::json!({
            "generated": generated,
            "generated_ms": generated_ms,
            "roots": roots,
            "installations": installations,
            "catalogs": catalogs,
        })
    }
}

fn discover_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(raw) = std::env::var("ARW_RUNTIME_BUNDLE_DIR") {
        for part in raw.split(';') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            roots.push(PathBuf::from(trimmed));
        }
    }
    if roots.is_empty() {
        if let Some(config_root) = arw_core::resolve_config_path("configs/runtime") {
            roots.push(config_root);
        }
    }
    let state_root = crate::util::state_dir().join("runtime").join("bundles");
    if !roots.iter().any(|p| p == &state_root) {
        roots.push(state_root);
    }
    roots
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBundleArtifactSummary {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeBundleInstallation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modalities: Vec<RuntimeModality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator: Option<RuntimeAccelerator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_path: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RuntimeBundleArtifactSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle: Option<RuntimeBundle>,
}

async fn load_installations_from_root(root: &Path) -> Result<Vec<RuntimeBundleInstallation>> {
    let mut installs = Vec::new();
    let mut dir = match fs::read_dir(root).await {
        Ok(dir) => dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(installs),
        Err(err) => {
            return Err(anyhow!(
                "reading runtime bundle root {}: {err}",
                root.display()
            ))
        }
    };
    loop {
        match dir.next_entry().await {
            Ok(Some(entry)) => {
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                if let Some(install) =
                    load_installation_from_dir(root, entry.path().as_path()).await?
                {
                    installs.push(install);
                }
            }
            Ok(None) => break,
            Err(err) => {
                return Err(anyhow!(
                    "iterating runtime bundle root {}: {err}",
                    root.display()
                ));
            }
        }
    }
    Ok(installs)
}

async fn load_installation_from_dir(
    root: &Path,
    dir: &Path,
) -> Result<Option<RuntimeBundleInstallation>> {
    let metadata_path = dir.join("bundle.json");
    let metadata_value = match fs::read(&metadata_path).await {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                warn!(
                    target: "arw::runtime",
                    path = %metadata_path.display(),
                    error = %err,
                    "failed to parse runtime bundle metadata"
                );
                None
            }
        },
        Err(err) if err.kind() == ErrorKind::NotFound => None,
        Err(err) => {
            warn!(
                target: "arw::runtime",
                path = %metadata_path.display(),
                error = %err,
                "failed to read runtime bundle metadata"
            );
            None
        }
    };

    let mut bundle_full: Option<RuntimeBundle> = None;
    let mut id = dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bundle")
        .to_string();
    let mut name: Option<String> = None;
    let mut adapter: Option<String> = None;
    let mut profiles: Vec<String> = Vec::new();
    let mut modalities: Vec<RuntimeModality> = Vec::new();
    let mut accelerator: Option<RuntimeAccelerator> = None;
    let mut channel: Option<String> = None;
    let mut installed_at: Option<String> = None;
    let mut imported_at: Option<String> = None;
    let mut source: Option<Value> = None;

    if let Some(metadata) = metadata_value.as_ref() {
        if let Some(bundle_node) = metadata.get("bundle") {
            if let Ok(parsed) = serde_json::from_value::<RuntimeBundle>(bundle_node.clone()) {
                id = parsed.id.clone();
                name = Some(parsed.name.clone());
                adapter = Some(parsed.adapter.clone());
                profiles = parsed.profiles.clone();
                modalities = parsed.modalities.clone();
                accelerator = parsed.accelerator.clone();
                bundle_full = Some(parsed);
            } else {
                if let Some(text) = bundle_node
                    .get("id")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    id = text.to_string();
                }
                name = bundle_node
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                adapter = bundle_node
                    .get("adapter")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                profiles = parse_string_array(bundle_node.get("profiles"));
                modalities = parse_modalities(bundle_node.get("modalities"));
                accelerator = bundle_node
                    .get("accelerator")
                    .and_then(|value| value.as_str())
                    .and_then(parse_accelerator);
            }
        }
        channel = metadata
            .pointer("/catalog/channel")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        installed_at = metadata
            .get("installed_at")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        imported_at = metadata
            .get("imported_at")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        source = metadata.get("source").cloned();
    }

    let artifacts_dir = dir.join("artifacts");
    let artifacts = load_artifact_summaries(&artifacts_dir).await;

    if bundle_full.is_none() && metadata_value.is_none() && artifacts.is_empty() {
        return Ok(None);
    }

    Ok(Some(RuntimeBundleInstallation {
        id,
        name,
        adapter,
        profiles,
        modalities,
        accelerator,
        channel,
        installed_at,
        imported_at,
        source,
        metadata_path: metadata_value
            .as_ref()
            .map(|_| metadata_path.display().to_string()),
        artifacts,
        root: Some(root.display().to_string()),
        bundle: bundle_full,
    }))
}

async fn load_artifact_summaries(dir: &Path) -> Vec<RuntimeBundleArtifactSummary> {
    let mut summaries = Vec::new();
    let mut reader = match fs::read_dir(dir).await {
        Ok(reader) => reader,
        Err(err) if err.kind() == ErrorKind::NotFound => return summaries,
        Err(err) => {
            warn!(
                target: "arw::runtime",
                path = %dir.display(),
                error = %err,
                "failed to read runtime bundle artifacts directory"
            );
            return summaries;
        }
    };

    loop {
        match reader.next_entry().await {
            Ok(Some(entry)) => {
                let Ok(file_type) = entry.file_type().await else {
                    continue;
                };
                if !file_type.is_file() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = match entry.metadata().await {
                    Ok(meta) => Some(meta.len()),
                    Err(err) => {
                        warn!(
                            target: "arw::runtime",
                            path = %entry.path().display(),
                            error = %err,
                            "failed to read runtime bundle artifact metadata"
                        );
                        None
                    }
                };
                summaries.push(RuntimeBundleArtifactSummary { name, bytes });
            }
            Ok(None) => break,
            Err(err) => {
                warn!(
                    target: "arw::runtime",
                    path = %dir.display(),
                    error = %err,
                    "failed during runtime bundle artifact scan"
                );
                break;
            }
        }
    }

    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    summaries
}

fn parse_string_array(node: Option<&Value>) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(Value::Array(items)) = node {
        for entry in items {
            if let Some(text) = entry.as_str() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    values.push(trimmed.to_string());
                }
            }
        }
    }
    values
}

fn parse_modalities(node: Option<&Value>) -> Vec<RuntimeModality> {
    let mut values = Vec::new();
    if let Some(Value::Array(items)) = node {
        for entry in items {
            if let Some(text) = entry.as_str() {
                match text.trim().to_ascii_lowercase().as_str() {
                    "text" => values.push(RuntimeModality::Text),
                    "audio" => values.push(RuntimeModality::Audio),
                    "vision" => values.push(RuntimeModality::Vision),
                    other => {
                        warn!(
                            target: "arw::runtime",
                            modality = %other,
                            "unknown runtime modality in bundle metadata"
                        );
                    }
                }
            }
        }
    }
    values
}

fn parse_accelerator(slug: &str) -> Option<RuntimeAccelerator> {
    match slug.trim().to_ascii_lowercase().as_str() {
        "cpu" => Some(RuntimeAccelerator::Cpu),
        "gpu_cuda" | "cuda" => Some(RuntimeAccelerator::GpuCuda),
        "gpu_rocm" | "rocm" => Some(RuntimeAccelerator::GpuRocm),
        "gpu_metal" | "metal" => Some(RuntimeAccelerator::GpuMetal),
        "gpu_vulkan" | "vulkan" => Some(RuntimeAccelerator::GpuVulkan),
        "npu_directml" | "directml" => Some(RuntimeAccelerator::NpuDirectml),
        "npu_coreml" | "coreml" => Some(RuntimeAccelerator::NpuCoreml),
        "npu" => Some(RuntimeAccelerator::NpuOther),
        "other" => Some(RuntimeAccelerator::Other),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn installation_scanner_parses_metadata() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let bundle_dir = root.join("llama_bundle");
        std::fs::create_dir_all(bundle_dir.join("artifacts"))?;

        let metadata = json!({
            "bundle": {
                "id": "llama.cpp-preview/linux-x86_64-cpu",
                "name": "Test LLaMA",
                "adapter": "process",
                "modalities": ["text"],
                "accelerator": "cpu",
                "profiles": ["balanced", "silent"]
            },
            "catalog": {
                "channel": "preview"
            },
            "installed_at": "2025-10-11T12:00:00Z",
            "source": {
                "kind": "local"
            }
        });
        let metadata_path = bundle_dir.join("bundle.json");
        if let Some(parent) = metadata_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata)?)?;
        std::fs::write(bundle_dir.join("artifacts").join("llama.bin"), b"123456")?;

        let installs = load_installations_from_root(root).await?;
        assert_eq!(installs.len(), 1);
        let install = &installs[0];
        assert_eq!(install.id, "llama.cpp-preview/linux-x86_64-cpu".to_string());
        assert_eq!(install.name.as_deref(), Some("Test LLaMA"));
        assert_eq!(install.adapter.as_deref(), Some("process"));
        assert_eq!(install.modalities, vec![RuntimeModality::Text]);
        assert_eq!(install.accelerator, Some(RuntimeAccelerator::Cpu));
        assert_eq!(install.channel.as_deref(), Some("preview"));
        assert_eq!(install.artifacts.len(), 1);
        assert_eq!(install.artifacts[0].name, "llama.bin");
        assert!(install.artifacts[0].bytes.is_some());
        assert_eq!(
            install.metadata_path.as_deref(),
            Some(metadata_path.to_string_lossy().as_ref())
        );
        Ok(())
    }

    #[tokio::test]
    async fn installation_without_metadata_and_artifacts_skipped() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let bundle_dir = tmp.path().join("empty_bundle");
        std::fs::create_dir_all(&bundle_dir)?;
        let installs = load_installations_from_root(tmp.path()).await?;
        assert!(installs.is_empty());
        Ok(())
    }
}
