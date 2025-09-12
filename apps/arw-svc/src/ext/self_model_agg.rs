use serde_json::{json, Value};

// Env key for agent id to update; falls back to the seeded id.
fn agent_id() -> String {
    std::env::var("ARW_SELF_AGENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| std::env::var("ARW_SELF_SEED_ID").unwrap_or_else(|_| "dev-assistant".to_string()))
}

// Handle selected events to keep a lightweight competence map up-to-date.
pub async fn on_event(env: &arw_events::Envelope) {
    match env.kind.as_str() {
        "Tool.Ran" => {
            if let Some(tool_id) = env.payload.get("id").and_then(|v| v.as_str()) {
                let agent = agent_id();
                // Increment count and update last timestamp; keep success_rate optional (unknown failures)
                let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let _ = crate::ext::self_model_update_merge(&agent, |m| {
                    // m.competence_map[tool_id].count += 1; last = now
                    let map = m
                        .as_object_mut()
                        .unwrap()
                        .entry("competence_map")
                        .or_insert_with(|| json!({}));
                    let obj = map.as_object_mut().unwrap();
                    let ent = obj.entry(tool_id.to_string()).or_insert_with(|| json!({"count": 0}));
                    if let Some(o) = ent.as_object_mut() {
                        let cur = o.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                        o.insert("count".into(), Value::Number(cur.saturating_add(1).into()));
                        o.insert("last".into(), Value::String(now.clone()));
                    }
                })
                .await;
            }
        }
        _ => {}
    }
}

// Periodically project route stats into a coarse resource forecaster inside the self-model.
pub async fn start_periodic() {
    let agent = agent_id();
    let mut intv = tokio::time::interval(std::time::Duration::from_secs(
        std::env::var("ARW_SELF_AGG_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(20)
            .max(5),
    ));
    loop {
        intv.tick().await;
        let routes = super::stats::routes_for_analysis().await;
        // Map common recipes to routes (best-effort)
        let chat = routes.get("/admin/chat/send").cloned();
        let tools = routes.get("/admin/tools/run").cloned();
        if chat.is_none() && tools.is_none() {
            continue;
        }
        let mut patch = json!({"resource_curve": {"recipes": {}}});
        if let Some((ewma_ms, _hits, _errs)) = chat {
            patch["resource_curve"]["recipes"]["chat"]["latency_ms_mean"] = Value::Number((ewma_ms as u64).into());
        }
        if let Some((ewma_ms, _hits, _errs)) = tools {
            patch["resource_curve"]["recipes"]["tools"]["latency_ms_mean"] = Value::Number((ewma_ms as u64).into());
        }
        let _ = crate::ext::self_model_update_merge(&agent, |m| {
            // Deep-merge patch into model
            fn merge(a: &mut Value, b: &Value) {
                match (a, b) {
                    (Value::Object(ao), Value::Object(bo)) => {
                        for (k, bv) in bo { merge(ao.entry(k.clone()).or_insert(Value::Null), bv); }
                    }
                    (a, b) => *a = b.clone(),
                }
            }
            merge(m, &patch);
        })
        .await;
    }
}

