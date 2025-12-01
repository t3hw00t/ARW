use arw_otel::init_with_service;

// Event topic and subject constants (dot.case)
// Only needed when built with the `nats` feature; guard to avoid dead_code in other builds.
#[cfg(feature = "nats")]
const TOPIC_TASK_COMPLETED: &str = "task.completed";
#[cfg(feature = "nats")]
const SUBJECT_EVENTS_ROOT: &str = "arw.events"; // cluster-wide
#[cfg(feature = "nats")]
const SUBJECT_NODE_ROOT: &str = "arw.events.node"; // per-node

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_with_service("arw-connector");

    #[cfg(feature = "nats")]
    {
        use arw_core::orchestrator::Queue;
        use arw_core::orchestrator_nats::NatsQueue;
        use serde_json::json;
        let url =
            std::env::var("ARW_NATS_URL").unwrap_or_else(|_| "nats://127.0.0.1:4222".to_string());
        let group = std::env::var("ARW_GROUP").unwrap_or_else(|_| "workers".to_string());
        let q = NatsQueue::connect(&url).await?;
        let nats = async_nats::connect(&url).await?;
        let node_id = std::env::var("ARW_NODE_ID").unwrap_or_else(|_| "connector".to_string());
        tracing::info!("arw-connector connected to {} as group {}", url, group);
        tokio::spawn({
            let group = group.clone();
            let node_id = node_id.clone();
            async move {
                loop {
                    match q.dequeue(&group).await {
                        Ok((t, lease)) => {
                            let out = match t.kind.as_str() {
                                "math.add" => {
                                    let a =
                                        t.payload.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                    let b =
                                        t.payload.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                    json!({"sum": a + b})
                                }
                                "time.now" => {
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis()
                                        as i64;
                                    json!({"now_ms": now})
                                }
                                _ => json!({"error":"unknown tool", "id": t.kind}),
                            };
                            let _ = q.ack(lease).await;
                            tracing::info!(target: "arw-connector", "completed task {}", t.id);
                            // Publish task.completed event
                            let env = json!({
                                "time": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                                "kind": TOPIC_TASK_COMPLETED,
                                "payload": {"id": t.id, "ok": true, "output": out}
                            });
                            if let Ok(bytes) = serde_json::to_vec(&env) {
                                // Cluster-wide subject
                                let subj_cluster =
                                    format!("{}.{}", SUBJECT_EVENTS_ROOT, TOPIC_TASK_COMPLETED);
                                let _ = nats.publish(subj_cluster, bytes.clone().into()).await;
                                // Node-scoped subject to avoid loops and allow targeting
                                let subj = format!(
                                    "{}.{}.{}",
                                    SUBJECT_NODE_ROOT, node_id, TOPIC_TASK_COMPLETED
                                );
                                let _ = nats.publish(subj, bytes.into()).await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("dequeue error: {}", e);
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        }
                    }
                }
            }
        });
        // Wait for Ctrl+C to allow graceful shutdown
        tokio::signal::ctrl_c().await?;
        Ok(())
    }

    #[cfg(not(feature = "nats"))]
    {
        eprintln!("arw-connector built without 'nats' feature; rebuild with features to connect to a broker");
        Ok(())
    }
}
