use std::collections::{HashMap, HashSet};

use chrono::{DateTime, SecondsFormat, Utc};
use jsonschema::JSONSchema;
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;

use crate::AppState;

static MODULAR_AGENT_MESSAGE_SCHEMA: Lazy<JSONSchema> = Lazy::new(|| {
    let raw = include_str!("../../../spec/schemas/modular_agent_message.json");
    let schema: Value =
        serde_json::from_str(raw).expect("spec/schemas/modular_agent_message.json must parse");
    JSONSchema::compile(&schema).expect("modular_agent_message schema must be valid")
});

static MODULAR_TOOL_INVOCATION_SCHEMA: Lazy<JSONSchema> = Lazy::new(|| {
    let raw = include_str!("../../../spec/schemas/modular_tool_invocation.json");
    let schema: Value =
        serde_json::from_str(raw).expect("spec/schemas/modular_tool_invocation.json must parse");
    JSONSchema::compile(&schema).expect("modular_tool_invocation schema must be valid")
});

#[derive(Debug, Error)]
pub enum ModularValidationError {
    #[error("schema validation failed: {0:?}")]
    Schema(Vec<String>),
    #[error("invalid payload: {0}")]
    Invalid(String),
    #[error("lease {id} is not active")]
    MissingLease { id: String },
    #[error("lease {id} expired at {expired}")]
    ExpiredLease { id: String, expired: String },
    #[error("capability {capability} requires an active lease")]
    MissingCapability { capability: String },
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Deserialize)]
pub struct PolicyScope {
    pub leases: Vec<String>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    #[serde(default)]
    pub requires_human_review: Option<bool>,
}

impl PolicyScope {
    fn requires_human_review(&self) -> bool {
        self.requires_human_review.unwrap_or(false)
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ModularAgentMessage {
    pub agent_id: String,
    pub turn_id: String,
    pub intent: String,
    pub payload: Value,
    #[serde(default)]
    pub context_refs: Vec<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    pub confidence: f64,
    pub latency_budget_ms: u64,
    pub policy_scope: PolicyScope,
    #[serde(default)]
    pub handoff_state: Option<Value>,
    #[serde(default)]
    pub created_ms: Option<i64>,
}

#[derive(Debug)]
pub struct ValidatedAgentMessage {
    pub message: ModularAgentMessage,
    pub leases: Vec<ValidatedLease>,
}

#[derive(Debug)]
pub struct ValidatedLease {
    pub id: String,
    pub capability: String,
    pub scope: Option<String>,
    pub ttl_until: DateTime<Utc>,
}

impl ValidatedLease {
    fn from_row(value: &Value) -> Result<Self, ModularValidationError> {
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ModularValidationError::Internal("lease missing id".into()))?
            .to_string();
        let capability = value
            .get("capability")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ModularValidationError::Internal("lease missing capability".into()))?
            .to_string();
        let ttl_str = value
            .get("ttl_until")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ModularValidationError::Internal("lease missing ttl_until".into()))?;
        let ttl_until = DateTime::parse_from_rfc3339(ttl_str)
            .map_err(|err| {
                ModularValidationError::Internal(format!("invalid ttl_until for lease {id}: {err}"))
            })?
            .with_timezone(&Utc);
        let scope = value
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(Self {
            id,
            capability,
            scope,
            ttl_until,
        })
    }

    pub fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "capability": self.capability,
            "scope": self.scope,
            "ttl_until": self.ttl_until.to_rfc3339_opts(SecondsFormat::Millis, true),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct SandboxRequirements {
    #[serde(default)]
    pub needs_network: Option<bool>,
    #[serde(default)]
    pub filesystem_scopes: Vec<String>,
    #[serde(default)]
    pub environment: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvocationStatus {
    Pending,
    Ok,
    Error,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct InvocationError {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub retryable: Option<bool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct InvocationResult {
    pub status: InvocationStatus,
    #[serde(default)]
    pub output: Option<Value>,
    #[serde(default)]
    pub error: Option<InvocationError>,
    #[serde(default)]
    pub latency_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ModularToolInvocation {
    pub invocation_id: String,
    pub requested_by: String,
    pub tool_id: String,
    pub operation_id: String,
    pub input_payload: Value,
    pub sandbox_requirements: SandboxRequirements,
    pub evidence_id: String,
    #[serde(default)]
    pub result: Option<InvocationResult>,
    #[serde(default)]
    pub started_ms: Option<i64>,
    #[serde(default)]
    pub completed_ms: Option<i64>,
}

pub struct ValidatedToolInvocation {
    pub invocation: ModularToolInvocation,
}

pub async fn validate_agent_message(
    state: &AppState,
    value: &Value,
) -> Result<ValidatedAgentMessage, ModularValidationError> {
    validate_against_schema(&MODULAR_AGENT_MESSAGE_SCHEMA, value)?;
    let message: ModularAgentMessage = serde_json::from_value(value.clone()).map_err(|err| {
        ModularValidationError::Internal(format!("failed to parse agent message: {err}"))
    })?;

    if !message.payload.is_object() {
        return Err(ModularValidationError::Invalid(
            "payload must be an object".into(),
        ));
    }
    if !(0.0..=1.0).contains(&message.confidence) {
        return Err(ModularValidationError::Invalid(format!(
            "confidence must be between 0 and 1 inclusive; got {}",
            message.confidence
        )));
    }
    if message.policy_scope.leases.is_empty() {
        return Err(ModularValidationError::Invalid(
            "policy_scope.leases must include at least one lease".into(),
        ));
    }
    ensure_unique(&message.policy_scope.leases)?;

    let leases_index = fetch_active_leases(state).await?;
    let mut validated_leases = Vec::new();
    for lease_id in &message.policy_scope.leases {
        let row =
            leases_index
                .get(lease_id)
                .ok_or_else(|| ModularValidationError::MissingLease {
                    id: lease_id.clone(),
                })?;
        let lease = ValidatedLease::from_row(row)?;
        if lease.ttl_until <= Utc::now() {
            return Err(ModularValidationError::ExpiredLease {
                id: lease.id.clone(),
                expired: lease.ttl_until.to_rfc3339_opts(SecondsFormat::Millis, true),
            });
        }
        validated_leases.push(lease);
    }

    if let Some(capabilities) = &message.policy_scope.capabilities {
        for capability in capabilities {
            if !validated_leases
                .iter()
                .any(|lease| &lease.capability == capability)
            {
                return Err(ModularValidationError::MissingCapability {
                    capability: capability.clone(),
                });
            }
        }
    }

    Ok(ValidatedAgentMessage {
        message,
        leases: validated_leases,
    })
}

pub async fn validate_tool_invocation(
    value: &Value,
) -> Result<ValidatedToolInvocation, ModularValidationError> {
    validate_against_schema(&MODULAR_TOOL_INVOCATION_SCHEMA, value)?;
    let invocation: ModularToolInvocation =
        serde_json::from_value(value.clone()).map_err(|err| {
            ModularValidationError::Internal(format!("failed to parse tool invocation: {err}"))
        })?;
    if invocation.invocation_id.trim().is_empty() {
        return Err(ModularValidationError::Invalid(
            "invocation_id must not be empty".into(),
        ));
    }
    if invocation.tool_id.trim().is_empty() {
        return Err(ModularValidationError::Invalid(
            "tool_id must not be empty".into(),
        ));
    }
    if invocation.operation_id.trim().is_empty() {
        return Err(ModularValidationError::Invalid(
            "operation_id must not be empty".into(),
        ));
    }
    if !invocation.input_payload.is_object() {
        return Err(ModularValidationError::Invalid(
            "input_payload must be an object".into(),
        ));
    }
    ensure_unique(&invocation.sandbox_requirements.filesystem_scopes)?;
    Ok(ValidatedToolInvocation { invocation })
}

pub fn agent_message_summary(validated: &ValidatedAgentMessage) -> Value {
    json!({
        "status": "accepted",
        "agent_id": validated.message.agent_id,
        "turn_id": validated.message.turn_id,
        "intent": validated.message.intent,
        "confidence": validated.message.confidence,
        "latency_budget_ms": validated.message.latency_budget_ms,
        "policy_scope": {
            "leases": validated
                .leases
                .iter()
                .map(|lease| lease.to_value())
                .collect::<Vec<_>>(),
            "capabilities": validated
                .message
                .policy_scope
                .capabilities
                .clone()
                .unwrap_or_default(),
            "requires_human_review": validated.message.policy_scope.requires_human_review(),
        },
        "handoff_state": validated.message.handoff_state,
        "created_ms": validated.message.created_ms,
    })
}

pub fn tool_invocation_summary(validated: &ValidatedToolInvocation) -> Value {
    let invocation = &validated.invocation;
    json!({
        "status": "accepted",
        "invocation_id": invocation.invocation_id,
        "requested_by": invocation.requested_by,
        "tool_id": invocation.tool_id,
        "operation_id": invocation.operation_id,
        "has_result": invocation.result.is_some(),
        "sandbox": {
            "needs_network": invocation.sandbox_requirements.needs_network.unwrap_or(false),
            "filesystem_scopes": invocation.sandbox_requirements.filesystem_scopes,
            "environment_keys": invocation
                .sandbox_requirements
                .environment
                .as_ref()
                .map(|env| env.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default(),
        },
        "evidence_id": invocation.evidence_id,
        "started_ms": invocation.started_ms,
        "completed_ms": invocation.completed_ms,
    })
}

fn validate_against_schema(
    schema: &JSONSchema,
    value: &Value,
) -> Result<(), ModularValidationError> {
    if let Err(errors) = schema.validate(value) {
        let issues = errors.map(|e| e.to_string()).collect::<Vec<_>>();
        return Err(ModularValidationError::Schema(issues));
    }
    Ok(())
}

fn ensure_unique(values: &[String]) -> Result<(), ModularValidationError> {
    let mut seen = HashSet::new();
    for v in values {
        if !seen.insert(v) {
            return Err(ModularValidationError::Invalid(format!(
                "duplicate entry detected: {v}"
            )));
        }
    }
    Ok(())
}

async fn fetch_active_leases(
    state: &AppState,
) -> Result<HashMap<String, Value>, ModularValidationError> {
    let rows = state
        .kernel()
        .list_leases_async(512)
        .await
        .map_err(|err| ModularValidationError::Internal(err.to_string()))?;
    let mut map = HashMap::new();
    for row in rows {
        if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
            map.insert(id.to_string(), row);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use chrono::Duration;
    use serde_json::json;
    use tempfile::tempdir;
    use uuid::Uuid;

    async fn test_state() -> (AppState, tempfile::TempDir) {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        test_support::init_tracing();
        let state = test_support::build_state(temp.path(), &mut ctx.env).await;
        drop(ctx);
        (state, temp)
    }

    async fn seed_lease(state: &AppState, capability: &str) -> String {
        let lease_id = Uuid::new_v4().to_string();
        let ttl_until =
            (Utc::now() + Duration::minutes(5)).to_rfc3339_opts(SecondsFormat::Millis, true);
        state
            .kernel()
            .insert_lease_async(
                lease_id.clone(),
                "modular".into(),
                capability.to_string(),
                Some("stack".into()),
                ttl_until,
                None,
                None,
            )
            .await
            .expect("insert lease");
        lease_id
    }

    #[tokio::test]
    async fn validate_agent_message_accepts_active_lease() {
        let (state, _tmp) = test_state().await;
        let lease_id = seed_lease(&state, "context:read").await;
        let body = json!({
            "agent_id": "assistant.chat",
            "turn_id": "turn-123",
            "intent": "draft_response",
            "payload": { "text": "hi" },
            "context_refs": ["memory/abc"],
            "evidence_ids": ["tool-xyz"],
            "confidence": 0.82,
            "latency_budget_ms": 1500,
            "policy_scope": {
                "leases": [lease_id.clone()],
                "capabilities": ["context:read"],
                "requires_human_review": false
            },
            "handoff_state": { "status": "complete" },
            "created_ms": 42
        });

        let validated = validate_agent_message(&state, &body)
            .await
            .expect("validation succeeds");
        assert_eq!(validated.message.agent_id, "assistant.chat");
        assert_eq!(validated.leases.len(), 1);
        assert_eq!(validated.leases[0].capability, "context:read");
    }

    #[tokio::test]
    async fn validate_agent_message_rejects_missing_lease() {
        let (state, _tmp) = test_state().await;
        let body = json!({
            "agent_id": "assistant.chat",
            "turn_id": "turn-123",
            "intent": "draft_response",
            "payload": { "text": "hi" },
            "context_refs": [],
            "evidence_ids": [],
            "confidence": 0.5,
            "latency_budget_ms": 1000,
            "policy_scope": {
                "leases": ["does-not-exist"]
            }
        });

        let err = validate_agent_message(&state, &body)
            .await
            .expect_err("validation fails");
        match err {
            ModularValidationError::MissingLease { id } => {
                assert_eq!(id, "does-not-exist");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn validate_tool_invocation_accepts_basic_payload() {
        let body = json!({
            "invocation_id": "invoke-123",
            "requested_by": "agent.recall",
            "tool_id": "memory.search",
            "operation_id": "memory.search@1.0.0",
            "input_payload": {
                "query": "hello",
                "limit": 5
            },
            "sandbox_requirements": {
                "needs_network": false,
                "filesystem_scopes": []
            },
            "evidence_id": "evidence-456"
        });

        let validated = validate_tool_invocation(&body)
            .await
            .expect("tool invocation valid");
        assert_eq!(validated.invocation.operation_id, "memory.search@1.0.0");
    }
}
