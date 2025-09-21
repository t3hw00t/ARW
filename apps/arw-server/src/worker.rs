use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::time;

use crate::{tasks::TaskHandle, tool_cache::StoreOutcome, util, AppState};
use arw_topics as topics;

pub(crate) fn start_local_worker(state: AppState) -> TaskHandle {
    let bus = state.bus();
    let kernel = state.kernel().clone();
    let policy = state.policy();
    let host = state.host();
    let tool_cache = state.tool_cache();
    TaskHandle::new(
        "worker.local",
        tokio::spawn(async move {
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
                        let out = if kind == "net.http.get" {
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
                                json!({"error":"lease required: net:http or io:egress"})
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
                                        v
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
                                        json!({"error":"denied","reason": reason})
                                    }
                                    Err(e) => json!({"error":"runtime","detail": e.to_string()}),
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
                                json!({"error":"lease required: fs or fs:patch"})
                            } else {
                                match host.run_tool("fs.patch", &input).await {
                                    Ok(v) => {
                                        let path_s =
                                            v.get("path").and_then(|x| x.as_str()).unwrap_or("");
                                        bus.publish(
                                            topics::TOPIC_PROJECTS_FILE_WRITTEN,
                                            &json!({"path": path_s, "sha256": v.get("sha256") }),
                                        );
                                        v
                                    }
                                    Err(e) => json!({"error":"runtime","detail": e.to_string()}),
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
                                json!({"error":"lease required: io:app:vscode or io:app"})
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
                                        v
                                    }
                                    Err(e) => json!({"error":"runtime","detail": e.to_string()}),
                                }
                            }
                        } else {
                            let mut cache_event: Option<Value> = None;
                            let output_value: Value;
                            if tool_cache.enabled() && tool_cache.is_cacheable(&kind) {
                                let cache_key = tool_cache.action_key(&kind, &input);
                                let lookup_start = Instant::now();
                                if let Some(hit) = tool_cache.lookup(&cache_key).await {
                                    let elapsed_ms = lookup_start.elapsed().as_millis() as u64;
                                    metrics::counter!("arw_tools_cache_hits", 1);
                                    cache_event = Some(json!({
                                        "action_id": id.clone(),
                                        "tool": kind.clone(),
                                        "outcome": "hit",
                                        "elapsed_ms": elapsed_ms,
                                        "key": cache_key,
                                        "digest": hit.digest,
                                        "cached": true,
                                        "age_secs": hit.age_secs,
                                    }));
                                    output_value = hit.value;
                                } else {
                                    let run_start = Instant::now();
                                    let simulated = simulate_action(&kind, &input)
                                        .unwrap_or_else(|_| json!({"ok": true}));
                                    let elapsed_ms = run_start.elapsed().as_millis() as u64;
                                    let store_outcome =
                                        tool_cache.store(&cache_key, &simulated).await;
                                    let (outcome_label, digest_opt, cached_flag, reason_opt) =
                                        match store_outcome {
                                            Some(StoreOutcome {
                                                digest,
                                                cached: true,
                                            }) => {
                                                metrics::counter!("arw_tools_cache_miss", 1);
                                                ("miss", Some(digest), true, None)
                                            }
                                            Some(StoreOutcome {
                                                digest,
                                                cached: false,
                                            }) => {
                                                metrics::counter!("arw_tools_cache_error", 1);
                                                (
                                                    "error",
                                                    Some(digest),
                                                    false,
                                                    Some("store_failed".to_string()),
                                                )
                                            }
                                            None => {
                                                metrics::counter!("arw_tools_cache_error", 1);
                                                (
                                                    "error",
                                                    None,
                                                    false,
                                                    Some("serialize_failed".to_string()),
                                                )
                                            }
                                        };
                                    let mut payload = json!({
                                        "action_id": id.clone(),
                                        "tool": kind.clone(),
                                        "outcome": outcome_label,
                                        "elapsed_ms": elapsed_ms,
                                        "key": cache_key,
                                        "digest": digest_opt,
                                        "cached": cached_flag,
                                        "age_secs": Value::Null,
                                    });
                                    if let Some(reason) = reason_opt {
                                        payload["reason"] = Value::String(reason);
                                    }
                                    cache_event = Some(payload);
                                    output_value = simulated;
                                }
                            } else {
                                let run_start = Instant::now();
                                let simulated = simulate_action(&kind, &input)
                                    .unwrap_or_else(|_| json!({"ok": true}));
                                let elapsed_ms = run_start.elapsed().as_millis() as u64;
                                if tool_cache.enabled() {
                                    let cache_key = tool_cache.action_key(&kind, &input);
                                    metrics::counter!("arw_tools_cache_bypass", 1);
                                    cache_event = Some(json!({
                                        "action_id": id.clone(),
                                        "tool": kind.clone(),
                                        "outcome": "not_cacheable",
                                        "elapsed_ms": elapsed_ms,
                                        "key": cache_key,
                                        "cached": false,
                                        "reason": "not_cacheable",
                                    }));
                                }
                                output_value = simulated;
                            }
                            if let Some(evt) = cache_event {
                                bus.publish(topics::TOPIC_TOOL_CACHE, &evt);
                            }
                            output_value
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

fn simulate_action(kind: &str, input: &Value) -> Result<Value, String> {
    match kind {
        "demo.echo" => Ok(json!({"echo": input})),
        _ => Ok(json!({"ok": true})),
    }
}
