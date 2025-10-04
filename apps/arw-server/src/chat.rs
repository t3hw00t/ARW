use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

use crate::{
    egress_log::{self, EgressRecord},
    tools::{self, ToolError},
    AppState,
};

use reqwest::header::CONTENT_TYPE;

const HISTORY_LIMIT: usize = 48;
const TOOL_HISTORY_LIMIT: usize = 12;

fn env_u64_in_range(key: &str, min: u64, max: u64) -> Option<u64> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<u64>()
        .ok()
        .map(|value| value.clamp(min, max))
}

fn env_f64_in_range(key: &str, min: f64, max: f64) -> Option<f64> {
    let raw = std::env::var(key).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(min, max))
}

fn env_list(key: &str) -> Option<Vec<String>> {
    let raw = std::env::var(key).ok()?;
    let items = raw
        .split([',', '\n'])
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect::<Vec<_>>();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn default_temperature() -> f64 {
    env_f64_in_range("ARW_CHAT_DEFAULT_TEMPERATURE", -5.0, 5.0).unwrap_or(0.2)
}

fn default_vote_k() -> usize {
    env_u64_in_range("ARW_CHAT_DEFAULT_VOTE_K", 1, 5)
        .map(|value| value as usize)
        .unwrap_or(1)
}

#[derive(Clone)]
pub struct ChatState {
    inner: Arc<Mutex<ChatLog>>,
}

#[derive(Default, Clone, Copy)]
pub struct ChatSendOptions {
    pub temperature: Option<f64>,
    pub vote_k: Option<usize>,
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChatLog::default())),
        }
    }

    pub async fn history(&self) -> Vec<ChatMessage> {
        let guard = self.inner.lock().await;
        guard.messages.clone()
    }

    pub async fn clear(&self) {
        let mut guard = self.inner.lock().await;
        guard.messages.clear();
        guard.backend = ChatBackend::default();
    }

    pub async fn send(
        &self,
        state: &AppState,
        prompt: &str,
        opts: ChatSendOptions,
    ) -> ChatSendOutcome {
        let mut guard = self.inner.lock().await;
        let user_ts = now_ms();
        let user = ChatMessage::new("user", prompt, user_ts);
        guard.messages.push(user.clone());
        prune_history(&mut guard.messages);
        let tool_history = recent_history(&guard.messages);
        drop(guard);

        let mut input = json!({
            "prompt": prompt,
            "history": tool_history
                .iter()
                .map(|m| json!({
                    "role": m.role,
                    "content": m.content,
                }))
                .collect::<Vec<Value>>(),
        });

        if let Some(temp) = opts.temperature {
            input["temperature"] = json!(temp);
        }
        if let Some(vote) = opts.vote_k {
            input["vote_k"] = json!(vote);
        }

        let tool_result = tools::run_tool(state, "chat.respond", input).await;

        let (reply_text, backend_label) = match tool_result {
            Ok(value) => {
                let backend = value
                    .get("backend")
                    .and_then(|v| v.as_str())
                    .unwrap_or("synthetic")
                    .to_string();
                let mut text = value
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no reply)")
                    .to_string();
                if text.trim().is_empty() {
                    text = "(empty reply)".to_string();
                }
                (text, backend)
            }
            Err(err) => {
                let bt = format!("(chat error: {})", err);
                (bt, "synthetic".to_string())
            }
        };

        let assistant = ChatMessage::new("assistant", &reply_text, now_ms());
        let mut guard = self.inner.lock().await;
        guard.messages.push(assistant.clone());
        guard.backend = ChatBackend::from_label(&backend_label);
        prune_history(&mut guard.messages);
        let backend_kind = guard.backend.kind().to_string();
        let history_snapshot = guard.messages.clone();
        drop(guard);

        ChatSendOutcome {
            backend: backend_kind,
            reply: assistant,
            history: history_snapshot,
        }
    }

    pub async fn status(&self, probe: bool) -> ChatStatus {
        let mut latency_ms = None;
        if probe {
            let start = std::time::Instant::now();
            // Simulate minimal work by taking the lock and cloning.
            let _ = self.history().await;
            latency_ms = Some(start.elapsed().as_millis() as u64);
        }
        let guard = self.inner.lock().await;
        ChatStatus {
            ok: true,
            backend: guard.backend.kind().to_string(),
            messages: guard.messages.len() as u64,
            latency_ms,
        }
    }
}

#[derive(Default)]
struct ChatLog {
    backend: ChatBackend,
    messages: Vec<ChatMessage>,
}

#[derive(Clone, Copy, Debug, Default)]
enum ChatBackend {
    #[default]
    Synthetic,
    Llama,
    OpenAi,
}

impl ChatBackend {
    fn kind(&self) -> &'static str {
        match self {
            ChatBackend::Synthetic => "synthetic",
            ChatBackend::Llama => "llama",
            ChatBackend::OpenAi => "openai",
        }
    }

    fn from_label(label: &str) -> Self {
        match label.to_ascii_lowercase().as_str() {
            "llama" => ChatBackend::Llama,
            "openai" => ChatBackend::OpenAi,
            _ => ChatBackend::Synthetic,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub ts_ms: u64,
}

impl ChatMessage {
    fn new(role: &str, content: &str, ts_ms: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: role.to_string(),
            content: content.to_string(),
            ts_ms,
        }
    }
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct ChatSendOutcome {
    pub backend: String,
    pub reply: ChatMessage,
    pub history: Vec<ChatMessage>,
}

#[derive(Clone, Serialize, utoipa::ToSchema)]
pub struct ChatStatus {
    pub ok: bool,
    pub backend: String,
    pub messages: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
}

fn now_ms() -> u64 {
    chrono::Utc::now()
        .timestamp_millis()
        .try_into()
        .unwrap_or_default()
}

fn prune_history(messages: &mut Vec<ChatMessage>) {
    if messages.len() > HISTORY_LIMIT {
        let drop_count = messages.len() - HISTORY_LIMIT;
        messages.drain(0..drop_count);
    }
}

fn recent_history(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    if messages.is_empty() {
        return Vec::new();
    }
    let len = messages.len().min(TOOL_HISTORY_LIMIT);
    messages[messages.len() - len..].to_vec()
}

#[derive(Clone, Debug)]
struct ToolHistoryEntry {
    role: String,
    content: String,
}

struct ChatToolInput {
    prompt: String,
    history: Vec<ToolHistoryEntry>,
    temperature: f64,
    vote_k: usize,
}

struct ChatEngineResult {
    backend: ChatBackend,
    text: String,
}

pub async fn run_chat_tool(state: &AppState, value: Value) -> Result<Value, ToolError> {
    let input = ChatToolInput::from_value(value)?;
    let start = Instant::now();
    let result = generate_chat_reply(state, &input).await?;
    let latency_ms = start.elapsed().as_millis() as u64;
    Ok(json!({
        "text": result.text,
        "backend": result.backend.kind(),
        "latency_ms": latency_ms,
    }))
}

impl ChatToolInput {
    fn from_value(value: Value) -> Result<Self, ToolError> {
        let prompt = value
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ToolError::Invalid("prompt required".into()))?
            .to_string();

        let mut history = Vec::new();
        if let Some(items) = value.get("history").and_then(|v| v.as_array()) {
            let start = items.len().saturating_sub(TOOL_HISTORY_LIMIT);
            for item in items.iter().skip(start) {
                let role = match item.get("role").and_then(|v| v.as_str()) {
                    Some(r) => r,
                    None => continue,
                };
                let content = match item.get("content").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => continue,
                };
                history.push(ToolHistoryEntry {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            }
        }

        let temperature = value
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or_else(default_temperature)
            .clamp(-5.0, 5.0);

        let vote_k = value
            .get("vote_k")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or_else(default_vote_k)
            .clamp(1, 5);

        Ok(Self {
            prompt,
            history,
            temperature,
            vote_k,
        })
    }
}

async fn generate_chat_reply(
    state: &AppState,
    input: &ChatToolInput,
) -> Result<ChatEngineResult, ToolError> {
    let attempts = input.vote_k.max(1);
    let mut best: Option<ChatEngineResult> = None;
    let mut best_score = 0usize;

    for _ in 0..attempts {
        let candidate = if let Some(res) = llama_chat(state, input).await? {
            Some(res)
        } else if let Some(res) = openai_chat(state, input).await? {
            Some(res)
        } else {
            Some(ChatEngineResult {
                backend: ChatBackend::Synthetic,
                text: synth_reply(&input.prompt),
            })
        };

        if let Some(res) = candidate {
            let score = res.text.split_whitespace().count();
            if score > best_score || best.is_none() {
                best_score = score;
                best = Some(res);
            }
        }
    }

    Ok(best.unwrap_or(ChatEngineResult {
        backend: ChatBackend::Synthetic,
        text: synth_reply(&input.prompt),
    }))
}

fn synth_reply(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        "(no input)".to_string()
    } else {
        format!("You said: {}", trimmed)
    }
}

fn render_prompt(history: &[ToolHistoryEntry]) -> String {
    if history.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for entry in history {
        let role = match entry.role.to_ascii_lowercase().as_str() {
            "assistant" => "Assistant",
            "system" => "System",
            _ => "User",
        };
        out.push_str(role);
        out.push_str(": ");
        out.push_str(entry.content.trim());
        out.push('\n');
    }
    out.push_str("Assistant:");
    out
}

fn normalize_role(role: &str) -> &'static str {
    match role.to_ascii_lowercase().as_str() {
        "assistant" => "assistant",
        "system" => "system",
        "tool" => "assistant",
        _ => "user",
    }
}

async fn llama_chat(
    state: &AppState,
    input: &ChatToolInput,
) -> Result<Option<ChatEngineResult>, ToolError> {
    let base = match std::env::var("ARW_LLAMA_URL") {
        Ok(val) if !val.trim().is_empty() => val,
        _ => return Ok(None),
    };
    let url = format!("{}/completion", base.trim_end_matches('/'));
    let prompt = render_prompt(&input.history);
    let mut body = json!({
        "prompt": format!("{}\nUser: {}\nAssistant:", prompt, input.prompt),
        "n_predict": 256,
        "cache_prompt": true,
    });
    if let Some(obj) = body.as_object_mut() {
        obj.insert("temperature".into(), json!(input.temperature));
        if let Some(n_predict) = env_u64_in_range("ARW_LLAMA_N_PREDICT", 1, 8192) {
            obj.insert("n_predict".into(), json!(n_predict));
        }
        if let Some(top_p) = env_f64_in_range("ARW_LLAMA_TOP_P", 0.0, 1.0) {
            obj.insert("top_p".into(), json!(top_p));
        }
        if let Some(top_k) = env_u64_in_range("ARW_LLAMA_TOP_K", 1, 5000) {
            obj.insert("top_k".into(), json!(top_k));
        }
        if let Some(min_p) = env_f64_in_range("ARW_LLAMA_MIN_P", 0.0, 1.0) {
            obj.insert("min_p".into(), json!(min_p));
        }
        if let Some(repeat_penalty) = env_f64_in_range("ARW_LLAMA_REPEAT_PENALTY", 0.0, 4.0) {
            obj.insert("repeat_penalty".into(), json!(repeat_penalty));
        }
        if let Some(stop) = env_list("ARW_LLAMA_STOP") {
            obj.insert("stop".into(), json!(stop));
        }
    }

    let client = crate::http_client::client().clone();
    let payload = match serde_json::to_vec(&body) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(ToolError::Runtime(format!(
                "failed to encode llama payload: {}",
                err
            )))
        }
    };
    let payload_len = payload.len() as i64;
    let request = client
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .body(payload);

    match request.send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.bytes().await {
                Ok(bytes) => {
                    let bytes_len = bytes.len() as i64;
                    if status.is_success() {
                        let reply = serde_json::from_slice::<Value>(&bytes)
                            .ok()
                            .and_then(|v| {
                                v.get("content")
                                    .and_then(|c| c.as_str())
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| synth_reply(&input.prompt));
                        record_egress(
                            state,
                            &url,
                            "allow",
                            None,
                            Some(payload_len),
                            Some(bytes_len),
                        )
                        .await;
                        if reply.trim().is_empty() {
                            return Ok(Some(ChatEngineResult {
                                backend: ChatBackend::Synthetic,
                                text: synth_reply(&input.prompt),
                            }));
                        }
                        Ok(Some(ChatEngineResult {
                            backend: ChatBackend::Llama,
                            text: reply,
                        }))
                    } else {
                        record_egress(
                            state,
                            &url,
                            "deny",
                            Some(status.as_str()),
                            Some(payload_len),
                            Some(bytes_len),
                        )
                        .await;
                        Ok(None)
                    }
                }
                Err(err) => {
                    record_egress(
                        state,
                        &url,
                        "deny",
                        Some("read_error"),
                        Some(payload_len),
                        None,
                    )
                    .await;
                    Err(ToolError::Runtime(err.to_string()))
                }
            }
        }
        Err(err) => {
            record_egress(
                state,
                &url,
                "deny",
                Some("network"),
                Some(payload_len),
                None,
            )
            .await;
            Err(ToolError::Runtime(err.to_string()))
        }
    }
}

async fn openai_chat(
    state: &AppState,
    input: &ChatToolInput,
) -> Result<Option<ChatEngineResult>, ToolError> {
    let raw_key = std::env::var("ARW_LITELLM_API_KEY")
        .or_else(|_| std::env::var("ARW_OPENAI_API_KEY"))
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let key = match raw_key {
        Some(k) => k,
        None => return Ok(None),
    };
    let base_url = std::env::var("ARW_LITELLM_BASE_URL")
        .or_else(|_| std::env::var("ARW_OPENAI_BASE_URL"))
        .unwrap_or_else(|_| "https://api.openai.com".into());
    let base_url = base_url.trim().trim_end_matches('/');
    let api_url = format!("{}/v1/chat/completions", base_url);
    let model = std::env::var("ARW_LITELLM_MODEL")
        .or_else(|_| std::env::var("ARW_OPENAI_MODEL"))
        .unwrap_or_else(|_| "gpt-4o-mini".into());
    let system_prompt = std::env::var("ARW_CHAT_SYSTEM_PROMPT")
        .unwrap_or_else(|_| "You are a helpful assistant.".into());

    let mut messages = Vec::new();
    messages.push(json!({"role": "system", "content": system_prompt}));
    for entry in &input.history {
        let role = normalize_role(&entry.role);
        messages.push(json!({"role": role, "content": entry.content}));
    }
    messages.push(json!({"role": "user", "content": input.prompt.clone()}));

    let mut body = json!({
        "model": model,
        "messages": messages,
        "temperature": input.temperature,
        "max_tokens": 512,
    });
    if let Some(obj) = body.as_object_mut() {
        if let Some(max_tokens) = env_u64_in_range("ARW_OPENAI_MAX_TOKENS", 16, 4096) {
            obj.insert("max_tokens".into(), json!(max_tokens));
        }
        if let Some(top_p) = env_f64_in_range("ARW_OPENAI_TOP_P", 0.0, 1.0) {
            obj.insert("top_p".into(), json!(top_p));
        }
        if let Some(freq) = env_f64_in_range("ARW_OPENAI_FREQUENCY_PENALTY", -2.0, 2.0) {
            obj.insert("frequency_penalty".into(), json!(freq));
        }
        if let Some(presence) = env_f64_in_range("ARW_OPENAI_PRESENCE_PENALTY", -2.0, 2.0) {
            obj.insert("presence_penalty".into(), json!(presence));
        }
        if let Some(stop) = env_list("ARW_OPENAI_STOP") {
            obj.insert("stop".into(), json!(stop));
        }
    }

    let client = crate::http_client::client().clone();
    let payload = match serde_json::to_vec(&body) {
        Ok(bytes) => bytes,
        Err(err) => {
            return Err(ToolError::Runtime(format!(
                "failed to encode OpenAI payload: {}",
                err
            )))
        }
    };
    let payload_len = payload.len() as i64;
    let request = client
        .post(&api_url)
        .bearer_auth(&key)
        .header(CONTENT_TYPE, "application/json")
        .body(payload);
    match request.send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.bytes().await {
                Ok(bytes) => {
                    let bytes_len = bytes.len() as i64;
                    if status.is_success() {
                        let reply = serde_json::from_slice::<Value>(&bytes)
                            .ok()
                            .and_then(|v| {
                                v.get("choices")
                                    .and_then(|c| c.as_array())
                                    .and_then(|arr| arr.first())
                                    .and_then(|choice| {
                                        choice
                                            .get("message")
                                            .and_then(|m| m.get("content"))
                                            .and_then(|c| c.as_str())
                                    })
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| synth_reply(&input.prompt));
                        record_egress(
                            state,
                            &api_url,
                            "allow",
                            None,
                            Some(payload_len),
                            Some(bytes_len),
                        )
                        .await;
                        if reply.trim().is_empty() {
                            return Ok(Some(ChatEngineResult {
                                backend: ChatBackend::Synthetic,
                                text: synth_reply(&input.prompt),
                            }));
                        }
                        Ok(Some(ChatEngineResult {
                            backend: ChatBackend::OpenAi,
                            text: reply,
                        }))
                    } else {
                        record_egress(
                            state,
                            &api_url,
                            "deny",
                            Some(status.as_str()),
                            Some(payload_len),
                            Some(bytes_len),
                        )
                        .await;
                        Ok(None)
                    }
                }
                Err(err) => {
                    record_egress(
                        state,
                        &api_url,
                        "deny",
                        Some("read_error"),
                        Some(payload_len),
                        None,
                    )
                    .await;
                    Err(ToolError::Runtime(err.to_string()))
                }
            }
        }
        Err(err) => {
            record_egress(
                state,
                &api_url,
                "deny",
                Some("network"),
                Some(payload_len),
                None,
            )
            .await;
            Err(ToolError::Runtime(err.to_string()))
        }
    }
}

async fn record_egress(
    state: &AppState,
    url: &str,
    decision: &'static str,
    reason: Option<&str>,
    bytes_out: Option<i64>,
    bytes_in: Option<i64>,
) {
    let parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => return,
    };
    let host_owned = parsed.host_str().map(|s| s.to_string());
    let port = parsed.port().map(|p| p as i64);
    let protocol_owned = parsed.scheme().to_string();
    let corr_id_owned = uuid::Uuid::new_v4().to_string();

    let record = EgressRecord {
        decision,
        reason,
        dest_host: host_owned.as_deref(),
        dest_port: port,
        protocol: Some(protocol_owned.as_str()),
        bytes_in,
        bytes_out,
        corr_id: Some(corr_id_owned.as_str()),
        project: None,
        meta: None,
    };

    egress_log::record(
        state.kernel_if_enabled(),
        &state.bus(),
        None,
        &record,
        false,
        true,
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_support::env, AppState};
    use arw_policy::PolicyEngine;
    use arw_topics as topics;
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::{future::IntoFuture, sync::Arc};
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;
    use tokio::time::{sleep, Duration};

    #[test]
    fn env_helpers_clamp_and_filter_values() {
        let mut guard = env::guard();
        guard.set("ARW_LLAMA_N_PREDICT", "9000");
        assert_eq!(
            super::env_u64_in_range("ARW_LLAMA_N_PREDICT", 1, 8192),
            Some(8192)
        );

        guard.set("ARW_LLAMA_TOP_P", "1.4");
        assert_eq!(
            super::env_f64_in_range("ARW_LLAMA_TOP_P", 0.0, 1.0),
            Some(1.0)
        );

        guard.set("ARW_LLAMA_STOP", " stop , next \n finish ");
        assert_eq!(
            super::env_list("ARW_LLAMA_STOP"),
            Some(vec!["stop".into(), "next".into(), "finish".into()])
        );

        guard.set("ARW_OPENAI_MAX_TOKENS", "8");
        assert_eq!(
            super::env_u64_in_range("ARW_OPENAI_MAX_TOKENS", 16, 4096),
            Some(16)
        );

        guard.set("ARW_OPENAI_TOP_P", "-0.5");
        assert_eq!(
            super::env_f64_in_range("ARW_OPENAI_TOP_P", 0.0, 1.0),
            Some(0.0)
        );

        guard.set("ARW_OPENAI_STOP", "foo,,bar");
        assert_eq!(
            super::env_list("ARW_OPENAI_STOP"),
            Some(vec!["foo".into(), "bar".into()])
        );

        guard.set("ARW_LLAMA_TOP_P", "not-a-number");
        assert_eq!(super::env_f64_in_range("ARW_LLAMA_TOP_P", 0.0, 1.0), None);

        guard.set("ARW_LLAMA_STOP", "   ");
        assert_eq!(super::env_list("ARW_LLAMA_STOP"), None);
    }

    #[test]
    fn chat_tool_input_respects_env_defaults() {
        let mut guard = env::guard();
        guard.set("ARW_CHAT_DEFAULT_TEMPERATURE", "1.5");
        guard.set("ARW_CHAT_DEFAULT_VOTE_K", "4");

        let input =
            super::ChatToolInput::from_value(json!({"prompt": "hello"})).expect("chat input");

        assert!((input.temperature - 1.5).abs() < f64::EPSILON);
        assert_eq!(input.vote_k, 4);

        guard.set("ARW_CHAT_DEFAULT_TEMPERATURE", "-3.5");
        guard.set("ARW_CHAT_DEFAULT_VOTE_K", "9");
        let input = super::ChatToolInput::from_value(json!({"prompt": "world"}))
            .expect("chat input clamped");
        assert!((input.temperature - (-3.5)).abs() < f64::EPSILON);
        assert_eq!(input.vote_k, 5);
    }

    async fn build_state(path: &std::path::Path, env_guard: &mut env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(8)
            .build()
            .await
    }

    #[tokio::test]
    async fn tool_defaults_to_synthetic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let output = run_chat_tool(&state, json!({"prompt": "hi"}))
            .await
            .expect("tool output");
        assert_eq!(output["backend"].as_str(), Some("synthetic"));
        assert!(output["text"].as_str().unwrap_or("").contains("hi"));
    }

    #[tokio::test]
    async fn chat_state_tracks_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let outcome = state
            .chat()
            .send(&state, "hello", ChatSendOptions::default())
            .await;
        assert_eq!(outcome.history.len(), 2);
        assert_eq!(outcome.history[0].role, "user");
        assert_eq!(outcome.history[1].role, "assistant");
        assert!(outcome.history[1].content.contains("hello"));
        assert_eq!(outcome.backend, "synthetic");
    }

    #[tokio::test]
    async fn llama_chat_enables_prompt_cache() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());

        let captured: Arc<AsyncMutex<Option<Value>>> = Arc::new(AsyncMutex::new(None));
        let captured_inner = captured.clone();
        let app = Router::new().route(
            "/completion",
            post(move |Json(payload): Json<Value>| {
                let captured = captured_inner.clone();
                async move {
                    {
                        let mut guard = captured.lock().await;
                        *guard = Some(payload);
                    }
                    Json(json!({ "content": "ok" }))
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind llama stub");
        let addr = listener.local_addr().expect("stub addr");
        let server_future = axum::serve(listener, app.into_make_service());
        let server_handle = tokio::spawn(async move {
            let _ = server_future.into_future().await;
        });

        ctx.env.set("ARW_LLAMA_URL", format!("http://{}", addr));
        let state = build_state(temp.path(), &mut ctx.env).await;
        let bus = state.bus();
        let mut events = bus.subscribe();

        let output = run_chat_tool(&state, json!({"prompt": "hello"}))
            .await
            .expect("chat output");
        assert_eq!(output["backend"].as_str(), Some("llama"));

        let payload = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(value) = {
                    let guard = captured.lock().await;
                    guard.clone()
                } {
                    break value;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("captured payload");

        assert_eq!(payload.get("cache_prompt"), Some(&Value::Bool(true)));

        let ledger_event = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                match events.recv().await {
                    Ok(env) => {
                        if env.kind == topics::TOPIC_EGRESS_LEDGER_APPENDED {
                            break env;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(err) => panic!("event receiver closed: {err}"),
                }
            }
        })
        .await
        .expect("egress ledger event");

        let payload = ledger_event.payload;
        let bytes_out = payload
            .get("bytes_out")
            .and_then(|v| v.as_i64())
            .expect("bytes_out present");
        assert!(
            bytes_out > 0,
            "expected positive bytes_out, got {bytes_out}"
        );

        let bytes_in = payload
            .get("bytes_in")
            .and_then(|v| v.as_i64())
            .expect("bytes_in present");
        assert!(bytes_in > 0, "expected positive bytes_in, got {bytes_in}");

        server_handle.abort();
        let _ = server_handle.await;
    }
}
