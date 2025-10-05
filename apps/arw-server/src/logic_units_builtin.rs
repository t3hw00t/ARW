use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{debug, warn};

use arw_topics as topics;

use crate::{
    api::config::{dot_to_pointer, ensure_path, get_by_dot, merge_values},
    AppState,
};

const NOC_MANIFEST_RAW: &str =
    include_str!("../../../interfaces/logic_units/never_out_of_context.json");

pub(crate) async fn seed(state: &AppState) {
    if let Err(err) = seed_internal(state).await {
        warn!(target: "logic_unit", error = %err, "failed to seed builtin logic units");
    }
}

async fn seed_internal(state: &AppState) -> Result<()> {
    let manifest: Value =
        serde_json::from_str(NOC_MANIFEST_RAW).context("parse never-out-of-context manifest")?;
    let id = manifest
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("never-out-of-context")
        .to_string();
    let status = manifest
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("installed")
        .to_string();

    let manifest_changed = upsert_manifest(state, &id, &manifest, &status).await?;
    let patches = manifest
        .get("patches")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let config_changed = apply_patches(state, &id, &patches).await?;

    if manifest_changed {
        state.bus().publish(
            topics::TOPIC_LOGICUNIT_INSTALLED,
            &json!({"id": id, "builtin": true}),
        );
    }
    if config_changed {
        debug!(target: "logic_unit", id, "builtin logic unit applied");
    }
    Ok(())
}

async fn upsert_manifest(
    state: &AppState,
    id: &str,
    manifest: &Value,
    status: &str,
) -> Result<bool> {
    let mut changed = true;
    if let Ok(existing) = state.kernel().list_logic_units_async(200).await {
        if let Some(entry) = existing
            .iter()
            .find(|item| item.get("id").and_then(Value::as_str) == Some(id))
        {
            let matches = entry.get("manifest") == Some(manifest)
                && entry
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .eq(status);
            changed = !matches;
        }
    }
    state
        .kernel()
        .insert_logic_unit_async(id.to_string(), manifest.clone(), status.to_string())
        .await
        .context("insert builtin logic unit")?;
    Ok(changed)
}

async fn apply_patches(state: &AppState, id: &str, patches: &[Value]) -> Result<bool> {
    if patches.is_empty() {
        return Ok(false);
    }
    let current_cfg = state.config_state().lock().await.clone();
    let mut cfg = current_cfg.clone();
    let mut diffs: Vec<Value> = Vec::new();

    for patch in patches {
        let target = patch.get("target").and_then(Value::as_str).unwrap_or("");
        if target.is_empty() {
            continue;
        }
        let op = patch.get("op").and_then(Value::as_str).unwrap_or("merge");
        let value = patch.get("value").cloned().unwrap_or_else(|| json!({}));
        if op != "merge" && op != "set" {
            continue;
        }
        let before = get_by_dot(&current_cfg, target).cloned();
        let dst = ensure_path(&mut cfg, target);
        match op {
            "set" => {
                *dst = value;
            }
            _ => {
                merge_values(dst, &value);
            }
        }
        let after = get_by_dot(&cfg, target).cloned();
        if before == after {
            continue;
        }
        diffs.push(json!({
            "target": target,
            "pointer": dot_to_pointer(target),
            "op": op,
            "before": before,
            "after": after,
        }));
    }

    if diffs.is_empty() {
        crate::config::apply_env_overrides_from(&cfg);
        return Ok(false);
    }

    let snapshot_id = if state.kernel_enabled() {
        match state
            .kernel()
            .insert_config_snapshot_async(cfg.clone())
            .await
        {
            Ok(id) => Some(id),
            Err(err) => {
                warn!(target: "logic_unit", error = %err, "failed to snapshot config for builtin logic unit");
                None
            }
        }
    } else {
        None
    };

    {
        let history = state.config_history();
        let mut hist = history.lock().await;
        hist.push((format!("logic_unit:{id}:builtin"), cfg.clone()));
    }
    {
        let cfg_state = state.config_state();
        let mut guard = cfg_state.lock().await;
        *guard = cfg.clone();
    }
    crate::config::apply_env_overrides_from(&cfg);

    let mut logic_payload = json!({"id": id, "ops": diffs.len(), "builtin": true});
    if let Some(ref snapshot) = snapshot_id {
        logic_payload["snapshot_id"] = json!(snapshot);
    }
    state
        .bus()
        .publish(topics::TOPIC_LOGICUNIT_APPLIED, &logic_payload);

    state.bus().publish(
        topics::TOPIC_CONFIG_PATCH_APPLIED,
        &json!({"ops": diffs.len(), "builtin": true}),
    );

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use crate::test_support::env as test_env;
    use arw_policy::PolicyEngine;
    use arw_wasi::NoopHost;
    use serde_json::{json, Value};
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn build_state(dir: &Path, env_guard: &mut test_env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        env_guard.set("ARW_ADMIN_TOKEN", "local");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_config_state(Arc::new(Mutex::new(json!({}))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    #[tokio::test]
    async fn builtin_manifest_applies_env_defaults() {
        test_support::init_tracing();
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        ctx.env.apply([
            ("ARW_CONTEXT_K", None),
            ("ARW_CONTEXT_SLOT_BUDGETS", None),
            ("ARW_CONTEXT_EXPAND_QUERY", None),
            ("ARW_REHYDRATE_FILE_HEAD_KB", None),
        ]);
        let state = build_state(temp.path(), &mut ctx.env).await;

        super::seed_internal(&state).await.expect("seed");

        assert_eq!(std::env::var("ARW_CONTEXT_K").unwrap(), "20");
        assert_eq!(std::env::var("ARW_CONTEXT_EXPAND_QUERY").unwrap(), "1");
        assert_eq!(std::env::var("ARW_REHYDRATE_FILE_HEAD_KB").unwrap(), "96");
        assert_eq!(
            std::env::var("ARW_CONTEXT_SLOT_BUDGETS").unwrap(),
            "{\"instructions\":2,\"plan\":3,\"policy\":2,\"safety\":2,\"notes\":3,\"memory\":3,\"evidence\":8}"
        );

        let cfg = state.config_state().lock().await.clone();
        let env_block = cfg
            .get("env")
            .and_then(Value::as_object)
            .expect("env block");
        assert_eq!(env_block.get("ARW_CONTEXT_K"), Some(&json!(20)));
        assert_eq!(
            env_block.get("ARW_CONTEXT_STREAM_DEFAULT"),
            Some(&json!(true))
        );
    }
}
