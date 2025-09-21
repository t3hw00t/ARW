use super::{
    queue::Queue,
    types::{LeaseToken, Task, DEFAULT_LEASE_TTL_MS},
    util::now_millis,
};
use anyhow::Result;
use async_nats::Client;
// JetStream imports currently disabled until API is finalized
// use async_nats::jetstream::context::Context as JsContext;
// use async_nats::jetstream::{self as js, consumer::pull::Message as JsMessage, consumer::pull::Stream as PullStream};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// NATS-backed queue using core queue subscriptions (at-most-once, best-effort).
#[derive(Clone)]
pub struct NatsQueue {
    client: Client,
    subject: String,
    subs: Arc<Mutex<HashMap<String, Arc<Mutex<async_nats::Subscriber>>>>>,
}

impl NatsQueue {
    pub async fn connect(url: &str) -> Result<Self> {
        let client = connect_with_env(url).await?;
        Ok(Self {
            client,
            subject: "arw.tasks".to_string(),
            subs: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

/// Build a NATS connection honoring basic env-based auth/TLS knobs and simple initial retry.
async fn connect_with_env(url: &str) -> Result<Client> {
    // Optional TLS upgrade
    let mut u = url.to_string();
    if std::env::var("ARW_NATS_TLS").ok().as_deref() == Some("1") {
        u = u.replacen("nats://", "tls://", 1);
        u = u.replacen("ws://", "wss://", 1);
    }
    // If ARW_NATS_USER/PASS are provided and URL has no userinfo, inject them (best-effort)
    if !u.contains('@') {
        if let (Ok(user), Ok(pass)) = (
            std::env::var("ARW_NATS_USER"),
            std::env::var("ARW_NATS_PASS"),
        ) {
            if let Some((scheme, rest)) = u.split_once("://") {
                u = format!("{}://{}:{}@{}", scheme, user, pass, rest);
            }
        }
    }
    // Initial connect retry/backoff
    let retries: u32 = std::env::var("ARW_NATS_CONNECT_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let backoff_ms: u64 = std::env::var("ARW_NATS_CONNECT_BACKOFF_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let mut last_err: Option<anyhow::Error> = None;
    for _ in 0..=retries {
        match async_nats::connect(&u).await {
            Ok(c) => return Ok(c),
            Err(e) => {
                last_err = Some(anyhow::anyhow!(e));
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("nats connect failed")))
}

#[async_trait]
impl Queue for NatsQueue {
    async fn enqueue(&self, mut t: Task) -> Result<String> {
        if t.id.is_empty() {
            t.id = Uuid::new_v4().to_string();
        }
        let id = t.id.clone();
        let bytes = serde_json::to_vec(&t)?;
        self.client
            .publish(self.subject.clone(), bytes.into())
            .await?;
        Ok(id)
    }

    async fn dequeue(&self, group: &str) -> Result<(Task, LeaseToken)> {
        // Ensure a queue subscriber for this group
        let sub_arc = {
            let mut subs = self.subs.lock().await;
            if let Some(s) = subs.get(group) {
                s.clone()
            } else {
                let s = self
                    .client
                    .queue_subscribe(self.subject.clone(), group.to_string())
                    .await?;
                let arc = Arc::new(Mutex::new(s));
                subs.insert(group.to_string(), arc.clone());
                arc
            }
        };

        // Receive next message
        let mut sub = sub_arc.lock().await;
        let msg = sub
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("nats closed"))?;
        let task: Task = serde_json::from_slice(&msg.payload)?;
        let lease = LeaseToken {
            task_id: task.id.clone(),
            lease_id: Uuid::new_v4().to_string(),
            expires_at_ms: now_millis() + DEFAULT_LEASE_TTL_MS,
        };
        Ok((task, lease))
    }

    async fn ack(&self, _lease: LeaseToken) -> Result<()> {
        // Core NATS has no acks; message was delivered at-most-once to our queue group.
        Ok(())
    }

    async fn nack(&self, _lease: LeaseToken, _retry_after_ms: Option<u64>) -> Result<()> {
        // Not supported in core NATS; connector may re-enqueue explicitly if desired.
        Ok(())
    }
}

#[cfg(any())]
mod jetstream_backend {
    use super::*;
    use async_nats::jetstream::context::Context as JsContext;
    use async_nats::jetstream::{
        consumer::pull::Message as JsMessage, consumer::pull::Stream as PullStream,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[derive(Clone)]
    pub struct JetStreamQueue {
        _client: Client,
        js: JsContext,
        stream: String,
        subject: String,
        pending: Arc<Mutex<HashMap<String, JsMessage>>>,
    }

    impl JetStreamQueue {
        pub async fn connect(url: &str, stream: &str, subject: &str) -> Result<Self> {
            let client = async_nats::connect(url).await?;
            let js = async_nats::jetstream::new(client.clone());
            let _ = js
                .get_or_create_stream(async_nats::jetstream::stream::Config {
                    name: stream.to_string(),
                    subjects: vec![subject.to_string()],
                    ..Default::default()
                })
                .await?;
            Ok(Self {
                _client: client,
                js,
                stream: stream.to_string(),
                subject: subject.to_string(),
                pending: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }

    #[async_trait]
    impl Queue for JetStreamQueue {
        async fn enqueue(&self, t: Task) -> Result<String> {
            let id = if t.id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                t.id.clone()
            };
            let mut t2 = t.clone();
            if t2.id.is_empty() {
                t2.id = id.clone();
            }
            let bytes = serde_json::to_vec(&t2)?;
            self.js.publish(self.subject.clone(), bytes.into()).await?;
            Ok(id)
        }

        async fn dequeue(&self, group: &str) -> Result<(Task, LeaseToken)> {
            use async_nats::jetstream::consumer::{pull, AckPolicy};
            let consumer = self
                .js
                .get_or_create_consumer(
                    self.stream.clone(),
                    pull::Config {
                        durable_name: Some(group.to_string()),
                        ack_policy: AckPolicy::Explicit,
                        filter_subject: self.subject.clone(),
                        ..Default::default()
                    },
                )
                .await?;
            let mut messages: PullStream = consumer.messages().await?;
            let msg = messages
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("no message"))?;
            let task: Task = serde_json::from_slice(&msg.message.payload)?;
            let lease_id = Uuid::new_v4().to_string();
            {
                let mut p = self.pending.lock().await;
                p.insert(lease_id.clone(), msg);
            }
            Ok((
                task.clone(),
                LeaseToken {
                    task_id: task.id.clone(),
                    lease_id,
                    expires_at_ms: now_millis() + 30_000,
                },
            ))
        }

        async fn ack(&self, lease: LeaseToken) -> Result<()> {
            if let Some(msg) = self.pending.lock().await.remove(&lease.lease_id) {
                msg.ack().await?;
            }
            Ok(())
        }

        async fn nack(&self, lease: LeaseToken, retry_after_ms: Option<u64>) -> Result<()> {
            if let Some(msg) = self.pending.lock().await.remove(&lease.lease_id) {
                if let Some(delay) = retry_after_ms {
                    msg.nak_with_delay(std::time::Duration::from_millis(delay))
                        .await?;
                } else {
                    msg.nak(None).await?;
                }
            }
            Ok(())
        }
    }
}
