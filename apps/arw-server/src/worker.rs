use chrono::SecondsFormat;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time;

use crate::{
    app_state::Policy,
    egress_log::{self, EgressRecord},
    guard_metadata::apply_posture_and_guard,
    tasks::TaskHandle,
    tools::{self, ToolError},
    util, AppState,
};
use arw_topics as topics;

pub(crate) fn start_local_worker(state: AppState) -> TaskHandle {
    let ctx = WorkerContext::new(&state);
    let worker_state = state;
    TaskHandle::new(
        "worker.local",
        tokio::spawn(async move {
            let state = worker_state;
            let ctx = ctx;
            loop {
                match ctx.kernel.dequeue_one_queued_async().await {
                    Ok(Some((id, kind, input))) => {
                        let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
                        let running_env = arw_events::Envelope {
                            time: now,
                            kind: topics::TOPIC_ACTIONS_RUNNING.into(),
                            payload: json!({"id": id.clone()}),
                            policy: None,
                            ce: None,
                        };
                        ctx.bus.publish(&running_env.kind, &running_env.payload);

                        let action_result = ctx.handle_action(&state, &id, &kind, &input).await;
                        match action_result {
                            Ok(outcome) => {
                                ctx.complete_action(&id, outcome).await;
                            }
                            Err(failure) => {
                                ctx.fail_action(&id, &kind, failure).await;
                                continue;
                            }
                        }
                    }
                    Ok(None) => time::sleep(Duration::from_millis(200)).await,
                    Err(err) => {
                        tracing::warn!(
                            target: "arw::worker",
                            error = ?err,
                            "kernel dequeue failed; retrying",
                        );
                        time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }),
    )
}

#[derive(Clone)]
struct WorkerContext {
    bus: arw_events::Bus,
    kernel: arw_kernel::Kernel,
    policy: Arc<tokio::sync::Mutex<Policy>>,
    host: Arc<dyn arw_wasi::ToolHost>,
}

impl WorkerContext {
    fn new(state: &AppState) -> Self {
        Self {
            bus: state.bus(),
            kernel: state.kernel().clone(),
            policy: state.policy(),
            host: state.host(),
        }
    }

    async fn handle_action(
        &self,
        state: &AppState,
        id: &str,
        kind: &str,
        input: &Value,
    ) -> Result<ActionOutcome, ActionFailure> {
        match kind {
            k if k.starts_with("net.http.") => self.handle_http_action(id, k, input).await,
            "fs.patch" => self.handle_fs_patch(input).await,
            "app.vscode.open" => self.handle_app_vscode_open(input).await,
            _ => match execute_dynamic_action(state, kind, input).await {
                Ok(value) => Ok(ActionOutcome::new(value)),
                Err(err) => Err(ActionFailure::new(err)),
            },
        }
    }

    async fn complete_action(&self, id: &str, outcome: ActionOutcome) {
        let ActionOutcome {
            output,
            posture,
            guard,
        } = outcome;

        let posture_value = posture.unwrap_or_else(util::effective_posture);
        let stored_output = enrich_output(output.clone(), guard.clone(), &posture_value);
        let _ = self
            .kernel
            .update_action_result_async(id.to_string(), Some(stored_output), None)
            .await;
        let _ = self.kernel.set_action_state_async(id, "completed").await;

        let mut completed_payload = serde_json::Map::new();
        completed_payload.insert("id".into(), Value::String(id.to_string()));
        completed_payload.insert("output".into(), output);
        apply_posture_and_guard(
            &mut completed_payload,
            Some(posture_value.as_str()),
            guard.as_ref().map(|g| g.to_external_value()),
            true,
        );
        let completed_payload = Value::Object(completed_payload);
        let completed_at = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let completed_env = arw_events::Envelope {
            time: completed_at,
            kind: topics::TOPIC_ACTIONS_COMPLETED.into(),
            payload: completed_payload,
            policy: None,
            ce: None,
        };
        self.bus
            .publish(&completed_env.kind, &completed_env.payload);

        let _ = self
            .kernel
            .append_contribution_async("local", "task.complete", 1.0, "task", None, None, None)
            .await;
    }

    async fn fail_action(&self, id: &str, kind: &str, failure: ActionFailure) {
        let ActionFailure {
            error,
            guard,
            posture,
        } = failure;
        let posture_value = posture.unwrap_or_else(util::effective_posture);

        let (error_msg, mut event_payload, denied) = tool_error_details(id, kind, &error);
        if let Value::Object(ref mut obj) = event_payload {
            apply_posture_and_guard(
                obj,
                Some(posture_value.as_str()),
                guard.as_ref().map(|g| g.to_external_value()),
                true,
            );
        }
        if let Some(ref guard_meta) = guard {
            tracing::debug!(
                target: "arw::worker",
                %id,
                %kind,
                allowed = guard_meta.allowed,
                policy_allow = guard_meta.policy_allow,
                posture = %posture_value,
                required_caps = ?guard_meta.required_capabilities,
                lease_capability = guard_meta.lease.as_ref().map(|l| l.capability.as_str()),
                "action guard metadata on failure",
            );
        } else {
            tracing::debug!(
                target: "arw::worker",
                %id,
                %kind,
                posture = %posture_value,
                "action failure without guard metadata",
            );
        }
        tracing::warn!(target: "arw::worker", %id, %kind, "action failed: {}", error_msg);

        let mut failure_body = serde_json::Map::new();
        if let Some(err_value) = event_payload.get("error") {
            failure_body.insert("error".into(), err_value.clone());
        } else {
            failure_body.insert("error".into(), Value::String(error_msg.clone()));
        }
        apply_posture_and_guard(
            &mut failure_body,
            Some(posture_value.as_str()),
            guard.as_ref().map(|g| g.to_internal_value()),
            true,
        );
        let failure_output = Value::Object(failure_body);

        let _ = self
            .kernel
            .update_action_result_async(
                id.to_string(),
                Some(failure_output),
                Some(error_msg.clone()),
            )
            .await;
        let _ = self.kernel.set_action_state_async(id, "failed").await;

        tools::ensure_corr(&mut event_payload);
        self.bus
            .publish(topics::TOPIC_ACTIONS_FAILED, &event_payload);

        if let Some(denied) = denied {
            let record = EgressRecord {
                decision: "deny",
                reason: Some(denied.reason.as_str()),
                dest_host: denied.dest_host.as_deref(),
                dest_port: denied.dest_port,
                protocol: denied.protocol.as_deref(),
                bytes_in: None,
                bytes_out: None,
                corr_id: Some(id),
                project: None,
                meta: None,
            };
            let _ = egress_log::record(
                Some(&self.kernel),
                &self.bus,
                Some(posture_value.as_str()),
                &record,
                false,
                true,
            )
            .await;
        }
    }

    async fn handle_http_action(
        &self,
        id: &str,
        kind: &str,
        input: &Value,
    ) -> Result<ActionOutcome, ActionFailure> {
        let mut input_with_headers = input.clone();
        let project = std::env::var("ARW_PROJECT_ID").ok();
        if let Some(obj) = input_with_headers.as_object_mut() {
            let headers = obj.entry("headers").or_insert_with(|| json!({}));
            if let Some(map) = headers.as_object_mut() {
                map.insert("X-ARW-Corr".to_string(), json!(id));
                if let Some(ref project_id) = project {
                    map.insert("X-ARW-Project".to_string(), json!(project_id));
                }
                if !map.contains_key("X-ARW-Method") {
                    if let Some(method) = kind.rsplit('.').next() {
                        map.insert("X-ARW-Method".to_string(), json!(method.to_uppercase()));
                    }
                }
            }
        }

        let guard = self
            .guard_action("net.http.", &["net:http", "io:egress"])
            .await;
        if !guard.allowed {
            let posture = util::effective_posture();
            let record = EgressRecord {
                decision: "deny",
                reason: Some("no_lease"),
                dest_host: None,
                dest_port: None,
                protocol: Some("http"),
                bytes_in: None,
                bytes_out: None,
                corr_id: Some(id),
                project: project.as_deref(),
                meta: None,
            };
            self.record_egress(Some(posture.as_str()), &record, true, true)
                .await;
            return Ok(ActionOutcome::new(
                json!({"error":"lease required: net:http or io:egress"}),
            )
            .with_posture(posture)
            .with_guard(guard));
        }

        if let Some(connector_id) = input_with_headers
            .get("connector_id")
            .and_then(|v| v.as_str())
        {
            if let Err(outcome) = self.ensure_connector_scopes(&guard, connector_id).await {
                return Ok(outcome);
            }
        }

        match self.host.run_tool("http.fetch", &input_with_headers).await {
            Ok(value) => {
                let host_name = value.get("dest_host").and_then(|x| x.as_str());
                let port = value.get("dest_port").and_then(|x| x.as_i64());
                let proto = value.get("protocol").and_then(|x| x.as_str());
                let bytes_in = value.get("bytes_in").and_then(|x| x.as_i64());
                let bytes_out = value.get("bytes_out").and_then(|x| x.as_i64());
                let posture = util::effective_posture();
                let record = EgressRecord {
                    decision: "allow",
                    reason: Some("ok"),
                    dest_host: host_name,
                    dest_port: port,
                    protocol: proto,
                    bytes_in,
                    bytes_out,
                    corr_id: Some(id),
                    project: project.as_deref(),
                    meta: None,
                };
                self.record_egress(Some(posture.as_str()), &record, false, true)
                    .await;
                Ok(ActionOutcome::new(value)
                    .with_posture(posture)
                    .with_guard(guard))
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
                    corr_id: Some(id),
                    project: project.as_deref(),
                    meta: None,
                };
                self.record_egress(Some(posture.as_str()), &record, false, true)
                    .await;
                Ok(
                    ActionOutcome::new(json!({"error":"denied","reason": reason}))
                        .with_posture(posture)
                        .with_guard(guard),
                )
            }
            Err(err) => Ok(ActionOutcome::new(
                json!({"error":"runtime","detail": err.to_string()}),
            )
            .with_guard(guard)),
        }
    }

    async fn ensure_connector_scopes(
        &self,
        guard: &ActionGuard,
        connector_id: &str,
    ) -> Result<(), ActionOutcome> {
        let manifest = match util::load_connector_manifest(connector_id).await {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(
                    target: "arw::worker",
                    %connector_id,
                    "failed to load connector manifest: {err}"
                );
                let outcome = ActionOutcome::new(json!({
                    "error": "connector manifest unavailable",
                    "connector_id": connector_id,
                }))
                .with_posture(util::effective_posture())
                .with_guard(guard.clone());
                return Err(outcome);
            }
        };

        let scopes: Vec<String> = manifest
            .get("scopes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();

        if scopes.is_empty() {
            return Ok(());
        }

        let mut missing: Vec<String> = Vec::new();
        for capability in &scopes {
            match self
                .kernel
                .find_valid_lease_async("local", capability)
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) => missing.push(capability.clone()),
                Err(err) => {
                    tracing::warn!(
                        target: "arw::worker",
                        %connector_id,
                        %capability,
                        "connector scope lease check failed: {err}"
                    );
                    let outcome = ActionOutcome::new(json!({
                        "error": "connector lease check failed",
                        "connector_id": connector_id,
                        "capability": capability,
                        "detail": err.to_string(),
                    }))
                    .with_posture(util::effective_posture())
                    .with_guard(guard.clone());
                    return Err(outcome);
                }
            }
        }

        if missing.is_empty() {
            return Ok(());
        }

        let missing_caps = missing.clone();
        self.bus.publish(
            topics::TOPIC_POLICY_DECISION,
            &json!({
                "action": format!("connector.{connector_id}"),
                "allow": false,
                "require_capability": missing_caps,
                "explain": {"reason": "connector_scopes"},
            }),
        );

        let posture = util::effective_posture();
        let outcome = ActionOutcome::new(json!({
            "error": "connector lease required",
            "connector_id": connector_id,
            "missing_scopes": missing,
        }))
        .with_posture(posture)
        .with_guard(guard.clone());
        Err(outcome)
    }

    async fn handle_fs_patch(&self, input: &Value) -> Result<ActionOutcome, ActionFailure> {
        let guard = self.guard_action("fs.patch", &["fs", "fs:patch"]).await;
        if !guard.allowed {
            self.bus.publish(
                topics::TOPIC_POLICY_DECISION,
                &json!({
                    "action": "fs.patch",
                    "allow": false,
                    "require_capability": "fs|fs:patch",
                    "explain": {"reason":"lease_required"}
                }),
            );
            return Ok(
                ActionOutcome::new(json!({"error":"lease required: fs or fs:patch"}))
                    .with_guard(guard),
            );
        }

        match self.host.run_tool("fs.patch", input).await {
            Ok(value) => {
                let path_s = value.get("path").and_then(|x| x.as_str()).unwrap_or("");
                self.bus.publish(
                    topics::TOPIC_PROJECTS_FILE_WRITTEN,
                    &json!({"path": path_s, "sha256": value.get("sha256") }),
                );
                Ok(ActionOutcome::new(value)
                    .with_posture(util::effective_posture())
                    .with_guard(guard))
            }
            Err(err) => Ok(ActionOutcome::new(
                json!({"error":"runtime","detail": err.to_string()}),
            )
            .with_guard(guard)),
        }
    }

    async fn handle_app_vscode_open(&self, input: &Value) -> Result<ActionOutcome, ActionFailure> {
        let guard = self
            .guard_action("app.vscode.open", &["io:app:vscode", "io:app"])
            .await;
        if !guard.allowed {
            self.bus.publish(
                topics::TOPIC_POLICY_DECISION,
                &json!({
                    "action": "app.vscode.open",
                    "allow": false,
                    "require_capability": "io:app:vscode|io:app",
                    "explain": {"reason":"lease_required"}
                }),
            );
            return Ok(ActionOutcome::new(
                json!({"error":"lease required: io:app:vscode or io:app"}),
            )
            .with_guard(guard));
        }

        match self.host.run_tool("app.vscode.open", input).await {
            Ok(value) => {
                let path_s = input.get("path").and_then(|x| x.as_str()).unwrap_or("");
                self.bus
                    .publish(topics::TOPIC_APPS_VSCODE_OPENED, &json!({"path": path_s }));
                Ok(ActionOutcome::new(value)
                    .with_posture(util::effective_posture())
                    .with_guard(guard))
            }
            Err(err) => Ok(ActionOutcome::new(
                json!({"error":"runtime","detail": err.to_string()}),
            )
            .with_guard(guard)),
        }
    }

    async fn guard_action(&self, action: &str, capabilities: &[&str]) -> ActionGuard {
        let decision = self.policy.lock().await.evaluate_action(action);
        if decision.allow {
            return ActionGuard {
                allowed: true,
                policy_allow: true,
                required_capabilities: Vec::new(),
                lease: None,
            };
        }

        let required = capabilities.iter().map(|c| c.to_string()).collect();
        match self.has_any_capability(capabilities).await {
            Some(lease) => ActionGuard {
                allowed: true,
                policy_allow: false,
                required_capabilities: required,
                lease: Some(lease),
            },
            None => ActionGuard {
                allowed: false,
                policy_allow: false,
                required_capabilities: required,
                lease: None,
            },
        }
    }

    async fn has_any_capability(&self, capabilities: &[&str]) -> Option<LeaseSummary> {
        for capability in capabilities {
            if let Ok(Some(lease_json)) = self
                .kernel
                .find_valid_lease_async("local", capability)
                .await
            {
                if let Some(summary) = LeaseSummary::from_value(&lease_json) {
                    return Some(summary);
                }
            }
        }
        None
    }

    async fn record_egress(
        &self,
        posture: Option<&str>,
        record: &EgressRecord<'_>,
        force: bool,
        emit_event: bool,
    ) -> Option<i64> {
        egress_log::record(
            Some(&self.kernel),
            &self.bus,
            posture,
            record,
            force,
            emit_event,
        )
        .await
    }
}

struct ActionOutcome {
    output: Value,
    posture: Option<String>,
    guard: Option<ActionGuard>,
}

impl ActionOutcome {
    fn new(output: Value) -> Self {
        Self {
            output,
            posture: None,
            guard: None,
        }
    }

    fn with_posture(mut self, posture: String) -> Self {
        self.posture = Some(posture);
        self
    }

    fn with_guard(mut self, guard: ActionGuard) -> Self {
        self.guard = Some(guard);
        self
    }
}

#[derive(Debug)]
struct ActionFailure {
    error: ToolError,
    guard: Option<ActionGuard>,
    posture: Option<String>,
}

impl ActionFailure {
    fn new(error: ToolError) -> Self {
        Self {
            error,
            guard: None,
            posture: None,
        }
    }

    #[allow(dead_code)]
    fn with_guard(mut self, guard: ActionGuard) -> Self {
        self.guard = Some(guard);
        self
    }

    #[allow(dead_code)]
    fn with_posture<S: Into<String>>(mut self, posture: S) -> Self {
        self.posture = Some(posture.into());
        self
    }

    #[allow(dead_code)]
    fn with_optional_guard(mut self, guard: Option<ActionGuard>) -> Self {
        self.guard = guard;
        self
    }
}

#[derive(Clone, Debug)]
struct ActionGuard {
    allowed: bool,
    policy_allow: bool,
    required_capabilities: Vec<String>,
    lease: Option<LeaseSummary>,
}

impl ActionGuard {
    fn to_internal_value(&self) -> Value {
        let lease_value = self
            .lease
            .as_ref()
            .map(|lease| lease.to_internal_value())
            .unwrap_or(Value::Null);
        json!({
            "allowed": self.allowed,
            "policy_allow": self.policy_allow,
            "required_capabilities": self.required_capabilities,
            "lease": lease_value,
        })
    }

    fn to_external_value(&self) -> Value {
        let lease_value = self
            .lease
            .as_ref()
            .map(|lease| lease.to_external_value())
            .unwrap_or(Value::Null);
        json!({
            "allowed": self.allowed,
            "policy_allow": self.policy_allow,
            "required_capabilities": self.required_capabilities,
            "lease": lease_value,
        })
    }
}

fn enrich_output(value: Value, guard: Option<ActionGuard>, posture: &str) -> Value {
    match value {
        Value::Object(mut map) => {
            apply_posture_and_guard(
                &mut map,
                Some(posture),
                guard.as_ref().map(|g| g.to_internal_value()),
                false,
            );
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("value".into(), other);
            apply_posture_and_guard(
                &mut map,
                Some(posture),
                guard.as_ref().map(|g| g.to_internal_value()),
                true,
            );
            Value::Object(map)
        }
    }
}

#[derive(Clone, Debug)]
struct LeaseSummary {
    id: String,
    subject: Option<String>,
    capability: String,
    scope: Option<String>,
    ttl_until: String,
}

impl LeaseSummary {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            id: value.get("id")?.as_str()?.to_string(),
            subject: value
                .get("subject")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            capability: value.get("capability")?.as_str()?.to_string(),
            scope: value
                .get("scope")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            ttl_until: value.get("ttl_until")?.as_str()?.to_string(),
        })
    }

    fn to_internal_value(&self) -> Value {
        json!({
            "id": self.id,
            "subject": self.subject,
            "capability": self.capability,
            "scope": self.scope,
            "ttl_until": self.ttl_until,
        })
    }

    fn to_external_value(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("capability".into(), Value::String(self.capability.clone()));
        obj.insert("ttl_until".into(), Value::String(self.ttl_until.clone()));
        if let Some(scope) = &self.scope {
            obj.insert("scope".into(), Value::String(scope.clone()));
        }
        Value::Object(obj)
    }
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
    use async_trait::async_trait;
    use chrono::{Duration as ChronoDuration, Utc};
    use once_cell::sync::Lazy;
    use serde_json::json;
    use std::sync::{Arc, Mutex as StdMutex, MutexGuard as StdMutexGuard};
    use tokio::sync::Mutex;
    use tokio::time::{timeout, Duration};
    use uuid::Uuid;

    async fn build_state(path: &std::path::Path) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        build_state_with_host(path, host).await
    }

    async fn build_state_with_host(
        path: &std::path::Path,
        host: Arc<dyn arw_wasi::ToolHost>,
    ) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        std::env::set_var("ARW_STATE_DIR", path.display().to_string());
        let bus = arw_events::Bus::new_with_replay(32, 32);
        let kernel = arw_kernel::Kernel::open(path).expect("init kernel");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_sse_capacity(16)
            .build()
            .await
    }

    static ENV_MUTEX: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

    struct EnvVarGuard {
        key: &'static str,
        _lock: StdMutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = ENV_MUTEX.lock().expect("env mutex poisoned");
            std::env::set_var(key, value);
            Self { key, _lock: lock }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.key);
        }
    }

    #[derive(Clone, Default)]
    struct AllowingHost;

    #[async_trait]
    impl arw_wasi::ToolHost for AllowingHost {
        async fn run_tool(
            &self,
            id: &str,
            _input: &serde_json::Value,
        ) -> Result<serde_json::Value, arw_wasi::WasiError> {
            match id {
                "http.fetch" => Ok(json!({
                    "dest_host": "example.com",
                    "dest_port": 443,
                    "protocol": "https",
                    "bytes_in": 2048,
                    "bytes_out": 512,
                })),
                "fs.patch" => Ok(json!({"path": "/tmp/file.txt", "sha256": "abc123"})),
                "app.vscode.open" => Ok(json!({"opened": true})),
                _ => Err(arw_wasi::WasiError::Unsupported(id.to_string())),
            }
        }
    }

    #[tokio::test]
    async fn unsupported_tool_marks_action_failed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
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
        assert!(env.payload["posture"].as_str().is_some());
        assert!(env.payload.get("guard").is_none());

        let stored = state
            .kernel()
            .get_action_async(&action_id)
            .await
            .expect("get action")
            .expect("action row");
        assert_eq!(stored.state, "failed");
        let error_text = stored.error.unwrap_or_default();
        assert!(error_text.contains("unsupported"));
        let stored_output = stored.output.expect("stored output");
        assert_eq!(stored_output["error"]["type"].as_str(), Some("unsupported"));
        assert!(stored_output["posture"].as_str().is_some());
        assert!(stored_output.get("guard").is_none());
    }

    #[tokio::test]
    async fn guard_action_respects_leases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;
        let ctx = WorkerContext::new(&state);

        assert!(
            !ctx.guard_action("fs.patch", &["fs", "fs:patch"])
                .await
                .allowed
        );

        let ttl = (Utc::now() + ChronoDuration::minutes(5)).to_rfc3339();
        state
            .kernel()
            .insert_lease_async(
                Uuid::new_v4().to_string(),
                "local".into(),
                "fs".into(),
                None,
                ttl,
                None,
                None,
            )
            .await
            .expect("insert lease");

        assert!(
            ctx.guard_action("fs.patch", &["fs", "fs:patch"])
                .await
                .allowed
        );
    }

    #[tokio::test]
    async fn connector_requires_scope_lease() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;

        let connectors_dir = util::state_dir().join("connectors");
        tokio::fs::create_dir_all(&connectors_dir)
            .await
            .expect("create connectors dir");
        let manifest = json!({
            "id": "gh-main",
            "kind": "cloud",
            "provider": "github",
            "scopes": ["cloud:github:repo:rw"],
            "meta": json!({})
        });
        tokio::fs::write(
            connectors_dir.join("gh-main.json"),
            serde_json::to_vec(&manifest).expect("manifest bytes"),
        )
        .await
        .expect("write manifest");

        let ttl = (Utc::now() + ChronoDuration::minutes(5)).to_rfc3339();
        state
            .kernel()
            .insert_lease_async(
                Uuid::new_v4().to_string(),
                "local".into(),
                "net:http".into(),
                None,
                ttl,
                None,
                None,
            )
            .await
            .expect("insert lease");

        let ctx = WorkerContext::new(&state);
        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_POLICY_DECISION.to_string()], Some(8));

        let outcome = ctx
            .handle_http_action(
                "conn-test",
                "net.http.get",
                &json!({
                    "url": "https://api.github.com",
                    "method": "GET",
                    "connector_id": "gh-main"
                }),
            )
            .await
            .expect("http fetch");

        assert_eq!(
            outcome.output["error"].as_str(),
            Some("connector lease required")
        );
        let missing = outcome.output["missing_scopes"]
            .as_array()
            .expect("missing scopes");
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].as_str(), Some("cloud:github:repo:rw"));

        let decision = timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("policy event timeout")
            .expect("policy event");
        assert_eq!(decision.kind, topics::TOPIC_POLICY_DECISION);
        assert_eq!(
            decision.payload["action"].as_str(),
            Some("connector.gh-main")
        );
        assert_eq!(
            decision.payload["require_capability"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str()),
            Some("cloud:github:repo:rw")
        );
    }

    #[tokio::test]
    async fn http_get_records_egress_on_success() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let _ledger_guard = EnvVarGuard::set("ARW_EGRESS_LEDGER_ENABLE", "1");
        let state = build_state_with_host(temp.path(), Arc::new(AllowingHost)).await;
        let ctx = WorkerContext::new(&state);

        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![topics::TOPIC_EGRESS_LEDGER_APPENDED.to_string()],
            Some(8),
        );

        let ttl = (Utc::now() + ChronoDuration::minutes(5)).to_rfc3339();
        state
            .kernel()
            .insert_lease_async(
                Uuid::new_v4().to_string(),
                "local".into(),
                "net:http".into(),
                None,
                ttl,
                None,
                None,
            )
            .await
            .expect("insert lease");

        let outcome = ctx
            .handle_http_action(
                "action-allow",
                "net.http.get",
                &json!({"url": "https://example.com", "headers": {}}),
            )
            .await
            .expect("http fetch");
        assert!(outcome.output.get("error").is_none());
        assert_eq!(outcome.guard.as_ref().map(|g| g.allowed), Some(true));

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("event recv")
            .expect("event value");
        assert_eq!(env.kind, topics::TOPIC_EGRESS_LEDGER_APPENDED);
        assert_eq!(env.payload["decision"].as_str(), Some("allow"));
        assert_eq!(env.payload["corr_id"].as_str(), Some("action-allow"));

        let ledger = state
            .kernel()
            .list_egress_async(1)
            .await
            .expect("ledger list");
        let entry = ledger.first().expect("entry");
        assert_eq!(entry["decision"].as_str(), Some("allow"));
        assert_eq!(entry["corr_id"].as_str(), Some("action-allow"));
        assert_eq!(entry["bytes_in"].as_i64(), Some(2048));
        assert_eq!(entry["bytes_out"].as_i64(), Some(512));
    }

    #[tokio::test]
    async fn http_get_denied_without_lease_records_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let _ledger_guard = EnvVarGuard::set("ARW_EGRESS_LEDGER_ENABLE", "0");
        let state = build_state_with_host(temp.path(), Arc::new(AllowingHost)).await;
        let ctx = WorkerContext::new(&state);

        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![topics::TOPIC_EGRESS_LEDGER_APPENDED.to_string()],
            Some(8),
        );

        let outcome = ctx
            .handle_http_action(
                "action-deny",
                "net.http.get",
                &json!({"url": "https://example.com", "headers": {}}),
            )
            .await
            .expect("http fetch");
        assert_eq!(
            outcome.output.get("error").and_then(|v| v.as_str()),
            Some("lease required: net:http or io:egress"),
        );
        assert_eq!(outcome.guard.as_ref().map(|g| g.allowed), Some(false));

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("event recv")
            .expect("event value");
        assert_eq!(env.kind, topics::TOPIC_EGRESS_LEDGER_APPENDED);
        assert_eq!(env.payload["decision"].as_str(), Some("deny"));
        assert_eq!(env.payload["reason"].as_str(), Some("no_lease"));
        assert_eq!(env.payload["corr_id"].as_str(), Some("action-deny"));

        let ledger = state
            .kernel()
            .list_egress_async(1)
            .await
            .expect("ledger list");
        let entry = ledger.first().expect("entry");
        assert_eq!(entry["decision"].as_str(), Some("deny"));
        assert_eq!(entry["reason"].as_str(), Some("no_lease"));
        assert_eq!(entry["corr_id"].as_str(), Some("action-deny"));
    }

    #[tokio::test]
    async fn completed_event_includes_guard_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let _ledger_guard = EnvVarGuard::set("ARW_EGRESS_LEDGER_ENABLE", "0");
        let state = build_state_with_host(temp.path(), Arc::new(AllowingHost)).await;

        let mut rx = state
            .bus()
            .subscribe_filtered(vec![topics::TOPIC_ACTIONS_COMPLETED.to_string()], Some(8));

        let ttl = (Utc::now() + ChronoDuration::minutes(5)).to_rfc3339();
        state
            .kernel()
            .insert_lease_async(
                Uuid::new_v4().to_string(),
                "local".into(),
                "net:http".into(),
                None,
                ttl,
                None,
                None,
            )
            .await
            .expect("insert lease");

        let worker = start_local_worker(state.clone());

        let action_id = Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(
                &action_id,
                "net.http.get",
                &json!({"url": "https://example.com", "headers": {}}),
                None,
                None,
                "queued",
            )
            .await
            .expect("enqueue action");

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("completed event recv")
            .expect("event value");
        let (_, _, handle) = worker.into_inner();
        handle.abort();

        assert_eq!(env.kind, topics::TOPIC_ACTIONS_COMPLETED);
        assert_eq!(env.payload["id"].as_str(), Some(action_id.as_str()));
        assert!(env.payload["posture"].as_str().is_some());
        assert_eq!(env.payload["guard"]["allowed"].as_bool(), Some(true));
        assert_eq!(
            env.payload["guard"]["lease"]["capability"].as_str(),
            Some("net:http"),
        );
        assert!(env.payload["guard"]["required_capabilities"]
            .as_array()
            .map(|v| !v.is_empty())
            .unwrap_or(false));

        let stored = state
            .kernel()
            .get_action_async(&action_id)
            .await
            .expect("get action")
            .expect("action row");
        let stored_output = stored.output.expect("stored output");
        assert_eq!(stored_output["guard"]["allowed"].as_bool(), Some(true));
        assert_eq!(
            stored_output["guard"]["lease"]["capability"].as_str(),
            Some("net:http"),
        );
        assert_eq!(
            stored_output["posture"].as_str(),
            env.payload["posture"].as_str(),
        );
    }

    #[tokio::test]
    async fn complete_action_updates_kernel_and_emits_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
        let ctx = WorkerContext::new(&state);

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_ACTIONS_COMPLETED.to_string()], Some(8));

        let action_id = Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(&action_id, "demo.echo", &json!({}), None, None, "queued")
            .await
            .expect("insert action");

        let expected_capability = "net:http".to_string();
        let guard = ActionGuard {
            allowed: true,
            policy_allow: false,
            required_capabilities: vec![expected_capability.clone()],
            lease: Some(LeaseSummary {
                id: "lease-1".into(),
                subject: Some("subject".into()),
                capability: expected_capability.clone(),
                scope: Some("scope".into()),
                ttl_until: "2099-01-01T00:00:00Z".into(),
            }),
        };

        let outcome = ActionOutcome::new(json!({"result": "ok"}))
            .with_posture("steady".to_string())
            .with_guard(guard);

        ctx.complete_action(&action_id, outcome).await;

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("completed event recv")
            .expect("event value");

        assert_eq!(env.kind, topics::TOPIC_ACTIONS_COMPLETED);
        assert_eq!(env.payload["id"].as_str(), Some(action_id.as_str()));
        assert_eq!(env.payload["posture"].as_str(), Some("steady"));
        assert_eq!(
            env.payload["guard"]["lease"]["capability"].as_str(),
            Some(expected_capability.as_str()),
        );
        assert_eq!(env.payload["output"], json!({"result": "ok"}));

        let stored = state
            .kernel()
            .get_action_async(&action_id)
            .await
            .expect("get action")
            .expect("action row");
        assert_eq!(stored.state, "completed");
        let stored_output = stored.output.expect("stored output");
        assert_eq!(stored_output["posture"].as_str(), Some("steady"));
        assert_eq!(
            stored_output["guard"]["lease"]["capability"].as_str(),
            Some(expected_capability.as_str()),
        );
        assert_eq!(stored_output["guard"]["allowed"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn fail_action_updates_kernel_and_emits_event() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _state_guard = crate::util::scoped_state_dir_for_tests(temp.path());
        let state = build_state(temp.path()).await;
        let ctx = WorkerContext::new(&state);

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![topics::TOPIC_ACTIONS_FAILED.to_string()], Some(8));

        let action_id = Uuid::new_v4().to_string();
        state
            .kernel()
            .insert_action_async(&action_id, "demo.echo", &json!({}), None, None, "queued")
            .await
            .expect("insert action");

        let required_caps = vec!["io:egress".to_string()];
        let failure = ActionFailure::new(ToolError::Invalid("bad input".into()))
            .with_guard(ActionGuard {
                allowed: false,
                policy_allow: false,
                required_capabilities: required_caps.clone(),
                lease: None,
            })
            .with_posture("alert");

        ctx.fail_action(&action_id, "demo.echo", failure).await;

        let env = timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("failed event recv")
            .expect("event value");

        assert_eq!(env.kind, topics::TOPIC_ACTIONS_FAILED);
        assert_eq!(env.payload["id"].as_str(), Some(action_id.as_str()));
        assert_eq!(env.payload["posture"].as_str(), Some("alert"));
        let event_caps: Vec<String> = env.payload["guard"]["required_capabilities"]
            .as_array()
            .expect("required caps")
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(event_caps, required_caps);
        assert_eq!(env.payload["error"]["type"].as_str(), Some("invalid"),);

        let stored = state
            .kernel()
            .get_action_async(&action_id)
            .await
            .expect("get action")
            .expect("action row");
        assert_eq!(stored.state, "failed");
        let stored_output = stored.output.expect("stored output");
        assert_eq!(stored_output["posture"].as_str(), Some("alert"));
        let stored_caps: Vec<String> = stored_output["guard"]["required_capabilities"]
            .as_array()
            .expect("stored guard caps")
            .iter()
            .map(|v| v.as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(stored_caps, required_caps);
        assert_eq!(stored_output["guard"]["allowed"].as_bool(), Some(false));
        assert_eq!(stored.error.as_deref(), Some("invalid request: bad input"));
    }
}
