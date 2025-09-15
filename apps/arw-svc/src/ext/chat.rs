use crate::AppState;
use arw_macros::arw_admin;
use axum::{extract::Query, extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

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
    // Read mode-driven planner hints (verify/self-consistency)
    let hints = { super::hints().read().await.clone() };
    let mode = hints
        .mode
        .as_deref()
        .unwrap_or("balanced")
        .to_ascii_lowercase();
    let (verify_pass, mut vote_k) = match mode.as_str() {
        "quick" => (false, 0u8),
        "deep" => (false, 5u8),
        "verified" => (true, 3u8),
        _ => (false, 3u8),
    };
    // Optional override from policy hints
    if let Some(v) = hints.vote_k {
        vote_k = v;
    }
    let model = req.model.clone().unwrap_or_else(|| "echo".to_string());
    let user = json!({"role":"user","content": req.message});
    // Self-consistency (vote-k) if gated and requested
    let reply_txt: String;
    if vote_k > 1 && arw_core::gating::allowed(arw_core::gating_keys::CHAT_SELF_CONSISTENCY) {
        let n = vote_k as usize;
        let mut votes: Vec<String> = Vec::with_capacity(n);
        for _i in 0..n {
            if let Some(r) = llama_reply(&req.message, req.temperature.or(Some(0.8))).await {
                votes.push(r);
            } else if let Some(r) = openai_reply(&req.message, req.temperature.or(Some(0.8))).await
            {
                votes.push(r);
            } else {
                votes.push(synth_reply(&req.message, &model));
            }
        }
        // Tally by normalized string
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for v in &votes {
            let key = v.trim().to_string();
            *counts.entry(key).or_insert(0) += 1;
        }
        let mut best: (&String, usize) = (&votes[0], 1);
        for (k, c) in counts.iter() {
            if *c > best.1 {
                best = (k, *c);
            }
        }
        reply_txt = best.0.clone();
    } else {
        reply_txt = if let Some(r) = llama_reply(&req.message, req.temperature).await {
            r
        } else if let Some(r) = openai_reply(&req.message, req.temperature).await {
            r
        } else {
            synth_reply(&req.message, &model)
        };
    }
    let mut assist = json!({"role":"assistant","content": reply_txt, "model": model});
    // Attach planner metadata (advisory)
    if let Some(obj) = assist.as_object_mut() {
        obj.insert(
            "planner".into(),
            json!({"mode": mode, "verify_pass": verify_pass, "consistency": {"vote_k": vote_k}}),
        );
    }
    // Optional verifier pass (gated)
    if verify_pass && arw_core::gating::allowed(arw_core::gating_keys::CHAT_VERIFY) {
        let question = req.message.clone();
        let answer = assist
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(obj) = assist.as_object_mut() {
            let (status, note) = verify_with_backend(&question, &answer, req.temperature).await;
            obj.insert("verify".into(), json!({"status": status, "note": note}));
        }
    }
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
    if let Some(cid) = in_evt.get("corr_id").cloned() {
        out_evt
            .as_object_mut()
            .unwrap()
            .insert("corr_id".into(), cid);
    }
    state
        .bus
        .publish(crate::ext::topics::TOPIC_CHAT_MESSAGE, &in_evt);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_CHAT_MESSAGE, &out_evt);
    // Emit a lightweight planner hint event for UI/recipes
    let mut hint_evt =
        json!({"mode": mode, "verify_pass": verify_pass, "consistency": {"vote_k": vote_k}});
    crate::ext::corr::ensure_corr(&mut hint_evt);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_CHAT_PLANNER, &hint_evt);
    super::ok(assist).into_response()
}

#[derive(Deserialize)]
pub(crate) struct ChatStatusQs {
    #[serde(default)]
    pub probe: Option<bool>,
}
#[arw_admin(
    method = "GET",
    path = "/admin/chat/status",
    summary = "Chat backend status"
)]
pub(crate) async fn chat_status(
    State(state): State<AppState>,
    Query(q): Query<ChatStatusQs>,
) -> impl IntoResponse {
    let backend = if std::env::var("ARW_LLAMA_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        "llama"
    } else if std::env::var("ARW_LITELLM_BASE_URL").ok().filter(|s| !s.trim().is_empty()).is_some()
        || std::env::var("ARW_OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
    {
        "openai-compatible"
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
            "openai" => match openai_reply("ping", None).await {
                Some(_) => ("openai", true, None::<String>),
                None => ("openai", false, Some("no reply".into())),
            },
            _ => ("synthetic", true, None::<String>),
        };
        let dt = t0.elapsed().as_millis() as u64;
        let mut payload =
            json!({"backend": backend, "probe_ok": ok, "latency_ms": dt, "error": err});
        crate::ext::corr::ensure_corr(&mut payload);
        state
            .bus
            .publish(crate::ext::topics::TOPIC_CHAT_PROBE, &payload);
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
        "n_predict": 128,
        // Enable llama.cpp prompt cache for KV/prefix reuse
        "cache_prompt": true
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
    // Support OpenAI-compatible proxies (e.g., LiteLLM) in addition to OpenAI proper.
    // Precedence: explicit LiteLLM vars -> OpenAI vars -> defaults.
    let litellm_base = std::env::var("ARW_LITELLM_BASE_URL").ok();
    let litellm_key = std::env::var("ARW_LITELLM_API_KEY").ok();
    let litellm_model = std::env::var("ARW_LITELLM_MODEL").ok();

    let base = litellm_base
        .or_else(|| std::env::var("ARW_OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com".to_string());
    let key = litellm_key
        .or_else(|| std::env::var("ARW_OPENAI_API_KEY").ok())
        .unwrap_or_default();
    let model = litellm_model
        .or_else(|| std::env::var("ARW_OPENAI_MODEL").ok())
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
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
    let mut req = client.post(url).json(&body);
    if !key.trim().is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.ok()?;
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

// Simple verifier: prompt the backend to judge the answer; returns (status, note)
async fn verify_with_backend(
    question: &str,
    answer: &str,
    temperature: Option<f64>,
) -> (String, String) {
    let prompt = format!("Verifier:\nQ: {}\nA: {}\nInstructions: Evaluate the answer for factuality and relevance. Respond with one of:\n- OK: <short reason>\n- NEEDS_REVIEW: <short reason>\n", question, answer);
    let resp = if let Some(r) = llama_reply(&prompt, temperature.or(Some(0.0))).await {
        Some(r)
    } else {
        openai_reply(
            &format!("You are a verifier. {}", prompt),
            temperature.or(Some(0.0)),
        )
        .await
    };
    if let Some(s) = resp {
        let up = s.to_ascii_uppercase();
        if up.starts_with("OK:") || up.trim() == "OK" {
            return ("ok".into(), s.trim().to_string());
        }
        if up.starts_with("NEEDS_REVIEW") || up.starts_with("REVIEW") {
            return ("needs_review".into(), s.trim().to_string());
        }
        return ("unknown".into(), s.trim().to_string());
    }
    ("ok".into(), "synthetic verifier".into())
}
