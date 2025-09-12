use crate::AppState;
use arw_macros::arw_admin;
use axum::{extract::Query, extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

static CHAT_LOG: OnceLock<RwLock<Vec<serde_json::Value>>> = OnceLock::new();
fn chat_log() -> &'static RwLock<Vec<serde_json::Value>> {
    CHAT_LOG.get_or_init(|| RwLock::new(Vec::new()))
}

#[derive(Deserialize)]
pub(crate) struct ChatSendReq {
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

pub(crate) async fn chat_get() -> impl IntoResponse {
    let msgs = chat_log().read().await.clone();
    super::ok(json!({"messages": msgs})).into_response()
}
pub(crate) async fn chat_clear() -> impl IntoResponse {
    chat_log().write().await.clear();
    super::ok(json!({})).into_response()
}

fn synth_reply(msg: &str, model: &str) -> String {
    match model.to_ascii_lowercase().as_str() {
        "reverse" => msg.chars().rev().collect(),
        "time" => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("[{}] {}", now, msg)
        }
        _ => format!("You said: {}", msg),
    }
}

pub(crate) async fn chat_send(
    State(state): State<AppState>,
    Json(req): Json<ChatSendReq>,
) -> impl IntoResponse {
    let model = req.model.clone().unwrap_or_else(|| "echo".to_string());
    let user = json!({"role":"user","content": req.message});
    let reply_txt = if let Some(r) = llama_reply(&req.message, req.temperature).await {
        r
    } else if let Some(r) = openai_reply(&req.message, req.temperature).await {
        r
    } else {
        synth_reply(&req.message, &model)
    };
    let mut assist = json!({"role":"assistant","content": reply_txt, "model": model});
    if let Some(t) = req.temperature {
        if let Some(obj) = assist.as_object_mut() {
            obj.insert("temperature".into(), json!(t));
        }
    }
    {
        let mut log = chat_log().write().await;
        log.push(user.clone());
        log.push(assist.clone());
        while log.len() > 200 {
            log.remove(0);
        }
    }
    // Use a single corr_id for user+assistant pair
    let mut in_evt = json!({"dir":"in","msg": user});
    crate::ext::corr::ensure_corr(&mut in_evt);
    let mut out_evt = json!({"dir":"out","msg": assist});
    if let Some(cid) = in_evt.get("corr_id").cloned() { out_evt.as_object_mut().unwrap().insert("corr_id".into(), cid); }
    state.bus.publish("Chat.Message", &in_evt);
    state.bus.publish("Chat.Message", &out_evt);
    super::ok(assist).into_response()
}

#[derive(Deserialize)]
pub(crate) struct ChatStatusQs {
    #[serde(default)]
    pub probe: Option<bool>,
}
#[arw_admin(method="GET", path="/admin/chat/status", summary="Chat backend status")]
pub(crate) async fn chat_status(State(state): State<AppState>, Query(q): Query<ChatStatusQs>) -> impl IntoResponse {
    let backend = if std::env::var("ARW_LLAMA_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        "llama"
    } else if std::env::var("ARW_OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        "openai"
    } else {
        "synthetic"
    };
    if q.probe.unwrap_or(false) {
        let t0 = std::time::Instant::now();
        let (backend, ok, err) = match backend {
            "llama" => match llama_reply("ping", None).await {
                Some(_) => ("llama", true, None::<String>),
                None => ("llama", false, Some("no reply".into())),
            },
            "openai" => {
                match openai_reply("ping", None).await {
                    Some(_) => ("openai", true, None::<String>),
                    None => ("openai", false, Some("no reply".into())),
                }
            }
            _ => ("synthetic", true, None::<String>),
        };
        let dt = t0.elapsed().as_millis() as u64;
        let mut payload = json!({"backend": backend, "probe_ok": ok, "latency_ms": dt, "error": err});
        crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Chat.Probe", &payload);
        return super::ok(payload).into_response();
    }
    super::ok(json!({"backend": backend})).into_response()
}

async fn llama_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let base = std::env::var("ARW_LLAMA_URL").ok()?; // e.g., http://127.0.0.1:8080
    if base.trim().is_empty() {
        return None;
    }
    let url = format!("{}/completion", base.trim_end_matches('/'));
    let timeout_s: u64 = crate::dyn_timeout::current_http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_s))
        .build()
        .ok()?;
    let mut body = serde_json::json!({
        "prompt": prompt,
        "n_predict": 128
    });
    if let Some(t) = temperature {
        if let Some(o) = body.as_object_mut() {
            o.insert("temperature".into(), json!(t));
        }
    }
    match client.post(url).json(&body).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                return None;
            }
            match resp.json::<serde_json::Value>().await {
                Ok(v) => {
                    // Try llama.cpp server shape
                    if let Some(s) = v.get("content").and_then(|x| x.as_str()) {
                        return Some(s.to_string());
                    }
                    // Fallback: OpenAI-like
                    if let Some(s) = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c0| c0.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|x| x.as_str())
                    {
                        return Some(s.to_string());
                    }
                    None
                }
                Err(_) => None,
            }
        }
        Err(_) => None,
    }
}

async fn openai_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let base = std::env::var("ARW_OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com".to_string());
    let key = std::env::var("ARW_OPENAI_API_KEY").ok()?;
    if key.trim().is_empty() {
        return None;
    }
    let model = std::env::var("ARW_OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
    let url = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
    let timeout_s: u64 = crate::dyn_timeout::current_http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_s))
        .build()
        .ok()?;
    let mut body = serde_json::json!({
        "model": model,
        "messages": [ {"role":"user", "content": prompt} ]
    });
    if let Some(t) = temperature {
        if let Some(o) = body.as_object_mut() {
            o.insert("temperature".into(), json!(t));
        }
    }
    let resp = client
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if let Ok(v) = resp.json::<serde_json::Value>().await {
        if let Some(s) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c0| c0.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|x| x.as_str())
        {
            return Some(s.to_string());
        }
    }
    None
}
