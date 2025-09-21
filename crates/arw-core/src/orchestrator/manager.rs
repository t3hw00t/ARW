use std::sync::Arc;

use super::{queue::Queue, types::Task};

/// Minimal orchestrator façade keeping placement logic separate from queue impls.
#[derive(Clone)]
pub struct Orchestrator<Q: Queue> {
    queue: Arc<Q>,
}

impl<Q: Queue> Orchestrator<Q> {
    pub fn new(queue: Arc<Q>) -> Self {
        Self { queue }
    }

    pub async fn admit(&self, task: Task) -> anyhow::Result<String> {
        self.queue.enqueue(task).await
    }
}
