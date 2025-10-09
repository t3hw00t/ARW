use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use arw_topics as topics;
use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use tokio::fs;
use tracing::warn;

use super::{CasGcDeletedItem, HashItem, HashPage, ModelStore};

pub(super) type ManifestHashIndex = HashMap<String, ManifestHashRefs>;

#[derive(Clone, Default)]
pub(super) struct ManifestHashRefs {
    bytes: u64,
    path: Option<String>,
    providers: HashSet<String>,
    models: HashSet<String>,
}

impl ManifestHashRefs {
    pub(super) fn ingest_manifest(&mut self, entry: &Value) {
        if self.bytes == 0 {
            if let Some(bytes) = entry.get("bytes").and_then(|v| v.as_u64()) {
                if bytes > 0 {
                    self.bytes = bytes;
                }
            }
        }
        if self.path.as_ref().map(|p| p.is_empty()).unwrap_or(true) {
            if let Some(path) = entry.get("path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    self.path = Some(path.to_string());
                }
            }
        }
        let provider = entry
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        self.providers.insert(provider.to_string());
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            self.models.insert(id.to_string());
        }
    }

    pub(super) fn to_hash_item(&self, sha256: &str) -> super::HashItem {
        let mut providers: Vec<_> = self.providers.iter().cloned().collect();
        providers.sort();
        let mut models: Vec<_> = self.models.iter().cloned().collect();
        models.sort();
        super::HashItem {
            sha256: sha256.to_string(),
            bytes: self.bytes,
            path: self.path.clone().unwrap_or_default(),
            providers,
            models,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CasGcRequest, ModelStore};
    use super::*;
    use arw_events;
    use serde_json::json;
    use tempfile::tempdir;

    #[tokio::test]
    async fn hashes_page_groups_and_filters_providers() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let items = vec![
            json!({
                "id": "m-primary",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bytes": 10,
                "provider": "alpha",
                "path": "/models/alpha.bin"
            }),
            json!({
                "id": "m-follower",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "provider": "beta"
            }),
            json!({
                "id": "m-two",
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bytes": 7,
                "provider": "alpha",
                "path": "/models/alpha-two.bin"
            }),
        ];
        store.replace_items(items).await;

        let page = store.hashes_page(10, 0, None, None, None, None).await;
        assert_eq!(page.total, 2);
        assert_eq!(page.count, 2);
        assert_eq!(page.limit, 10);
        assert_eq!(page.offset, 0);
        assert_eq!(page.page, 1);
        assert_eq!(page.pages, 1);
        assert!(page.prev_offset.is_none());
        assert!(page.next_offset.is_none());
        assert_eq!(page.last_offset, 0);

        let first = &page.items[0];
        assert_eq!(
            first.sha256,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(first.bytes, 10);
        assert_eq!(first.path, "/models/alpha.bin");
        assert_eq!(first.providers, vec!["alpha", "beta"]);
        assert_eq!(first.models, vec!["m-follower", "m-primary"]);

        let filtered = store
            .hashes_page(10, 0, Some("beta".into()), None, None, None)
            .await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.count, 1);
        assert_eq!(filtered.page, 1);
        assert_eq!(filtered.pages, 1);
        assert!(filtered.prev_offset.is_none());
        assert!(filtered.next_offset.is_none());
        assert_eq!(filtered.last_offset, 0);
        assert_eq!(filtered.items[0].sha256, first.sha256);
    }

    #[tokio::test]
    async fn hashes_page_filters_by_model_id() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let items = vec![
            json!({
                "id": "first-model",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bytes": 12,
                "provider": "alpha",
                "path": "/models/a.bin"
            }),
            json!({
                "id": "second-model",
                "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "bytes": 9,
                "provider": "alpha",
                "path": "/models/b.bin"
            }),
            json!({
                "id": "follower",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "provider": "beta"
            }),
        ];
        store.replace_items(items).await;

        let filtered = store
            .hashes_page(10, 0, None, Some("follower".into()), None, None)
            .await;
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items.len(), 1);
        assert_eq!(filtered.page, 1);
        assert_eq!(filtered.pages, 1);
        assert!(filtered.prev_offset.is_none());
        assert!(filtered.next_offset.is_none());
        assert_eq!(filtered.last_offset, 0);
        let entry = &filtered.items[0];
        assert_eq!(entry.models, vec!["first-model", "follower"]);
    }

    #[tokio::test]
    async fn hashes_page_reports_offsets() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        let mut items = Vec::new();
        for i in 0..103 {
            let sha = format!("{:064x}", i + 1);
            items.push(json!({
                "id": format!("model-{i}"),
                "sha256": sha,
                "bytes": 1 + i as u64,
                "provider": "local",
                "path": format!("/models/model-{i}.bin"),
            }));
        }
        store.replace_items(items).await;

        let page1 = store
            .hashes_page(25, 0, None, None, Some("sha256".into()), Some("asc".into()))
            .await;
        assert_eq!(page1.count, 25);
        assert_eq!(page1.page, 1);
        assert_eq!(page1.pages, 5);
        assert_eq!(page1.prev_offset, None);
        assert_eq!(page1.next_offset, Some(25));
        assert_eq!(page1.last_offset, 100);

        let page2 = store
            .hashes_page(
                25,
                page1.next_offset.expect("next offset"),
                None,
                None,
                Some("sha256".into()),
                Some("asc".into()),
            )
            .await;
        assert_eq!(page2.offset, 25);
        assert_eq!(page2.page, 2);
        assert_eq!(page2.prev_offset, Some(0));
        assert_eq!(page2.next_offset, Some(50));

        let page_last = store
            .hashes_page(
                25,
                9999,
                None,
                None,
                Some("sha256".into()),
                Some("asc".into()),
            )
            .await;
        assert_eq!(page_last.offset, 100);
        assert_eq!(page_last.count, 3);
        assert_eq!(page_last.page, 5);
        assert_eq!(page_last.pages, 5);
        assert_eq!(page_last.prev_offset, Some(75));
        assert_eq!(page_last.next_offset, None);
        assert_eq!(page_last.last_offset, 100);
    }

    #[tokio::test]
    async fn manifest_hash_index_invalidates_on_mutation() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);
        store
            .replace_items(vec![
                json!({
                    "id": "keep",
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "bytes": 4,
                    "provider": "alpha",
                    "path": "/models/keep.bin"
                }),
                json!({
                    "id": "drop",
                    "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "bytes": 8,
                    "provider": "beta",
                    "path": "/models/drop.bin"
                }),
            ])
            .await;

        let first = store.manifest_hash_index().await;
        assert_eq!(first.len(), 2);
        drop(first);

        let removed = store.remove_model("drop").await;
        assert!(removed, "expected model removal to succeed");

        let second = store.manifest_hash_index().await;
        assert_eq!(second.len(), 1);
        assert!(
            second.contains_key("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(!second
            .contains_key("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[tokio::test]
    async fn cas_gc_verbose_reports_deleted_entries() {
        let tmp = tempdir().expect("tempdir");
        let _ctx = crate::test_support::begin_state_env(tmp.path());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let store = ModelStore::new(bus, None);

        let keep_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let stale_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

        store
            .replace_items(vec![json!({
                "id": "keep-model",
                "sha256": keep_hash,
                "bytes": 4,
                "provider": "alpha",
                "path": format!("/models/{keep_hash}"),
            })])
            .await;

        let keep_path = store.cas_blob_path(keep_hash);
        let stale_path = store.cas_blob_path(stale_hash);
        if let Some(parent) = keep_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .expect("create cas dir");
        }

        tokio::fs::write(&keep_path, b"keep")
            .await
            .expect("write keep blob");
        tokio::fs::write(&stale_path, b"stale-bytes")
            .await
            .expect("write stale blob");

        let payload = store
            .cas_gc(CasGcRequest {
                ttl_hours: Some(0),
                verbose: Some(true),
            })
            .await
            .expect("gc response");

        assert_eq!(payload.get("scanned").and_then(Value::as_u64), Some(2));
        assert_eq!(payload.get("deleted").and_then(Value::as_u64), Some(1));
        assert_eq!(payload.get("kept").and_then(Value::as_u64), Some(1));

        let deleted_items = payload
            .get("deleted_items")
            .and_then(Value::as_array)
            .expect("deleted items array");
        assert_eq!(deleted_items.len(), 1);

        tokio::fs::metadata(&keep_path)
            .await
            .expect("keep blob still present");
        assert!(tokio::fs::metadata(&stale_path).await.is_err());
    }
}

impl ModelStore {
    pub(super) fn models_dir(&self) -> PathBuf {
        super::util::state_dir().join("models")
    }

    pub(super) fn cas_dir(&self) -> PathBuf {
        self.models_dir().join("by-hash")
    }

    pub fn cas_blob_path(&self, hash: &str) -> PathBuf {
        self.cas_dir().join(hash)
    }

    pub(super) fn manifest_path(&self, id: &str) -> PathBuf {
        self.models_dir().join(format!("{id}.json"))
    }

    pub(super) fn models_file(&self) -> PathBuf {
        self.models_dir().join("models.json")
    }

    pub(super) async fn persist_download_metrics(&self, ewma: f64) -> Result<(), String> {
        let metrics_path = self.models_dir().join("downloads.metrics.json");
        let body = json!({"ewma_mbps": ewma});
        if let Some(parent) = metrics_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("metrics dir create failed: {e}"))?;
        }
        fs::write(
            &metrics_path,
            serde_json::to_vec_pretty(&body).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| format!("persist download metrics failed: {e}"))
    }

    pub(super) async fn cas_usage_bytes(&self) -> Result<u64, String> {
        let mut total = 0u64;
        let dir = self.cas_dir();
        let mut entries = match fs::read_dir(&dir).await {
            Ok(it) => it,
            Err(_) => return Ok(0),
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(meta) = entry.metadata().await {
                if meta.is_file() {
                    total = total.saturating_add(meta.len());
                }
            }
        }
        Ok(total)
    }

    pub(super) async fn write_manifest(&self, id: &str, manifest: &Value) -> Result<(), String> {
        let path = self.manifest_path(id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create manifest dir failed: {e}"))?;
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?,
        )
        .await
        .map_err(|e| format!("write manifest failed: {e}"))
    }

    pub(super) async fn manifest_hash_index(&self) -> Arc<ManifestHashIndex> {
        if let Some(cached) = self.manifest_index.read().await.as_ref().cloned() {
            return cached;
        }

        let items_snapshot = {
            let guard = self.items.read().await;
            guard.clone()
        };
        let built = Arc::new(Self::collect_manifest_hash_index(&items_snapshot));
        let entries = built.len();

        let mut guard = self.manifest_index.write().await;
        if let Some(existing) = guard.as_ref() {
            return existing.clone();
        }
        metrics::counter!(super::METRIC_MANIFEST_INDEX_REBUILDS).increment(1);
        metrics::gauge!(super::GAUGE_MANIFEST_INDEX_ENTRIES).set(entries as f64);
        super::debug!(entries, "manifest hash index rebuilt");
        *guard = Some(built.clone());
        built
    }

    pub(super) async fn invalidate_manifest_index(&self) {
        self.manifest_index.write().await.take();
    }

    pub(super) fn collect_manifest_hash_index(items: &[Value]) -> ManifestHashIndex {
        let mut index = ManifestHashIndex::new();
        for entry in items {
            let Some(hash) = entry.get("sha256").and_then(|v| v.as_str()) else {
                continue;
            };
            if hash.len() != 64 {
                continue;
            }
            let bucket = index.entry(hash.to_string()).or_default();
            bucket.ingest_manifest(entry);
        }
        index
    }

    pub(super) fn state_dir(&self) -> PathBuf {
        super::util::state_dir()
    }

    pub(super) async fn execute_cas_gc(
        &self,
        ttl_hours: u64,
        verbose: bool,
    ) -> Result<Value, String> {
        let cutoff = Utc::now() - Duration::hours(ttl_hours as i64);
        let manifest_index = self.manifest_hash_index().await;
        let cas_dir = self.cas_dir();
        let mut scanned = 0u64;
        let mut kept = 0u64;
        let mut deleted = 0u64;
        let mut deleted_bytes = 0u64;
        let mut deleted_items = if verbose { Some(Vec::new()) } else { None };

        if let Ok(mut entries) = fs::read_dir(&cas_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                scanned += 1;
                let fname = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if manifest_index.contains_key(&fname) {
                    kept += 1;
                    continue;
                }
                let meta = match entry.metadata().await {
                    Ok(m) => m,
                    Err(err) => {
                        warn!("cas gc metadata failed for {:?}: {err}", path);
                        continue;
                    }
                };
                let (modified, modified_str) = match meta.modified() {
                    Ok(time) => {
                        let dt = DateTime::<Utc>::from(time);
                        (dt, Some(dt.to_rfc3339()))
                    }
                    Err(_) => {
                        let fallback = Utc::now();
                        (fallback, None)
                    }
                };
                if modified > cutoff {
                    kept += 1;
                    continue;
                }
                let size = meta.len();
                if let Err(err) = fs::remove_file(&path).await {
                    warn!("cas gc remove failed {:?}: {err}", path);
                    kept += 1;
                    continue;
                }
                deleted += 1;
                deleted_bytes = deleted_bytes.saturating_add(size);
                if let Some(ref mut list) = deleted_items {
                    let rel_path = path.strip_prefix(&cas_dir).unwrap_or(&path).to_path_buf();
                    list.push(CasGcDeletedItem {
                        sha256: fname.clone(),
                        path: rel_path.to_string_lossy().into_owned(),
                        bytes: size,
                        last_modified: modified_str,
                    });
                }
            }
        }

        let mut payload = json!({
            "scanned": scanned,
            "kept": kept,
            "deleted": deleted,
            "deleted_bytes": deleted_bytes,
            "ttl_hours": ttl_hours,
        });
        if let Some(list) = deleted_items {
            payload.as_object_mut().expect("payload object").insert(
                "deleted_items".into(),
                serde_json::to_value(list).unwrap_or(Value::Null),
            );
        }
        self.bus.publish(topics::TOPIC_MODELS_CAS_GC, &payload);
        Ok(payload)
    }

    pub(super) async fn hashes_page_internal(
        &self,
        limit: usize,
        offset: usize,
        provider: Option<String>,
        model: Option<String>,
        sort: Option<String>,
        order: Option<String>,
    ) -> HashPage {
        let index = self.manifest_hash_index().await;
        let mut rows: Vec<HashItem> = index
            .iter()
            .map(|(sha256, refs)| refs.to_hash_item(sha256))
            .collect();
        if let Some(filter) = provider.as_ref() {
            rows.retain(|row| row.providers.iter().any(|p| p == filter));
        }
        if let Some(filter) = model.as_ref() {
            rows.retain(|row| row.models.iter().any(|m| m == filter));
        }
        let sort_key = sort
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "bytes".to_string());
        let desc_default = sort_key == "bytes";
        let desc = match order.as_deref() {
            Some("asc") => false,
            Some("desc") => true,
            _ => desc_default,
        };
        rows.sort_by(|a, b| {
            let ord = match sort_key.as_str() {
                "sha256" => a.sha256.cmp(&b.sha256),
                "path" => a.path.cmp(&b.path),
                "providers_count" => a.providers.len().cmp(&b.providers.len()),
                _ => a.bytes.cmp(&b.bytes),
            };
            if desc {
                ord.reverse()
            } else {
                ord
            }
        });
        let total = rows.len();
        let limit = limit.clamp(1, 10_000);
        let pages = if total == 0 {
            0
        } else {
            ((total - 1) / limit) + 1
        };
        let max_offset = if pages == 0 { 0 } else { (pages - 1) * limit };
        let offset = if total == 0 {
            0
        } else {
            offset.min(max_offset)
        };
        let end = offset.saturating_add(limit).min(total);
        let slice = rows[offset..end].to_vec();
        let count = end.saturating_sub(offset);
        let page = if pages == 0 { 0 } else { (offset / limit) + 1 };
        let prev_offset = if page <= 1 {
            None
        } else {
            Some(offset.saturating_sub(limit))
        };
        let next_offset = if page == 0 || page >= pages {
            None
        } else {
            Some(end)
        };
        HashPage {
            items: slice,
            total,
            count,
            limit,
            offset,
            prev_offset,
            next_offset,
            page,
            pages,
            last_offset: max_offset,
        }
    }
}
