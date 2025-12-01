#![cfg(feature = "nats")]

use std::sync::Arc;

use anyhow::Result;
use arw_core::orchestrator::{Orchestrator, Queue, Task};
use arw_core::orchestrator_nats::NatsQueue;
use serde_json::json;
use tokio::time::{timeout, Duration};

#[tokio::test]
#[ignore = "requires a running nats-server at nats://127.0.0.1:4222"]
async fn nats_queue_round_trip() -> Result<()> {
    let queue = Arc::new(NatsQueue::connect("nats://127.0.0.1:4222").await?);
    let orchestrator = Orchestrator::new(queue.clone());

    let worker_queue = queue.clone();
    let worker = tokio::spawn(async move {
        let (task, lease) = worker_queue.dequeue("workers").await?;
        worker_queue.ack(lease).await?;
        Result::<String>::Ok(task.kind)
    });

    // Give the worker a moment to establish the subscription before publishing.
    tokio::time::sleep(Duration::from_millis(200)).await;

    orchestrator
        .admit(Task::new("nats_test", json!({"value": "nats"})))
        .await?;

    let join_result = timeout(Duration::from_secs(5), worker).await?;
    let task_result = join_result?;
    let task_kind = task_result?;
    assert_eq!(task_kind, "nats_test");
    Ok(())
}
