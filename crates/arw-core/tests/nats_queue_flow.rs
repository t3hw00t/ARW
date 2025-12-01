#![cfg(feature = "nats")]

use std::sync::Arc;

use anyhow::Result;
use arw_core::orchestrator::{Orchestrator, Queue, Task};
use arw_core::test_support::nats::connect_or_spawn;
use serde_json::json;
use tokio::time::{timeout, Duration};

const NATS_URL: &str = "nats://127.0.0.1:4222";

#[tokio::test]
async fn nats_queue_round_trip() -> Result<()> {
    // Best-effort: try local broker first, then auto-spawn when a nats-server binary is present.
    let allow_spawn = std::env::var("ARW_TEST_SPAWN_NATS")
        .map(|v| v != "0")
        .unwrap_or(true);

    let Some(harness) = connect_or_spawn(NATS_URL, allow_spawn).await else {
        eprintln!("skipping nats_queue_round_trip: nats-server not reachable");
        return Ok(());
    };
    let queue = Arc::new(harness.queue.clone());

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
    drop(harness);
    Ok(())
}
