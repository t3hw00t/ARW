#![cfg(feature = "nats")]

use std::sync::Arc;

use anyhow::Result;
use arw_core::orchestrator::{Orchestrator, Queue, Task};
use arw_core::orchestrator_nats::NatsQueue;
use serde_json::json;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn nats_queue_round_trip() -> Result<()> {
    // Best-effort: if no local NATS is running, try to spawn (when opted-in), otherwise skip.
    let mut spawned: Option<std::process::Child> = None;
    let queue = match timeout(Duration::from_secs(2), NatsQueue::connect("nats://127.0.0.1:4222"))
        .await
    {
        Ok(Ok(q)) => Arc::new(q),
        _ => {
            // Spawn nats-server only if explicitly requested, and ignore failure.
            if std::env::var("ARW_TEST_SPAWN_NATS").is_ok() {
                if let Ok(child) = std::process::Command::new("nats-server")
                    .args(["-p", "4222"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                {
                    spawned = Some(child);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    if let Ok(Ok(q2)) =
                        timeout(Duration::from_secs(2), NatsQueue::connect("nats://127.0.0.1:4222"))
                            .await
                    {
                        Arc::new(q2)
                    } else {
                        eprintln!("skipping nats_queue_round_trip: nats-server not reachable after spawn");
                        return Ok(());
                    }
                } else {
                    eprintln!("skipping nats_queue_round_trip: failed to spawn nats-server");
                    return Ok(());
                }
            } else {
                eprintln!("skipping nats_queue_round_trip: nats-server not reachable on localhost:4222");
                return Ok(());
            }
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
    // Clean up spawned broker if any.
    if let Some(mut child) = spawned {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}
