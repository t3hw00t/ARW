use std::time::Instant;

use arw_topics as topics;
use arw_wasi::ToolHost;
use serde_json::{json, Value};

use crate::singleflight::FlightGuard;
use crate::tool_cache::{StoreOutcome, ToolCacheHit};
use crate::{capsule_guard, AppState};

mod guardrails;
pub(crate) use guardrails::metrics as guardrails_metrics_value;
mod error;
pub use error::ToolError;

const METRIC_CACHE_HIT: &str = "arw_tools_cache_hits";
const METRIC_CACHE_COALESCED: &str = "arw_tools_cache_coalesced";
const METRIC_CACHE_COALESCED_WAITERS: &str = "arw_tools_cache_coalesced_waiters";
const METRIC_CACHE_MISS: &str = "arw_tools_cache_miss";
const METRIC_CACHE_ERROR: &str = "arw_tools_cache_error";
const METRIC_CACHE_BYPASS: &str = "arw_tools_cache_bypass";

fn publish_cache_hit(
    bus: &arw_events::Bus,
    id: &str,
    key: &str,
    hit: &ToolCacheHit,
    outcome: &str,
    elapsed_ms: u64,
    latency_saved_ms: Option<u64>,
) -> Value {
    metrics::counter!(METRIC_CACHE_HIT, 1);
    if outcome == "coalesced" {
        metrics::counter!(METRIC_CACHE_COALESCED, 1);
    }

    let mut cache_evt = json!({
        "tool": id,
        "outcome": outcome,
        "elapsed_ms": elapsed_ms,
        "key": key,
        "digest": hit.digest,
        "cached": true,
        "age_secs": hit.age_secs,
    });
    if let Some(bytes) = hit.payload_bytes {
        cache_evt["payload_bytes"] = json!(bytes);
    }
    if let Some(saved) = latency_saved_ms {
        cache_evt["latency_saved_ms"] = json!(saved);
    }
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

    hit.value.clone()
}

pub async fn run_tool(state: &AppState, id: &str, input: Value) -> Result<Value, ToolError> {
    capsule_guard::refresh_capsules(state).await;
    let start = Instant::now();
    let bus = state.bus();
    let cache = state.tool_cache();
    let cacheable = cache.enabled() && cache.is_cacheable(id);
    let cache_key = cacheable.then(|| cache.action_key(id, &input));

    let mut flight_guard: Option<FlightGuard<'_>> = None;

    if let Some(ref key) = cache_key {
        if let Some(hit) = cache.lookup(key).await {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let saved_ms = cache.record_hit_metrics(key, &hit, elapsed_ms);
            let value = publish_cache_hit(&bus, id, key, &hit, "hit", elapsed_ms, saved_ms);
            return Ok(value);
        }

        loop {
            let guard = cache.begin_singleflight(key);
            if guard.is_leader() {
                flight_guard = Some(guard);
                break;
            }

            cache.record_coalesced_wait();
            metrics::counter!(METRIC_CACHE_COALESCED_WAITERS, 1);
            guard.wait().await;

            if let Some(hit) = cache.lookup(key).await {
                let elapsed_ms = start.elapsed().as_millis() as u64;
                let saved_ms = cache.record_hit_metrics(key, &hit, elapsed_ms);
                let value =
                    publish_cache_hit(&bus, id, key, &hit, "coalesced", elapsed_ms, saved_ms);
                return Ok(value);
            }
        }
    }

    let output = match run_tool_inner(state, id, &input).await {
        Ok(value) => value,
        Err(err) => {
            if let Some(mut guard) = flight_guard.take() {
                guard.notify_waiters();
            }
            return Err(err);
        }
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;

    if let Some(ref key) = cache_key {
        match cache.store(key, &output, elapsed_ms).await {
            Some(StoreOutcome {
                digest,
                cached: true,
                payload_bytes,
                miss_elapsed_ms,
            }) => {
                metrics::counter!(METRIC_CACHE_MISS, 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "miss",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": true,
                    "age_secs": Value::Null,
                    "payload_bytes": payload_bytes,
                    "miss_elapsed_ms": miss_elapsed_ms,
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            Some(StoreOutcome {
                digest,
                cached: false,
                payload_bytes,
                miss_elapsed_ms,
            }) => {
                metrics::counter!(METRIC_CACHE_ERROR, 1);
                let mut cache_evt = json!({
                    "tool": id,
                    "outcome": "error",
                    "elapsed_ms": elapsed_ms,
                    "key": key,
                    "digest": digest,
                    "cached": false,
                    "reason": "store_failed",
                    "payload_bytes": payload_bytes,
                    "miss_elapsed_ms": miss_elapsed_ms,
                });
                ensure_corr(&mut cache_evt);
                bus.publish(topics::TOPIC_TOOL_CACHE, &cache_evt);
            }
            None => {
                metrics::counter!(METRIC_CACHE_ERROR, 1);
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
        metrics::counter!(METRIC_CACHE_BYPASS, 1);
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

    if let Some(mut guard) = flight_guard.take() {
        guard.notify_waiters();
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
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    #[derive(Clone)]
    struct SlowHost {
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    impl SlowHost {
        fn new(delay: Duration) -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                delay,
            }
        }

        fn calls(&self) -> Arc<AtomicUsize> {
            self.calls.clone()
        }
    }

    #[async_trait]
    impl ToolHost for SlowHost {
        async fn run_tool(&self, id: &str, _input: &Value) -> Result<Value, arw_wasi::WasiError> {
            assert_eq!(id, "custom.test");
            self.calls.fetch_add(1, Ordering::Relaxed);
            sleep(self.delay).await;
            Ok(json!({"ok": true}))
        }
    }

    #[tokio::test]
    async fn singleflight_coalesces_identical_tool_runs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        ctx.env.set("ARW_TOOLS_CACHE_CAP", "8");
        ctx.env.set("ARW_TOOLS_CACHE_TTL_SECS", "60");
        ctx.env.set("ARW_TOOLS_CACHE_ALLOW", "custom.test");

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let slow_host = Arc::new(SlowHost::new(Duration::from_millis(50)));
        let host: Arc<dyn ToolHost> = slow_host.clone();

        let state = AppState::builder(bus, kernel, policy_handle, host, true)
            .with_sse_capacity(16)
            .build()
            .await;

        let state_a = state.clone();
        let state_b = state.clone();
        let input_a = json!({"value": 1});
        let input_b = json!({"value": 1});

        let fut1 = tokio::spawn(async move { run_tool(&state_a, "custom.test", input_a).await });
        let fut2 = tokio::spawn(async move { run_tool(&state_b, "custom.test", input_b).await });

        let (res1, res2) = tokio::join!(fut1, fut2);
        let out1 = res1.expect("task1").expect("run1");
        let out2 = res2.expect("task2").expect("run2");
        assert_eq!(out1, out2);
        let call_count = slow_host.calls();
        assert_eq!(call_count.load(Ordering::Relaxed), 1);

        let stats = state.tool_cache().stats();
        assert_eq!(stats.miss, 1);
        assert_eq!(stats.hit, 1);
        assert_eq!(stats.coalesced, 1);
    }

    #[cfg(not(feature = "tool_screenshots"))]
    #[tokio::test]
    async fn screenshot_tools_return_unsupported_without_feature() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _ctx = crate::test_support::begin_state_env(temp.path());

        let bus = arw_events::Bus::new_with_replay(8, 8);
        let kernel = arw_kernel::Kernel::open(temp.path()).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn ToolHost> = Arc::new(arw_wasi::NoopHost);

        let state = AppState::builder(bus, kernel, policy_handle, host, true)
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
