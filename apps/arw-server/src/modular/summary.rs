use serde_json::{json, Value};

use super::types::{
    flatten_capability_requirements, validation_status_label, AgentPayload, ValidatedAgentMessage,
    ValidatedToolInvocation,
};

pub fn agent_message_summary(validated: &ValidatedAgentMessage) -> Value {
    let payload_kind = validated.payload.kind();
    let mut summary = json!({
        "status": "accepted",
        "agent_id": validated.message.agent_id,
        "turn_id": validated.message.turn_id,
        "intent": validated.message.intent,
        "confidence": validated.message.confidence,
        "latency_budget_ms": validated.message.latency_budget_ms,
        "payload_kind": payload_kind.as_str(),
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
        "lifecycle": validated.lifecycle.to_value(),
        "handoff_state": validated.message.handoff_state,
        "created_ms": validated.message.created_ms,
    });

    if let Value::Object(ref mut obj) = summary {
        if let Some(persona) = validated.message.persona_id.as_ref() {
            obj.insert("persona_id".into(), json!(persona));
        }
        match &validated.payload {
            AgentPayload::Chat(chat) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "text_preview": preview_text(&chat.text, 160),
                        "citations": chat.citations.len(),
                        "followups": chat.followups.len(),
                    }),
                );
            }
            AgentPayload::Recall(recall) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "items": recall.items.len(),
                        "exhausted": recall.exhausted.unwrap_or(false),
                    }),
                );
            }
            AgentPayload::Compression(compression) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "candidates": compression.candidates.len(),
                        "retained": compression.retained.len(),
                        "dropped": compression.dropped.len(),
                    }),
                );
            }
            AgentPayload::Interpretation(interpretation) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "plan_steps": interpretation.plan_steps.len(),
                        "risks": interpretation.risks.len(),
                        "open_questions": interpretation.open_questions.len(),
                    }),
                );
            }
            AgentPayload::Validation(validation) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "status": validation_status_label(&validation.status),
                        "findings": validation.findings.len(),
                    }),
                );
            }
            AgentPayload::ToolBroker(broker) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "scheduled": broker.scheduled.len(),
                        "completed": broker.completed.len(),
                        "failed": broker.failed.len(),
                    }),
                );
            }
            AgentPayload::OrchestratorSummary(orchestrator) => {
                obj.insert(
                    "payload_summary".into(),
                    json!({
                        "logic_unit_id": orchestrator.logic_unit_id,
                        "has_hints": orchestrator.hints.is_some(),
                        "has_training_meta": orchestrator.training_meta.is_some(),
                    }),
                );
            }
            AgentPayload::Generic(value) => {
                if obj.get("payload_summary").is_none() {
                    let keys = value
                        .as_object()
                        .map(|map| map.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    obj.insert(
                        "payload_summary".into(),
                        json!({
                            "type": "generic",
                            "keys": keys,
                        }),
                    );
                }
            }
        }
    }

    summary
}

pub fn tool_invocation_summary(validated: &ValidatedToolInvocation) -> Value {
    let invocation = &validated.invocation;
    let sandbox_value =
        serde_json::to_value(&invocation.sandbox_requirements).unwrap_or_else(|_| json!({}));
    let required_capabilities = flatten_capability_requirements(&validated.required_capabilities);
    let declared_capabilities = invocation
        .policy_scope
        .capabilities
        .clone()
        .unwrap_or_default();
    let policy_scope_value = json!({
        "leases": validated
            .leases
            .iter()
            .map(|lease| lease.to_value())
            .collect::<Vec<_>>(),
        "capabilities": declared_capabilities,
        "requires_human_review": invocation.policy_scope.requires_human_review(),
    });
    let result_status = invocation
        .result
        .as_ref()
        .map(|r| r.status.as_str().to_string());
    let result_latency = invocation.result.as_ref().and_then(|r| r.latency_ms);
    let result_error = invocation
        .result
        .as_ref()
        .and_then(|r| r.error.as_ref())
        .map(|err| {
            json!({
                "kind": err.kind,
                "message": err.message,
                "retryable": err.retryable,
            })
        });
    let result_output_keys = invocation
        .result
        .as_ref()
        .and_then(|r| r.output.as_ref())
        .and_then(|out| out.as_object())
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let created_ms = invocation.started_ms.or(invocation.completed_ms);

    json!({
        "status": "accepted",
        "payload_kind": "tool_invocation",
        "invocation_id": invocation.invocation_id,
        "requested_by": invocation.requested_by,
        "tool_id": invocation.tool_id,
        "operation_id": invocation.operation_id,
        "persona_id": invocation.persona_id,
        "input_payload": invocation.input_payload,
        "sandbox_requirements": sandbox_value,
        "policy_scope": policy_scope_value,
        "required_capabilities": required_capabilities,
        "has_result": invocation.result.is_some(),
        "result_status": result_status,
        "result_latency_ms": result_latency,
        "result_error": result_error,
        "result_output_keys": result_output_keys,
        "lifecycle": {
            "stage": "accepted",
            "validation_gate": "skipped"
        },
        "payload_summary": {
            "result_status": result_status.clone(),
            "needs_network": invocation
                .sandbox_requirements
                .needs_network
                .unwrap_or(false),
            "filesystem_scopes": invocation.sandbox_requirements.filesystem_scopes.len(),
            "required_capabilities": required_capabilities.clone(),
        },
        "evidence_id": invocation.evidence_id,
        "started_ms": invocation.started_ms,
        "completed_ms": invocation.completed_ms,
        "created_ms": created_ms,
    })
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut buf = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            buf.push('.');
            return buf;
        }
        buf.push(ch);
    }
    buf
}
