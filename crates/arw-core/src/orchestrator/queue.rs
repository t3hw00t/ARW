use super::types::{LeaseToken, Task};

/// Queue abstraction for distributing Tasks.
#[async_trait::async_trait]
pub trait Queue: Send + Sync {
    /// Enqueue a task; returns effective task id.
    async fn enqueue(&self, t: Task) -> anyhow::Result<String>;
    /// Dequeue next task for this consumer group; returns task and lease token.
    async fn dequeue(&self, group: &str) -> anyhow::Result<(Task, LeaseToken)>;
    /// Acknowledge and remove a task using its lease token.
    async fn ack(&self, lease: LeaseToken) -> anyhow::Result<()>;
    /// Negative-acknowledge; optionally schedule retry after millis.
    async fn nack(&self, lease: LeaseToken, retry_after_ms: Option<u64>) -> anyhow::Result<()>;
}
