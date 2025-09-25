use std::sync::Arc;
use std::time::Instant;

use crate::http_timeout;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use url::Url;

use crate::{
    egress_log::{self, EgressRecord},
    tools::{self, ToolError},
    AppState,
};

const HISTORY_LIMIT: usize = 48;
const TOOL_HISTORY_LIMIT: usize = 12;

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
            .unwrap_or(0.2)
            .clamp(-5.0, 5.0);

        let vote_k = value
            .get("vote_k")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .clamp(1, 5) as usize;

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
    }

    let client = reqwest::Client::builder()
        .timeout(http_timeout::get_duration())
        .build()
        .map_err(|e| ToolError::Runtime(e.to_string()))?;

    match client.post(&url).json(&body).send().await {
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
                        record_egress(state, &url, "allow", None, Some(bytes_len)).await;
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
                        record_egress(state, &url, "deny", Some(status.as_str()), Some(bytes_len))
                            .await;
                        Ok(None)
                    }
                }
                Err(err) => {
                    record_egress(state, &url, "deny", Some("read_error"), None).await;
                    Err(ToolError::Runtime(err.to_string()))
                }
            }
        }
        Err(err) => {
            record_egress(state, &url, "deny", Some("network"), None).await;
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

    let body = json!({
        "model": model,
        "messages": messages,
        "temperature": input.temperature,
        "max_tokens": 512,
    });

    let client = reqwest::Client::builder()
        .timeout(http_timeout::get_duration())
        .build()
        .map_err(|e| ToolError::Runtime(e.to_string()))?;

    let request = client.post(&api_url).bearer_auth(&key).json(&body);
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
                        record_egress(state, &api_url, "allow", None, Some(bytes_len)).await;
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
                            Some(bytes_len),
                        )
                        .await;
                        Ok(None)
                    }
                }
                Err(err) => {
                    record_egress(state, &api_url, "deny", Some("read_error"), None).await;
                    Err(ToolError::Runtime(err.to_string()))
                }
            }
        }
        Err(err) => {
            record_egress(state, &api_url, "deny", Some("network"), None).await;
            Err(ToolError::Runtime(err.to_string()))
        }
    }
}

async fn record_egress(
    state: &AppState,
    url: &str,
    decision: &'static str,
    reason: Option<&str>,
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
        bytes_out: None,
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
    use crate::AppState;
    use arw_policy::PolicyEngine;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(8)
            .build()
            .await
    }

    #[tokio::test]
    async fn tool_defaults_to_synthetic() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
        let output = run_chat_tool(&state, json!({"prompt": "hi"}))
            .await
            .expect("tool output");
        assert_eq!(output["backend"].as_str(), Some("synthetic"));
        assert!(output["text"].as_str().unwrap_or("").contains("hi"));
    }

    #[tokio::test]
    async fn chat_state_tracks_history() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
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
}
