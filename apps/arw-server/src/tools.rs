use std::time::Instant;

use arw_topics as topics;
use arw_wasi::ToolHost;
use serde_json::{json, Value};

use crate::tool_cache::StoreOutcome;
use crate::{capsule_guard, AppState};

mod guardrails;
pub(crate) use guardrails::metrics as guardrails_metrics_value;
mod error;
pub use error::ToolError;

pub async fn run_tool(state: &AppState, id: &str, input: Value) -> Result<Value, ToolError> {
    capsule_guard::refresh_capsules(state).await;
    let start = Instant::now();
    let bus = state.bus();
    let cache = state.tool_cache();
    let cacheable = cache.enabled() && cache.is_cacheable(id);
    let cache_key = cacheable.then(|| cache.action_key(id, &input));

    if let Some(ref key) = cache_key {
        if let Some(hit) = cache.lookup(key).await {
            metrics::counter!("arw_tools_cache_hits", 1);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let mut cache_evt = json!({
                "tool": id,
                "outcome": "hit",
                "elapsed_ms": elapsed_ms,
                "key": key,
                "digest": hit.digest,
                "cached": true,
                "age_secs": hit.age_secs,
            });
            ensure_corr(&mut cache_evt);
            bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);

            let mut payload = json!({"id": id, "output": hit.value.clone()});
            ensure_corr(&mut payload);
            bus.publish(topics::TOPIC_TOOL_RAN, &payload);
            if id == "ui.screenshot.capture" {
                let mut shot = hit.value.clone();
                ensure_corr(&mut shot);
                bus.publish(topics::TOPIC_SCREENSHOTS_CAPTURED, &shot);
            }
            return Ok(hit.value);
        }
    }

    let output = run_tool_inner(state, id, &input).await?;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    if let Some(ref key) = cache_key {
        match cache.store(key, &output).await {
            Some(StoreOutcome {
                digest,
                cached: true,
            }) => {
                metrics::counter!("arw_tools_cache_miss", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "miss",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": true,
                    "age_secs": Value::Null,
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            Some(StoreOutcome {
                digest,
                cached: false,
            }) => {
                metrics::counter!("arw_tools_cache_error", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "error",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": false,
                    "reason": "store_failed",
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            None => {
                metrics::counter!("arw_tools_cache_error", 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "error",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "cached": false,
                    "reason": "serialize_failed",
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
        }
    } else if cache.enabled() {
        cache.record_bypass();
        metrics::counter!("arw_tools_cache_bypass", 1);
        let mut cache_evt = json!({
            "tool": id,
            "outcome": "not_cacheable",
            "elapsed_ms": elapsed_ms,
            "cached": false,
            "reason": "not_cacheable",
        });
        ensure_corr(&mut cache_evt);
        bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
    }

    let mut payload = json!({"id": id, "output": output.clone()});
    ensure_corr(&mut payload);
    bus.publish(topics::TOPIC_TOOL_RAN, &payload);
    if id == "ui.screenshot.capture" {
        let mut shot = output.clone();
        ensure_corr(&mut shot);
        bus.publish(topics::TOPIC_SCREENSHOTS_CAPTURED, &shot);
    }

    Ok(output)
}

async fn run_tool_inner(state: &AppState, id: &str, input: &Value) -> Result<Value, ToolError> {
    match id {
        "ui.screenshot.capture" => screenshots::capture(input.clone()).await,
        "ui.screenshot.annotate_burn" => screenshots::annotate(input.clone()).await,
        "ui.screenshot.ocr" => screenshots::ocr(input.clone()).await,
        "guardrails.check" => guardrails::run(input).await,
        "chat.respond" => crate::chat::run_chat_tool(state, input.clone()).await,
        "demo.echo" => Ok(json!({"echo": input.clone()})),
        "introspect.tools" => serde_json::to_value(arw_core::introspect_tools())
            .map_err(|e| ToolError::Runtime(e.to_string())),
        _ => run_host_tool(state.host(), id, input).await,
    }
}

async fn run_host_tool(
    host: std::sync::Arc<dyn ToolHost>,
    id: &str,
    input: &Value,
) -> Result<Value, ToolError> {
    host.run_tool(id, input).await.map_err(Into::into)
}

pub fn ensure_corr(value: &mut Value) {
    if let Value::Object(map) = value {
        if !map.contains_key("corr_id") {
            map.insert(
                "corr_id".into(),
                Value::String(uuid::Uuid::new_v4().to_string()),
            );
        }
    }
}
#[cfg(feature = "tool_screenshots")]
mod screenshots;

#[cfg(not(feature = "tool_screenshots"))]
mod screenshots_disabled;

#[cfg(not(feature = "tool_screenshots"))]
use screenshots_disabled as screenshots;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppState;
    use arw_policy::PolicyEngine;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[cfg(not(feature = "tool_screenshots"))]
    #[tokio::test]
    async fn screenshot_tools_return_unsupported_without_feature() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::env::set_var("ARW_STATE_DIR", temp.path().display().to_string());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);

        let state = AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await;

        let err = run_tool(&state, "ui.screenshot.capture", json!({}))
            .await
            .expect_err("tool should be unsupported without feature");
        match err {
            ToolError::Unsupported(msg) => {
                assert!(msg.contains("tool_screenshots"));
            }
            other => panic!("expected unsupported, got {other:?}"),
        }
    }
}
