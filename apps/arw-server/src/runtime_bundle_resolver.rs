use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use serde_json::Value;
use tracing::{info, warn};

use arw_runtime::RuntimeDescriptor;

use crate::runtime_bundles::{RuntimeBundleInstallation, RuntimeBundleStore};
use crate::runtime_supervisor::{ManagedRuntimeDefinition, RuntimeSupervisor, SupervisorError};

const SOURCE_PREFIX: &str = "bundle:";
const PROCESS_ARGS_KEY: &str = "process.args";
const PROCESS_ENV_KEY: &str = "process.env";
const PROCESS_WORKDIR_KEY: &str = "process.workdir";
const PROCESS_COMMAND_KEY: &str = "process.command";
const PROCESS_HEALTH_URL_KEY: &str = "process.health.url";
const PROCESS_HEALTH_METHOD_KEY: &str = "process.health.method";
const PROCESS_HEALTH_STATUS_KEY: &str = "process.health.expect_status";
const PROCESS_HEALTH_BODY_KEY: &str = "process.health.expect_body";
const PROCESS_HEALTH_TIMEOUT_KEY: &str = "process.health.timeout_ms";

/// Ensure the runtime supervisor knows about any bundles staged on disk.
pub async fn reconcile(
    supervisor: Arc<RuntimeSupervisor>,
    store: Arc<RuntimeBundleStore>,
) -> Result<(), SupervisorError> {
    let installations = store.installations().await;
    let mut keep_ids: HashSet<String> = HashSet::new();

    for install in &installations {
        let Some((runtime_id, definition)) = definition_from_installation(install) else {
            continue;
        };
        keep_ids.insert(runtime_id.clone());
        match supervisor.install_definition(definition).await {
            Ok(_) => {
                info!(
                    target: "arw::runtime",
                    runtime = %runtime_id,
                    "registered bundle runtime descriptor"
                );
            }
            Err(err) => {
                warn!(
                    target: "arw::runtime",
                    runtime = %runtime_id,
                    error = %err,
                    "failed to register bundle runtime descriptor"
                );
            }
        }
    }

    let existing_ids = supervisor
        .runtime_ids_with_source_prefix(SOURCE_PREFIX)
        .await;
    for runtime_id in existing_ids {
        if keep_ids.contains(&runtime_id) {
            continue;
        }
        match supervisor.remove_definition(&runtime_id).await {
            Ok(_) => {
                info!(
                    target = "arw::runtime",
                    runtime = %runtime_id,
                    "removed bundle runtime descriptor"
                );
            }
            Err(err) => {
                warn!(
                    target = "arw::runtime",
                    runtime = %runtime_id,
                    error = %err,
                    "failed to remove bundle runtime descriptor"
                );
            }
        }
    }

    Ok(())
}

fn definition_from_installation(
    install: &RuntimeBundleInstallation,
) -> Option<(String, ManagedRuntimeDefinition)> {
    let bundle = install.bundle.as_ref()?;
    let runtime_id = bundle.id.clone();
    let adapter_id = if bundle.adapter.trim().is_empty() {
        "process".to_string()
    } else {
        bundle.adapter.clone()
    };

    let process_tags = process_tags_from_metadata(bundle.metadata.as_ref())?;

    let mut descriptor = RuntimeDescriptor::new(runtime_id.clone(), adapter_id.clone());
    descriptor.name = Some(bundle.name.clone());
    descriptor.profile = bundle.profiles.iter().cloned().next();
    descriptor.modalities = bundle.modalities.clone();
    descriptor.accelerator = bundle.accelerator.clone();

    descriptor
        .tags
        .insert("bundle.id".into(), bundle.id.clone());
    if let Some(channel) = install.channel.as_ref() {
        if !channel.is_empty() {
            descriptor
                .tags
                .insert("bundle.channel".into(), channel.clone());
        }
    }
    if let Some(root) = install.root.as_ref() {
        descriptor.tags.insert("bundle.root".into(), root.clone());
    }
    if let Some(installed_at) = install.installed_at.as_ref() {
        descriptor
            .tags
            .insert("bundle.installed_at".into(), installed_at.clone());
    }
    if let Some(imported_at) = install.imported_at.as_ref() {
        descriptor
            .tags
            .insert("bundle.imported_at".into(), imported_at.clone());
    }
    if let Some(meta_path) = install.metadata_path.as_ref() {
        descriptor
            .tags
            .insert("bundle.metadata_path".into(), meta_path.clone());
    }
    if let Some(source) = install
        .source
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
    {
        descriptor.tags.insert("bundle.source".into(), source);
    }
    if !install.artifacts.is_empty() {
        if let Ok(payload) = serde_json::to_string(&install.artifacts) {
            descriptor.tags.insert("bundle.artifacts".into(), payload);
        }
    }

    descriptor.tags.extend(process_tags);

    let definition = ManagedRuntimeDefinition::new(
        descriptor,
        adapter_id,
        false,
        bundle.profiles.iter().cloned().next(),
        Some(format!("{}{}", SOURCE_PREFIX, runtime_id)),
    );

    Some((runtime_id, definition))
}

fn process_tags_from_metadata(metadata: Option<&Value>) -> Option<BTreeMap<String, String>> {
    let process_obj = metadata
        .and_then(|meta| meta.get("process"))
        .and_then(Value::as_object)?;

    let command = process_obj
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)?;

    let mut tags = BTreeMap::new();
    tags.insert(PROCESS_COMMAND_KEY.into(), command);

    if let Some(args) = process_obj.get("args").and_then(Value::as_array) {
        let arg_list: Vec<String> = args
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        if !arg_list.is_empty() {
            if let Ok(payload) = serde_json::to_string(&arg_list) {
                tags.insert(PROCESS_ARGS_KEY.into(), payload);
            }
        }
    }

    if let Some(env) = process_obj.get("env").and_then(Value::as_object) {
        let env_map: BTreeMap<String, String> = env
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|val| (key.clone(), val.to_string())))
            .collect();
        if !env_map.is_empty() {
            if let Ok(payload) = serde_json::to_string(&env_map) {
                tags.insert(PROCESS_ENV_KEY.into(), payload);
            }
        }
    }

    if let Some(workdir) = process_obj.get("workdir").and_then(Value::as_str) {
        tags.insert(PROCESS_WORKDIR_KEY.into(), workdir.to_string());
    }

    if let Some(health) = process_obj.get("health").and_then(Value::as_object) {
        if let Some(url) = health.get("url").and_then(Value::as_str) {
            tags.insert(PROCESS_HEALTH_URL_KEY.into(), url.to_string());
        }
        if let Some(method) = health.get("method").and_then(Value::as_str) {
            tags.insert(PROCESS_HEALTH_METHOD_KEY.into(), method.to_string());
        }
        if let Some(status) = health.get("expect_status").and_then(Value::as_i64) {
            tags.insert(PROCESS_HEALTH_STATUS_KEY.into(), status.to_string());
        }
        if let Some(body) = health.get("expect_body").and_then(Value::as_str) {
            tags.insert(PROCESS_HEALTH_BODY_KEY.into(), body.to_string());
        }
        if let Some(timeout) = health.get("timeout_ms").and_then(Value::as_i64) {
            tags.insert(PROCESS_HEALTH_TIMEOUT_KEY.into(), timeout.to_string());
        }
    } else if let Some(health_probe) = process_obj.get("health_probe").and_then(Value::as_str) {
        tags.insert(PROCESS_HEALTH_URL_KEY.into(), health_probe.to_string());
        tags.insert(PROCESS_HEALTH_METHOD_KEY.into(), "GET".into());
        tags.insert(PROCESS_HEALTH_STATUS_KEY.into(), "200".into());
    }

    Some(tags)
}
