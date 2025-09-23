use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time;

use crate::{
    tasks::TaskHandle,
    tools::{self, ToolError},
    util, AppState,
};
use arw_topics as topics;

pub(crate) fn start_local_worker(state: AppState) -> TaskHandle {
    let bus = state.bus();
    let kernel = state.kernel().clone();
    let policy = state.policy();
    let host = state.host();
    let worker_state = state;
    TaskHandle::new(
        "worker.local",
        tokio::spawn(async move {
            let state = worker_state;
            loop {
                match kernel.dequeue_one_queued_async().await {
                    Ok(Some((id, kind, input))) => {
                        let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                        let env = arw_events::Envelope {
                            time: now,
                            kind: topics::TOPIC_ACTIONS_RUNNING.into(),
                            payload: json!({"id": id}),
                            policy: None,
                            ce: None,
                        };
                        bus.publish(&env.kind, &env.payload);
                        let action_result: Result<Value, ToolError> = if kind == "net.http.get" {
                            let mut input2 = input.clone();
                            if let Some(obj) = input2.as_object_mut() {
                                let hdrs = obj.entry("headers").or_insert_with(|| json!({}));
                                if let Some(hmap) = hdrs.as_object_mut() {
                                    hmap.insert("X-ARW-Corr".to_string(), json!(id.clone()));
                                    if let Ok(p) = std::env::var("ARW_PROJECT_ID") {
                                        hmap.insert("X-ARW-Project".to_string(), json!(p));
                                    }
                                }
                            }
                            let policy_allows =
                                policy.lock().await.evaluate_action("net.http.").allow;
                            let allowed = if policy_allows {
                                true
                            } else {
                                let has_http = kernel
                                    .find_valid_lease_async("local", "net:http")
                                    .await
                                    .ok()
                                    .flatten()
                                    .is_some();
                                if has_http {
                                    true
                                } else {
                                    kernel
                                        .find_valid_lease_async("local", "io:egress")
                                        .await
                                        .ok()
                                        .flatten()
                                        .is_some()
                                }
                            };
                            if !allowed {
                                append_egress_entry_async(
                                    &kernel,
                                    None,
                                    &EgressRecord {
                                        decision: "deny",
                                        reason: Some("no_lease"),
                                        dest_host: None,
                                        dest_port: None,
                                        protocol: Some("http"),
                                        bytes_in: None,
                                        bytes_out: None,
                                        corr_id: None,
                                    },
                                    true,
                                )
                                .await;
                                Ok(json!({"error":"lease required: net:http or io:egress"}))
                            } else {
                                match host.run_tool("http.fetch", &input2).await {
                                    Ok(v) => {
                                        let host_name = v.get("dest_host").and_then(|x| x.as_str());
                                        let port = v.get("dest_port").and_then(|x| x.as_i64());
                                        let proto = v.get("protocol").and_then(|x| x.as_str());
                                        let bin = v.get("bytes_in").and_then(|x| x.as_i64());
                                        let posture = util::effective_posture();
                                        let record = EgressRecord {
                                            decision: "allow",
                                            reason: Some("ok"),
                                            dest_host: host_name,
                                            dest_port: port,
                                            protocol: proto,
                                            bytes_in: bin,
                                            bytes_out: Some(0),
                                            corr_id: Some(id.as_str()),
                                        };
                                        let entry = append_egress_entry_async(
                                            &kernel,
                                            Some(posture.as_str()),
                                            &record,
                                            false,
                                        )
                                        .await;
                                        publish_egress_event(&bus, &posture, entry, &record);
                                        Ok(v)
                                    }
                                    Err(arw_wasi::WasiError::Denied {
                                        reason,
                                        dest_host,
                                        dest_port,
                                        protocol,
                                        ..
                                    }) => {
                                        let posture = util::effective_posture();
                                        let record = EgressRecord {
                                            decision: "deny",
                                            reason: Some(reason.as_str()),
                                            dest_host: dest_host.as_deref(),
                                            dest_port,
                                            protocol: protocol.as_deref(),
                                            bytes_in: None,
                                            bytes_out: None,
                                            corr_id: Some(id.as_str()),
                                        };
                                        let entry = append_egress_entry_async(
                                            &kernel,
                                            Some(posture.as_str()),
                                            &record,
                                            false,
                                        )
                                        .await;
                                        publish_egress_event(&bus, &posture, entry, &record);
                                        Ok(json!({"error":"denied","reason": reason}))
                                    }
                                    Err(e) => {
                                        Ok(json!({"error":"runtime","detail": e.to_string()}))
                                    }
                                }
                            }
                        } else if kind == "fs.patch" {
                            let allowed = if !policy.lock().await.evaluate_action("fs.patch").allow
                            {
                                let has_fs = kernel
                                    .find_valid_lease_async("local", "fs")
                                    .await
                                    .ok()
                                    .flatten()
                                    .is_some();
                                if has_fs {
                                    true
                                } else {
                                    kernel
                                        .find_valid_lease_async("local", "fs:patch")
                                        .await
                                        .ok()
                                        .flatten()
                                        .is_some()
                                }
                            } else {
                                true
                            };
                            if !allowed {
                                bus.publish(
                                    topics::TOPIC_POLICY_DECISION,
                                    &json!({
                                        "action": "fs.patch",
                                        "allow": false,
                                        "require_capability": "fs|fs:patch",
                                        "explain": {"reason":"lease_required"}
                                    }),
                                );
                                Ok(json!({"error":"lease required: fs or fs:patch"}))
                            } else {
                                match host.run_tool("fs.patch", &input).await {
                                    Ok(v) => {
                                        let path_s =
                                            v.get("path").and_then(|x| x.as_str()).unwrap_or("");
                                        bus.publish(
                                            topics::TOPIC_PROJECTS_FILE_WRITTEN,
                                            &json!({"path": path_s, "sha256": v.get("sha256") }),
                                        );
                                        Ok(v)
                                    }
                                    Err(e) => {
                                        Ok(json!({"error":"runtime","detail": e.to_string()}))
                                    }
                                }
                            }
                        } else if kind == "app.vscode.open" {
                            let allowed =
                                if !policy.lock().await.evaluate_action("app.vscode.open").allow {
                                    let has_vscode = kernel
                                        .find_valid_lease_async("local", "io:app:vscode")
                                        .await
                                        .ok()
                                        .flatten()
                                        .is_some();
                                    if has_vscode {
                                        true
                                    } else {
                                        kernel
                                            .find_valid_lease_async("local", "io:app")
                                            .await
                                            .ok()
                                            .flatten()
                                            .is_some()
                                    }
                                } else {
                                    true
                                };
                            if !allowed {
                                bus.publish(
                                    topics::TOPIC_POLICY_DECISION,
                                    &json!({
                                        "action": "app.vscode.open",
                                        "allow": false,
                                        "require_capability": "io:app:vscode|io:app",
                                        "explain": {"reason":"lease_required"}
                                    }),
                                );
                                Ok(json!({"error":"lease required: io:app:vscode or io:app"}))
                            } else {
                                match host.run_tool("app.vscode.open", &input).await {
                                    Ok(v) => {
                                        let path_s = input
                                            .get("path")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or("");
                                        bus.publish(
                                            topics::TOPIC_APPS_VSCODE_OPENED,
                                            &json!({"path": path_s }),
                                        );
                                        Ok(v)
                                    }
                                    Err(e) => {
                                        Ok(json!({"error":"runtime","detail": e.to_string()}))
                                    }
                                }
                            }
                        } else {
                            execute_dynamic_action(&state, &kind, &input).await
                        };
                        let out = match action_result {
                            Ok(value) => value,
                            Err(err) => {
                                handle_action_failure(&kernel, &bus, &id, &kind, err).await;
                                continue;
                            }
                        };
                        let _ = kernel
                            .update_action_result_async(id.clone(), Some(out.clone()), None)
                            .await;
                        let _ = kernel.set_action_state_async(&id, "completed").await;
                        let now2 = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                        let env2 = arw_events::Envelope {
                            time: now2,
                            kind: topics::TOPIC_ACTIONS_COMPLETED.into(),
                            payload: json!({"id": env.payload["id"], "output": out}),
                            policy: None,
                            ce: None,
                        };
                        bus.publish(&env2.kind, &env2.payload);
                        let _ = kernel
                            .append_contribution_async(
                                "local",
                                "task.complete",
                                1.0,
                                "task",
                                None,
                                None,
                                None,
                            )
                            .await;
                    }
                    Ok(None) => time::sleep(Duration::from_millis(200)).await,
                    Err(_) => time::sleep(Duration::from_millis(500)).await,
                }
            }
        }),
    )
}

struct EgressRecord<'a> {
    decision: &'static str,
    reason: Option<&'a str>,
    dest_host: Option<&'a str>,
    dest_port: Option<i64>,
    protocol: Option<&'a str>,
    bytes_in: Option<i64>,
    bytes_out: Option<i64>,
    corr_id: Option<&'a str>,
}

fn ledger_enabled() -> bool {
    matches!(
        std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref(),
        Some("1")
    )
}

async fn append_egress_entry_async(
    kernel: &arw_kernel::Kernel,
    posture: Option<&str>,
    record: &EgressRecord<'_>,
    force: bool,
) -> Option<i64> {
    if force || ledger_enabled() {
        kernel
            .append_egress_async(
                record.decision.to_string(),
                record.reason.map(|s| s.to_string()),
                record.dest_host.map(|s| s.to_string()),
                record.dest_port,
                record.protocol.map(|s| s.to_string()),
                record.bytes_in,
                record.bytes_out,
                record.corr_id.map(|s| s.to_string()),
                None,
                posture.map(|s| s.to_string()),
                None,
            )
            .await
            .ok()
    } else {
        None
    }
}

fn publish_egress_event(
    bus: &arw_events::Bus,
    posture: &str,
    ledger_id: Option<i64>,
    record: &EgressRecord<'_>,
) {
    let mut payload = serde_json::Map::new();
    payload.insert("id".into(), json!(ledger_id));
    payload.insert("decision".into(), json!(record.decision));
    if let Some(reason) = record.reason {
        payload.insert("reason".into(), json!(reason));
    }
    payload.insert("dest_host".into(), json!(record.dest_host));
    payload.insert("dest_port".into(), json!(record.dest_port));
    payload.insert("protocol".into(), json!(record.protocol));
    payload.insert("bytes_in".into(), json!(record.bytes_in));
    payload.insert("corr_id".into(), json!(record.corr_id));
    payload.insert("posture".into(), json!(posture));
    bus.publish(
        topics::TOPIC_EGRESS_LEDGER_APPENDED,
        &Value::Object(payload),
    );
}

struct DeniedInfo {
    reason: String,
    dest_host: Option<String>,
    dest_port: Option<i64>,
    protocol: Option<String>,
}

fn tool_error_details(
    id: &str,
    kind: &str,
    err: &ToolError,
) -> (String, Value, Option<DeniedInfo>) {
    match err {
        ToolError::Unsupported(tool_id) => {
            let tool_name = tool_id.clone();
            let error_msg = format!("unsupported tool: {}", tool_name);
            let payload = json!({
                "id": id,
                "kind": kind,
                "error": {
                    "type": "unsupported",
                    "tool": tool_name,
                    "detail": "tool is not available",
                }
            });
            (error_msg, payload, None)
        }
        ToolError::Invalid(detail) => {
            let detail_cloned = detail.clone();
            let error_msg = format!("invalid request: {}", detail_cloned);
            let payload = json!({
                "id": id,
                "kind": kind,
                "error": {
                    "type": "invalid",
                    "detail": detail_cloned,
                }
            });
            (error_msg, payload, None)
        }
        ToolError::Runtime(detail) => {
            let detail_cloned = detail.clone();
            let error_msg = format!("runtime error: {}", detail_cloned);
            let payload = json!({
                "id": id,
                "kind": kind,
                "error": {
                    "type": "runtime",
                    "detail": detail_cloned,
                }
            });
            (error_msg, payload, None)
        }
        ToolError::Denied {
            reason,
            dest_host,
            dest_port,
            protocol,
        } => {
            let denied = DeniedInfo {
                reason: reason.clone(),
                dest_host: dest_host.clone(),
                dest_port: *dest_port,
                protocol: protocol.clone(),
            };
            let error_msg = format!("denied: {}", reason);
            let payload = json!({
                "id": id,
                "kind": kind,
                "error": {
                    "type": "denied",
                    "reason": reason,
                    "dest_host": dest_host,
                    "dest_port": dest_port,
                    "protocol": protocol,
                }
            });
            (error_msg, payload, Some(denied))
        }
    }
}

async fn handle_action_failure(
    kernel: &arw_kernel::Kernel,
    bus: &arw_events::Bus,
    id: &str,
    kind: &str,
    err: ToolError,
) {
    let (error_msg, mut event_payload, denied) = tool_error_details(id, kind, &err);
    tracing::warn!(target: "arw::worker", %id, %kind, "action failed: {}", error_msg);

    let _ = kernel
        .update_action_result_async(id.to_string(), None, Some(error_msg.clone()))
        .await;
    let _ = kernel.set_action_state_async(id, "failed").await;

    tools::ensure_corr(&mut event_payload);
    bus.publish(topics::TOPIC_ACTIONS_FAILED, &event_payload);

    if let Some(denied) = denied {
        let posture = util::effective_posture();
        let record = EgressRecord {
            decision: "deny",
            reason: Some(denied.reason.as_str()),
            dest_host: denied.dest_host.as_deref(),
            dest_port: denied.dest_port,
            protocol: denied.protocol.as_deref(),
            bytes_in: None,
            bytes_out: None,
            corr_id: Some(id),
        };
        let entry = append_egress_entry_async(kernel, Some(posture.as_str()), &record, false).await;
        publish_egress_event(bus, &posture, entry, &record);
    }
}

async fn execute_dynamic_action(
    state: &AppState,
    kind: &str,
    input: &Value,
) -> Result<Value, ToolError> {
    match tools::run_tool(state, kind, input.clone()).await {
        Ok(value) => Ok(value),
        Err(err) => {
            if kind.starts_with("demo.") {
                simulate_action(kind, input).map_err(ToolError::Runtime)
            } else {
                Err(err)
            }
        }
    }
}

fn simulate_action(kind: &str, input: &Value) -> Result<Value, String> {
    match kind {
        "demo.echo" => Ok(json!({"echo": input})),
        _ => Ok(json!({"ok": true})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use arw_policy::PolicyEngine;
    use arw_topics as topics;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(32, 32);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    #[tokio::test]
    async fn unsupported_tool_marks_action_failed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
        let _worker = start_local_worker(state.clone());

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_ACTIONS_FAILED.to_string()], Some(8));

        let action_id = uuid::Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(&action_id, "tool.missing", &json!({}), None, None, "queued")
            .await
            .expect("enqueue action");

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("bus event")
            .expect("event value");
        assert_eq!(env.kind, topics::TOPIC_ACTIONS_FAILED);
        assert_eq!(env.payload["id"].as_str(), Some(action_id.as_str()));
        assert_eq!(env.payload["kind"].as_str(), Some("tool.missing"));
        assert_eq!(env.payload["error"]["type"].as_str(), Some("unsupported"));

        let stored = state
            .kernel()
            .get_action_async(&action_id)
            .await
            .expect("get action")
            .expect("action row");
        assert_eq!(stored.state, "failed");
        let error_text = stored.error.unwrap_or_default();
        assert!(error_text.contains("unsupported"));
    }
}
