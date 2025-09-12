use serde_json::{json, Value};

use crate::app_state::AppState;
use futures_util::StreamExt;
use once_cell::sync::OnceCell;
use std::collections::HashSet;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct ModelsService;

impl ModelsService {
    pub fn new() -> Self {
        Self
    }

    pub async fn list(&self) -> Vec<Value> {
        crate::ext::models().read().await.clone()
    }

    pub async fn refresh(&self, state: &AppState) -> Vec<Value> {
        let new = super::super::ext::default_models();
        {
            let mut m = crate::ext::models().write().await;
            *m = new.clone();
        }
        let _ = super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(new.clone()),
        )
        .await;
        state
            .bus
            .publish("Models.Refreshed", &json!({"count": new.len()}));
        new
    }

    pub async fn save(&self) -> Result<(), String> {
        let v = crate::ext::models().read().await.clone();
        super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(v),
        )
        .await
        .map_err(|e| e.to_string())
    }

    pub async fn load(&self) -> Result<Vec<Value>, String> {
        match super::super::ext::io::load_json_file_async(&super::super::ext::paths::models_path())
            .await
            .and_then(|v| v.as_array().cloned())
        {
            Some(arr) => {
                {
                    let mut m = crate::ext::models().write().await;
                    *m = arr.clone();
                }
                Ok(arr)
            }
            None => Err("no models.json".into()),
        }
    }

    pub async fn add(&self, state: &AppState, id: String, provider: Option<String>) {
        let mut v = crate::ext::models().write().await;
        if !v
            .iter()
            .any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
        {
            v.push(json!({"id": id, "provider": provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
            state.bus.publish(
                "Models.Changed",
                &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}),
            );
            // audit
            super::super::ext::io::audit_event("models.add", &json!({"id": v.last().and_then(|m| m.get("id")).cloned() })).await;
        }
    }

    pub async fn delete(&self, state: &AppState, id: String) {
        let mut v = crate::ext::models().write().await;
        let before = v.len();
        v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&id));
        if v.len() != before {
            state
                .bus
                .publish("Models.Changed", &json!({"op":"delete","id": id}));
            super::super::ext::io::audit_event("models.delete", &json!({"id": id})).await;
        }
    }

    pub async fn default_get(&self) -> String {
        crate::ext::default_model().read().await.clone()
    }

    pub async fn default_set(&self, state: &AppState, id: String) -> Result<(), String> {
        {
            let mut d = crate::ext::default_model().write().await;
            *d = id.clone();
        }
        state
            .bus
            .publish("Models.Changed", &json!({"op":"default","id": id}));
        let res = super::super::ext::io::save_json_file_async(
            &super::super::ext::paths::models_path(),
            &Value::Array(crate::ext::models().read().await.clone()),
        )
        .await
        .map_err(|e| e.to_string());
        if res.is_ok() {
            super::super::ext::io::audit_event("models.default", &json!({"id": id})).await;
        }
        res
    }

    // ---- Download worker ----
    fn cancel_cell() -> &'static RwLock<HashSet<String>> {
        static DL_CANCEL: OnceCell<RwLock<HashSet<String>>> = OnceCell::new();
        DL_CANCEL.get_or_init(|| RwLock::new(HashSet::new()))
    }
    async fn is_cancelled(id: &str) -> bool {
        Self::cancel_cell().read().await.contains(id)
    }
    async fn clear_cancel(id: &str) {
        Self::cancel_cell().write().await.remove(id);
    }
    async fn set_cancel(id: &str) {
        Self::cancel_cell().write().await.insert(id.to_string());
    }

    pub async fn cancel_download(&self, state: &AppState, id: String) {
        Self::set_cancel(&id).await;
        let mut p = json!({"id": id, "status":"cancel-requested"});
        crate::ext::corr::ensure_corr(&mut p);
        state.bus.publish("Models.DownloadProgress", &p);
        super::super::ext::io::audit_event("models.download.cancel", &p).await;
    }

    pub async fn download(
        &self,
        state: &AppState,
        id_in: String,
        url_in: String,
        provider_in: Option<String>,
        sha256_in: Option<String>,
    ) -> Result<(), String> {
        // ensure model exists with status
        let mut already_in_progress = false;
        {
            let mut v = crate::ext::models().write().await;
            if let Some(m) = v
                .iter_mut()
                .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id_in))
            {
                let prev = m.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if prev.eq_ignore_ascii_case("downloading") {
                    already_in_progress = true;
                } else {
                    *m = json!({"id": id_in, "provider": provider_in.clone().unwrap_or("local".into()), "status":"downloading"});
                }
            } else {
                v.push(json!({"id": id_in, "provider": provider_in.clone().unwrap_or("local".into()), "status":"downloading"}));
            }
        }
        if already_in_progress {
            let mut p = json!({"id": id_in, "status":"already-in-progress"});
            crate::ext::corr::ensure_corr(&mut p);
            state.bus.publish("Models.DownloadProgress", &p);
            return Ok(());
        }
        // Validate URL scheme
        if !(url_in.starts_with("http://") || url_in.starts_with("https://")) {
            return Err("invalid url scheme".into());
        }
        // Publish start
        {
            let mut p = json!({"id": id_in, "url": url_in});
            crate::ext::corr::ensure_corr(&mut p);
            state.bus.publish("Models.Download", &p);
            super::super::ext::io::audit_event("models.download", &p).await;
        }
        // Spawn worker
        let id = id_in.clone();
        let url = url_in.clone();
        let provider = provider_in.clone().unwrap_or("local".into());
        let expect_sha = sha256_in.clone().map(|s| s.to_lowercase());
        let sp = state.clone();
        tokio::spawn(async move {
            use sha2::Digest;
            use tokio::fs as afs;
            use tokio::io::AsyncWriteExt;
            // sanitize filename and compute target paths
            let file_name = url.rsplit('/').next().unwrap_or(&id).to_string();
            let safe_name = file_name.replace(['\\', '/'], "_");
            let target_dir = crate::ext::paths::state_dir().join("models");
            let target = target_dir.join(&safe_name);
            let tmp = target.with_extension("part");
            if let Err(e) = afs::create_dir_all(&target_dir).await {
                let mut p = json!({"id": id, "error": format!("mkdir failed: {}", e)});
                crate::ext::corr::ensure_corr(&mut p);
                sp.bus.publish("Models.DownloadProgress", &p);
                return;
            }
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let mut resume_from: u64 = 0;
            if let Ok(meta) = afs::metadata(&tmp).await {
                resume_from = meta.len();
            }
            let mut reqb = client.get(&url);
            if resume_from > 0 {
                reqb = reqb.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
            }
            match reqb.send().await {
                Ok(resp) => {
                    let total_rem = resp.content_length().unwrap_or(0);
                    let status = resp.status();
                    let total_all = if resume_from > 0
                        && status == axum::http::StatusCode::PARTIAL_CONTENT
                    {
                        let mut p = json!({"id": id, "status":"resumed", "offset": resume_from});
                        crate::ext::corr::ensure_corr(&mut p);
                        sp.bus.publish("Models.DownloadProgress", &p);
                        resume_from + total_rem
                    } else {
                        if resume_from > 0 {
                            let _ = afs::remove_file(&tmp).await;
                            resume_from = 0;
                        }
                        total_rem
                    };
                    let mut file = if resume_from > 0 {
                        match afs::OpenOptions::new().append(true).open(&tmp).await {
                            Ok(f) => f,
                            Err(e) => {
                                let mut p =
                                    json!({"id": id, "error": format!("open failed: {}", e)});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                                return;
                            }
                        }
                    } else {
                        match afs::File::create(&tmp).await {
                            Ok(f) => f,
                            Err(e) => {
                                let mut p =
                                    json!({"id": id, "error": format!("create failed: {}", e)});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                                return;
                            }
                        }
                    };
                    let mut downloaded: u64 = 0;
                    let mut stream = resp.bytes_stream();
                    while let Some(chunk) = stream.next().await {
                        match chunk {
                            Ok(bytes) => {
                                if Self::is_cancelled(&id).await {
                                    let _ = afs::remove_file(&tmp).await;
                                    let mut p = json!({"id": id, "status":"canceled"});
                                    crate::ext::corr::ensure_corr(&mut p);
                                    sp.bus.publish("Models.DownloadProgress", &p);
                                    Self::clear_cancel(&id).await;
                                    return;
                                }
                                if let Err(e) = file.write_all(&bytes).await {
                                    let mut p =
                                        json!({"id": id, "error": format!("write failed: {}", e)});
                                    crate::ext::corr::ensure_corr(&mut p);
                                    sp.bus.publish("Models.DownloadProgress", &p);
                                    return;
                                }
                                downloaded += bytes.len() as u64;
                                if total_all > 0 {
                                    let pct =
                                        (((resume_from + downloaded) * 100) / total_all).min(100);
                                    let mut p = json!({"id": id, "progress": pct, "downloaded": resume_from + downloaded, "total": total_all});
                                    crate::ext::corr::ensure_corr(&mut p);
                                    sp.bus.publish("Models.DownloadProgress", &p);
                                }
                            }
                            Err(e) => {
                                let mut p =
                                    json!({"id": id, "error": format!("read failed: {}", e)});
                                crate::ext::corr::ensure_corr(&mut p);
                                sp.bus.publish("Models.DownloadProgress", &p);
                                return;
                            }
                        }
                    }
                    if let Err(e) = file.flush().await {
                        let mut p = json!({"id": id, "error": format!("flush failed: {}", e)});
                        crate::ext::corr::ensure_corr(&mut p);
                        sp.bus.publish("Models.DownloadProgress", &p);
                        return;
                    }
                    if let Err(e) = afs::rename(&tmp, &target).await {
                        let mut p = json!({"id": id, "error": format!("finalize failed: {}", e)});
                        crate::ext::corr::ensure_corr(&mut p);
                        sp.bus.publish("Models.DownloadProgress", &p);
                        return;
                    }
                    if let Some(exp) = expect_sha {
                        let mut f = match afs::File::open(&target).await {
                            Ok(f) => f,
                            Err(_) => {
                                return;
                            }
                        };
                        let mut h = sha2::Sha256::new();
                        let mut buf = vec![0u8; 1024 * 1024];
                        loop {
                            match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
                                Ok(0) => break,
                                Ok(n) => {
                                    h.update(&buf[..n]);
                                }
                                Err(_) => break,
                            }
                        }
                        let actual = format!("{:x}", h.finalize());
                        if actual != exp {
                            let _ = afs::remove_file(&target).await;
                            let mut p = json!({"id": id, "error": "checksum mismatch", "expected": exp, "actual": actual});
                            crate::ext::corr::ensure_corr(&mut p);
                            sp.bus.publish("Models.DownloadProgress", &p);
                            return;
                        }
                    }
                    let mut p = json!({"id": id, "status":"complete", "file": safe_name, "provider": provider});
                    crate::ext::corr::ensure_corr(&mut p);
                    sp.bus.publish("Models.DownloadProgress", &p);
                    {
                        let mut v = crate::ext::models().write().await;
                        if let Some(m) = v
                            .iter_mut()
                            .find(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id))
                        {
                            *m = json!({"id": id, "provider": provider, "status":"available", "path": target.to_string_lossy()});
                        }
                    }
                    let _ = crate::ext::io::save_json_file_async(
                        &crate::ext::paths::models_path(),
                        &Value::Array(crate::ext::models().read().await.clone()),
                    )
                    .await;
                    sp.bus
                        .publish("Models.Changed", &json!({"op":"downloaded","id": id}));
                }
                Err(e) => {
                    let mut p = json!({"id": id, "error": format!("request failed: {}", e)});
                    crate::ext::corr::ensure_corr(&mut p);
                    sp.bus.publish("Models.DownloadProgress", &p);
                }
            }
        });
        Ok(())
    }
}
