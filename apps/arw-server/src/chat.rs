use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ChatState {
    inner: Arc<Mutex<ChatLog>>,
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
    }

    pub async fn send(&self, prompt: &str) -> ChatSendOutcome {
        let mut guard = self.inner.lock().await;
        let now_ms = now_ms();
        let user = ChatMessage::new("user", prompt, now_ms);
        guard.messages.push(user.clone());

        // Synthetic response: mirror minimal behaviour for debug tooling.
        let reply_text = if prompt.trim().is_empty() {
            "(no input)".to_string()
        } else {
            format!("synthetic echo: {}", prompt.trim())
        };
        let assistant = ChatMessage::new("assistant", &reply_text, now_ms + 1);
        guard.messages.push(assistant.clone());

        ChatSendOutcome {
            backend: guard.backend.kind().to_string(),
            reply: assistant,
            history: guard.messages.clone(),
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

#[derive(Clone, Copy, Default)]
enum ChatBackend {
    #[default]
    Synthetic,
}

impl ChatBackend {
    fn kind(&self) -> &'static str {
        match self {
            ChatBackend::Synthetic => "synthetic",
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
