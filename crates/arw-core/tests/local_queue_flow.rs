use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use arw_core::orchestrator::{LocalQueue, Orchestrator, Queue, Task};
use serde_json::json;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn local_queue_end_to_end_flow() {
    let queue = Arc::new(LocalQueue::with_lease_ttl(250));
    let orchestrator = Orchestrator::new(queue.clone());

    let received: Arc<tokio::sync::Mutex<Vec<String>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let retry_attempt = Arc::new(AtomicU32::new(0));

    let worker_queue = queue.clone();
    let worker_received = received.clone();
    let worker_retry_attempt = retry_attempt.clone();

    let worker = tokio::spawn(async move {
        let mut acked = 0usize;
        while acked < 3 {
            let (task, lease) = worker_queue.dequeue("workers").await.unwrap();
            if task.kind == "nack_once" && task.attempt == 0 {
                worker_queue
                    .nack(lease, Some(75))
                    .await
                    .expect("nack should succeed");
                continue;
            }

            if task.kind == "nack_once" {
                worker_retry_attempt.store(task.attempt, Ordering::SeqCst);
            }

            worker_queue.ack(lease).await.expect("ack should succeed");
            worker_received.lock().await.push(task.kind.clone());
            acked += 1;
        }
    });

    let mut low = Task::new("low", json!({"value": "low"}));
    low.priority = 5;
    orchestrator.admit(low).await.expect("enqueue low");

    let mut high = Task::new("high", json!({"value": "high"}));
    high.priority = -10;
    orchestrator.admit(high).await.expect("enqueue high");

    let mut retry = Task::new("nack_once", json!({"value": "retry"}));
    retry.priority = 0;
    orchestrator.admit(retry).await.expect("enqueue retry");

    timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker join timeout")
        .expect("worker task failed");

    let results = received.lock().await.clone();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0], "high", "highest priority task should run first");
    assert_eq!(results[1], "low", "lower priority task should follow");
    assert_eq!(
        results[2], "nack_once",
        "requeued task should complete last"
    );
    assert_eq!(
        retry_attempt.load(Ordering::SeqCst),
        1,
        "retry attempt should be recorded"
    );
}
