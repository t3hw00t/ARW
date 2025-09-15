use axum::response::IntoResponse;
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{state_dir, AppState};

#[derive(Deserialize)]
pub(crate) struct AssembleReq {
    #[serde(default)]
    pub proj: Option<String>,
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub k: Option<usize>,
}

pub async fn context_assemble(Json(req): Json<AssembleReq>) -> impl IntoResponse {
    let proj = req.proj.clone();
    let query = req.q.clone().unwrap_or_default();
    let k = req.k.unwrap_or(12).max(1).min(100);
    let proj_clone = proj.clone();
    let query_clone = query.clone();
    let (items, scanned) = match tokio::task::spawn_blocking(move || {
        scan_files_for_query(&proj_clone, &query_clone, k)
    })
    .await
    {
        Ok(res) => res,
        Err(_) => (Vec::new(), 0),
    };
    Json(json!({
        "proj": proj,
        "query": query,
        "k": k,
        "beliefs": items,
        "counts": {"beliefs": k},
        "coverage": {"scanned": scanned, "hits": k.min(scanned)}
    }))
}

fn scan_files_for_query(proj: &Option<String>, q: &str, k: usize) -> (Vec<Value>, usize) {
    use std::fs;
    use std::io::{BufRead, BufReader};
    let mut out: Vec<Value> = Vec::new();
    let mut scanned = 0usize;
    let base = state_dir().join("projects");
    let root = match proj {
        Some(p) => base.join(p),
        None => base.clone(),
    };
    let limit_files: usize = std::env::var("ARW_CONTEXT_SCAN_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut stack: Vec<std::path::PathBuf> = vec![root];
    while let Some(path) = stack.pop() {
        if out.len() >= k {
            break;
        }
        if let Ok(meta) = fs::metadata(&path) {
            if meta.is_dir() {
                if let Ok(rd) = fs::read_dir(&path) {
                    for ent in rd.flatten() {
                        stack.push(ent.path());
                    }
                }
            } else if meta.is_file() {
                scanned += 1;
                if scanned > limit_files {
                    break;
                }
                if meta.len() > 512 * 1024 {
                    continue;
                }
                let pstr = path.to_string_lossy().to_string();
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    let ext_low = ext.to_ascii_lowercase();
                    let ok = matches!(
                        ext_low.as_str(),
                        "md" | "txt" | "rs" | "py" | "js" | "ts" | "json" | "yaml" | "yml" | "toml"
                    );
                    if !ok {
                        continue;
                    }
                }
                if let Ok(f) = fs::File::open(&path) {
                    let reader = BufReader::new(f);
                    for (i, line) in reader.lines().flatten().enumerate().take(2000) {
                        if out.len() >= k {
                            break;
                        }
                        if q.is_empty()
                            || line.to_ascii_lowercase().contains(&q.to_ascii_lowercase())
                        {
                            let excerpt = line.chars().take(240).collect::<String>();
                            let id = format!("file::{}#{}", pstr, i + 1);
                            out.push(json!({
                                "id": id,
                                "text": excerpt,
                                "ptr": {"kind":"file", "path": pstr}
                            }));
                        }
                    }
                }
            }
        }
    }
    (out, scanned)
}

#[derive(Deserialize)]
pub(crate) struct RehydrateReq {
    pub ptr: Value,
}
pub async fn context_rehydrate(
    State(state): State<AppState>,
    Json(req): Json<RehydrateReq>,
) -> impl IntoResponse {
    let kind = req.ptr.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "file" => {
            if !state
                .policy
                .lock()
                .await
                .evaluate_action("context.rehydrate")
                .allow
            {
                if state
                    .kernel
                    .find_valid_lease("local", "context:rehydrate:file")
                    .ok()
                    .flatten()
                    .is_none()
                    && state
                        .kernel
                        .find_valid_lease("local", "fs")
                        .ok()
                        .flatten()
                        .is_none()
                {
                    state.bus.publish(
                        "policy.decision",
                        &json!({
                            "action": "context.rehydrate",
                            "allow": false,
                            "require_capability": "context:rehydrate:file|fs",
                            "explain": {"reason":"lease_required"}
                        }),
                    );
                    return (
                        axum::http::StatusCode::FORBIDDEN,
                        Json(
                            json!({"type":"about:blank","title":"Forbidden","status":403, "detail":"Lease required: context:rehydrate:file or fs"}),
                        ),
                    );
                }
            }
            let path = match req.ptr.get("path").and_then(|v| v.as_str()) {
                Some(s) => std::path::PathBuf::from(s),
                None => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        Json(
                            json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"missing path"}),
                        ),
                    );
                }
            };
            let cap_kb: u64 = std::env::var("ARW_REHYDRATE_FILE_HEAD_KB")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(64);
            match tokio::fs::metadata(&path).await {
                Ok(m) if m.is_file() => {
                    let take = std::cmp::min(m.len(), cap_kb * 1024);
                    let f = match tokio::fs::File::open(&path).await {
                        Ok(f) => f,
                        Err(_) => {
                            return (
                                axum::http::StatusCode::NOT_FOUND,
                                Json(
                                    json!({"type":"about:blank","title":"Not Found","status":404}),
                                ),
                            );
                        }
                    };
                    let mut buf = vec![0u8; take as usize];
                    use tokio::io::AsyncReadExt as _;
                    use tokio::io::BufReader as TokioBufReader;
                    let mut br = TokioBufReader::new(f);
                    let n = br.read(&mut buf).await.unwrap_or(0);
                    let content = String::from_utf8_lossy(&buf[..n]).to_string();
                    (
                        axum::http::StatusCode::OK,
                        Json(
                            json!({"ptr": req.ptr, "file": {"path": path.to_string_lossy(), "size": m.len(), "head_bytes": n as u64, "truncated": (m.len() as usize) > n }, "content": content}),
                        ),
                    )
                }
                _ => (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(
                        json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"not a file"}),
                    ),
                ),
            }
        }
        _ => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(
                json!({"type":"about:blank","title":"Bad Request","status":400, "detail":"unsupported ptr kind"}),
            ),
        ),
    }
}
