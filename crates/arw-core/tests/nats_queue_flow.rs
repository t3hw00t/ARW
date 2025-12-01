#![cfg(feature = "nats")]

use std::sync::Arc;

use anyhow::Result;
use arw_core::orchestrator::{Orchestrator, Queue, Task};
use arw_core::orchestrator_nats::NatsQueue;
use serde_json::json;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn nats_queue_round_trip() -> Result<()> {
    // Best-effort: if no local NATS is running, skip instead of failing the suite.
    let queue = match timeout(Duration::from_secs(2), NatsQueue::connect("nats://127.0.0.1:4222"))
        .await
    {
        Ok(Ok(q)) => Arc::new(q),
        _ => {
            eprintln!("skipping nats_queue_round_trip: nats-server not reachable on localhost:4222");
            return Ok(());
        }
    };
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
