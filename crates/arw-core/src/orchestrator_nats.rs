#![cfg(feature = "nats")]
use crate::orchestrator::{LeaseToken, Queue, Task};
use anyhow::Result;
use async_nats::Client;
#[cfg(feature = "nats_js")]
use async_nats::jetstream::context::Context as JsContext;
#[cfg(feature = "nats_js")]
use async_nats::jetstream::{self as js, consumer::pull::Message as JsMessage, consumer::pull::Stream as PullStream};
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
        let client = async_nats::connect(url).await?;
        Ok(Self {
            client,
            subject: "arw.tasks".to_string(),
            subs: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl Queue for NatsQueue {
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
            expires_at_ms: crate::orchestrator::now_millis() + 30_000,
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

#[cfg(feature = "nats_js")]
mod jetstream_backend {
    use super::*;
    use async_nats::jetstream::context::Context as JsContext;
    use async_nats::jetstream::{consumer::pull::Message as JsMessage, consumer::pull::Stream as PullStream};
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
            let _ = js.get_or_create_stream(async_nats::jetstream::stream::Config {
                name: stream.to_string(),
                subjects: vec![subject.to_string()],
                ..Default::default()
            }).await?;
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
            let id = if t.id.is_empty() { Uuid::new_v4().to_string() } else { t.id.clone() };
            let mut t2 = t.clone();
            if t2.id.is_empty() { t2.id = id.clone(); }
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
            let msg = messages.next().await.ok_or_else(|| anyhow::anyhow!("no message"))?;
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
                    expires_at_ms: crate::orchestrator::now_millis() + 30_000,
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
                    msg.nak_with_delay(std::time::Duration::from_millis(delay)).await?;
                } else {
                    msg.nak(None).await?;
                }
            }
            Ok(())
        }
    }
}
