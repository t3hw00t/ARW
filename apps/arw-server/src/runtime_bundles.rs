use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use arw_core::runtime_bundles::{
    load_catalogs_from_dir, RuntimeBundle, RuntimeBundleCatalog, RuntimeBundleCatalogSource,
};

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
}

impl RuntimeBundleStore {
    pub async fn load_default() -> Arc<Self> {
        let roots = discover_roots();
        let store = Arc::new(Self {
            roots,
            catalogs: RwLock::new(Vec::new()),
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

        {
            let mut guard = self.catalogs.write().await;
            *guard = collected;
        }
        let catalog_count = self.catalogs.read().await.len();
        let roots_list = self
            .roots
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();

        info!(
            target: "arw::runtime",
            roots = ?roots_list,
            catalogs = catalog_count,
            "runtime bundle catalogs loaded"
        );
        Ok(())
    }

    pub async fn catalogs(&self) -> Vec<RuntimeBundleCatalogView> {
        self.catalogs.read().await.clone()
    }

    pub async fn snapshot(&self) -> serde_json::Value {
        let catalogs = self.catalogs().await;
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
