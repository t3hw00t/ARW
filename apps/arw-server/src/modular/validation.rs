use std::collections::{HashMap, HashSet};

use chrono::{SecondsFormat, Utc};
use jsonschema::Validator;
use serde_json::Value;

use crate::AppState;

use super::error::ModularValidationError;
use super::schema::{MODULAR_AGENT_MESSAGE_SCHEMA, MODULAR_TOOL_INVOCATION_SCHEMA};
use super::types::{
    derive_capability_requirements, AgentLifecycle, AgentPayload, ModularAgentMessage,
    ModularToolInvocation, ValidatedAgentMessage, ValidatedLease, ValidatedToolInvocation,
};

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

    let payload = AgentPayload::from_message(&message)?;
    let lifecycle = AgentLifecycle::new(
        &payload,
        message.policy_scope.requires_human_review(),
        &validated_leases,
    );

    Ok(ValidatedAgentMessage {
        message,
        payload,
        lifecycle,
        leases: validated_leases,
    })
}

pub async fn validate_tool_invocation(
    state: &AppState,
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
    if invocation.policy_scope.leases.is_empty() {
        return Err(ModularValidationError::Invalid(
            "policy_scope.leases must include at least one lease".into(),
        ));
    }
    ensure_unique(&invocation.sandbox_requirements.filesystem_scopes)?;
    ensure_unique(&invocation.policy_scope.leases)?;

    let leases_index = fetch_active_leases(state).await?;
    let mut validated_leases = Vec::new();
    for lease_id in &invocation.policy_scope.leases {
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

    let declared_caps = invocation
        .policy_scope
        .capabilities
        .clone()
        .unwrap_or_default();
    let capability_requirements = derive_capability_requirements(&invocation.sandbox_requirements);
    for requirement in &capability_requirements {
        if requirement.is_empty() {
            continue;
        }
        if !requirement.satisfied_by_caps(&declared_caps) {
            return Err(ModularValidationError::MissingCapability {
                capability: requirement.representative(),
            });
        }
        if !requirement.satisfied_by_leases(&validated_leases) {
            return Err(ModularValidationError::MissingCapability {
                capability: requirement.representative(),
            });
        }
    }

    Ok(ValidatedToolInvocation {
        invocation,
        leases: validated_leases,
        required_capabilities: capability_requirements,
    })
}

fn validate_against_schema(
    schema: &Validator,
    value: &Value,
) -> Result<(), ModularValidationError> {
    let issues = schema
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect::<Vec<_>>();
    if !issues.is_empty() {
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
