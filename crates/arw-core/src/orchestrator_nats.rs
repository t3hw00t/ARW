#![cfg(feature = "nats")]
use crate::orchestrator::{LeaseToken, Queue, Task};
use anyhow::Result;
use async_nats::Client;
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
