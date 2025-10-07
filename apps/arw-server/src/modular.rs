use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result as AnyhowResult};
use chrono::{DateTime, SecondsFormat, Utc};
use jsonschema::JSONSchema;
use metrics::histogram;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;
use tracing::warn;

use crate::{memory_service, read_models, AppState};
use memory_service::MemoryUpsertInput;

const SHORT_TERM_TTL_SECS_DEFAULT: i64 = 900;
const SHORT_TERM_TTL_ENV: &str = "ARW_MEMORY_SHORT_TTL_SECS";
const MODULAR_SHORT_TERM_SOURCE: &str = "modular.agent.short_term";
const MODULAR_EPISODIC_SOURCE: &str = "modular.agent.episodic";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentPayloadKind {
    Chat,
    Recall,
    Compression,
    Interpretation,
    Validation,
    ToolBroker,
    OrchestratorSummary,
    Generic,
}

impl AgentPayloadKind {
    fn as_str(&self) -> &'static str {
        match self {
            AgentPayloadKind::Chat => "chat",
            AgentPayloadKind::Recall => "recall",
            AgentPayloadKind::Compression => "compression",
            AgentPayloadKind::Interpretation => "interpretation",
            AgentPayloadKind::Validation => "validation",
            AgentPayloadKind::ToolBroker => "tool_broker",
            AgentPayloadKind::OrchestratorSummary => "orchestrator_summary",
            AgentPayloadKind::Generic => "generic",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatAgentPayload {
    pub text: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub followups: Vec<FollowupSuggestion>,
    #[serde(default)]
    pub citations: Vec<AgentCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowupSuggestion {
    pub prompt: String,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCitation {
    pub evidence_id: String,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallAgentPayload {
    pub items: Vec<RecallItem>,
    #[serde(default)]
    pub exhausted: Option<bool>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallItem {
    pub id: String,
    pub lane: String,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionAgentPayload {
    pub candidates: Vec<CompressionCandidate>,
    #[serde(default)]
    pub retained: Vec<String>,
    #[serde(default)]
    pub dropped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionCandidate {
    pub id: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub loss_score: Option<f64>,
    #[serde(default)]
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterpretationAgentPayload {
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub plan_steps: Vec<String>,
    #[serde(default)]
    pub risks: Vec<InterpretationRisk>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterpretationRisk {
    pub kind: String,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationAgentPayload {
    pub status: String,
    #[serde(default)]
    pub findings: Vec<ValidationFinding>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationFinding {
    pub kind: String,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolBrokerPayload {
    #[serde(default)]
    pub scheduled: Vec<String>,
    #[serde(default)]
    pub completed: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorSummaryPayload {
    pub goal: String,
    #[serde(default)]
    pub logic_unit_id: Option<String>,
    #[serde(default)]
    pub hints: Option<Value>,
    #[serde(default)]
    pub training_meta: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum AgentPayload {
    Chat(ChatAgentPayload),
    Recall(RecallAgentPayload),
    Compression(CompressionAgentPayload),
    Interpretation(InterpretationAgentPayload),
    Validation(ValidationAgentPayload),
    ToolBroker(ToolBrokerPayload),
    OrchestratorSummary(OrchestratorSummaryPayload),
    Generic(Value),
}

impl AgentPayload {
    fn kind(&self) -> AgentPayloadKind {
        match self {
            AgentPayload::Chat(_) => AgentPayloadKind::Chat,
            AgentPayload::Recall(_) => AgentPayloadKind::Recall,
            AgentPayload::Compression(_) => AgentPayloadKind::Compression,
            AgentPayload::Interpretation(_) => AgentPayloadKind::Interpretation,
            AgentPayload::Validation(_) => AgentPayloadKind::Validation,
            AgentPayload::ToolBroker(_) => AgentPayloadKind::ToolBroker,
            AgentPayload::OrchestratorSummary(_) => AgentPayloadKind::OrchestratorSummary,
            AgentPayload::Generic(_) => AgentPayloadKind::Generic,
        }
    }

    fn from_message(message: &ModularAgentMessage) -> Result<Self, ModularValidationError> {
        let agent_id = message.agent_id.as_str();
        match agent_id {
            "assistant.chat" => {
                let payload = serde_json::from_value::<ChatAgentPayload>(message.payload.clone())
                    .map_err(|err| {
                    ModularValidationError::Invalid(format!("chat payload invalid: {err}"))
                })?;
                if payload.text.trim().is_empty() {
                    return Err(ModularValidationError::Invalid(
                        "chat payload text must not be empty".into(),
                    ));
                }
                Ok(AgentPayload::Chat(payload))
            }
            "memory.recall" => {
                let payload = serde_json::from_value::<RecallAgentPayload>(message.payload.clone())
                    .map_err(|err| {
                        ModularValidationError::Invalid(format!("recall payload invalid: {err}"))
                    })?;
                if payload.items.is_empty() {
                    return Err(ModularValidationError::Invalid(
                        "recall payload must include at least one item".into(),
                    ));
                }
                Ok(AgentPayload::Recall(payload))
            }
            "memory.compression" => {
                let payload =
                    serde_json::from_value::<CompressionAgentPayload>(message.payload.clone())
                        .map_err(|err| {
                            ModularValidationError::Invalid(format!(
                                "compression payload invalid: {err}"
                            ))
                        })?;
                Ok(AgentPayload::Compression(payload))
            }
            "analysis.interpretation" | "interpretation.brief" => {
                let payload =
                    serde_json::from_value::<InterpretationAgentPayload>(message.payload.clone())
                        .map_err(|err| {
                        ModularValidationError::Invalid(format!(
                            "interpretation payload invalid: {err}"
                        ))
                    })?;
                Ok(AgentPayload::Interpretation(payload))
            }
            "validation.guard" => {
                let payload =
                    serde_json::from_value::<ValidationAgentPayload>(message.payload.clone())
                        .map_err(|err| {
                            ModularValidationError::Invalid(format!(
                                "validation payload invalid: {err}"
                            ))
                        })?;
                Ok(AgentPayload::Validation(payload))
            }
            "tool.broker" => {
                let payload = serde_json::from_value::<ToolBrokerPayload>(message.payload.clone())
                    .map_err(|err| {
                        ModularValidationError::Invalid(format!(
                            "tool broker payload invalid: {err}"
                        ))
                    })?;
                Ok(AgentPayload::ToolBroker(payload))
            }
            "orchestrator.trainer" => {
                let payload =
                    serde_json::from_value::<OrchestratorSummaryPayload>(message.payload.clone())
                        .map_err(|err| {
                        ModularValidationError::Invalid(format!(
                            "orchestrator payload invalid: {err}"
                        ))
                    })?;
                if payload.goal.trim().is_empty() {
                    return Err(ModularValidationError::Invalid(
                        "orchestrator payload goal must not be empty".into(),
                    ));
                }
                Ok(AgentPayload::OrchestratorSummary(payload))
            }
            _ => Ok(AgentPayload::Generic(message.payload.clone())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentLifecycle {
    stage: LifecycleStage,
    validation_gate: ValidationGate,
    lease_scopes: Vec<String>,
    requires_human_review: bool,
}

impl AgentLifecycle {
    fn new(payload: &AgentPayload, requires_human_review: bool, leases: &[ValidatedLease]) -> Self {
        let lease_scopes = leases
            .iter()
            .filter_map(|l| l.scope.clone())
            .collect::<Vec<_>>();
        let validation_gate = ValidationGate::from_payload(payload, requires_human_review);
        let stage = LifecycleStage::from_gate(validation_gate, requires_human_review);
        Self {
            stage,
            validation_gate,
            lease_scopes,
            requires_human_review,
        }
    }

    fn to_value(&self) -> Value {
        json!({
            "stage": self.stage.as_str(),
            "validation_gate": self.validation_gate.as_str(),
            "lease_scopes": self.lease_scopes,
            "requires_human_review": self.requires_human_review,
        })
    }

    pub(crate) fn stage_str(&self) -> &'static str {
        self.stage.as_str()
    }

    pub(crate) fn validation_gate_str(&self) -> &'static str {
        self.validation_gate.as_str()
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleStage {
    Accepted,
    PendingHumanReview,
    Blocked,
}

impl LifecycleStage {
    fn from_gate(gate: ValidationGate, requires_review: bool) -> Self {
        match gate {
            ValidationGate::Rejected => LifecycleStage::Blocked,
            ValidationGate::Pending => LifecycleStage::PendingHumanReview,
            ValidationGate::Required => LifecycleStage::PendingHumanReview,
            ValidationGate::Approved | ValidationGate::Skipped => {
                if requires_review {
                    LifecycleStage::PendingHumanReview
                } else {
                    LifecycleStage::Accepted
                }
            }
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            LifecycleStage::Accepted => "accepted",
            LifecycleStage::PendingHumanReview => "pending_human_review",
            LifecycleStage::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ValidationGate {
    Approved,
    Pending,
    Rejected,
    Required,
    Skipped,
}

impl ValidationGate {
    fn as_str(self) -> &'static str {
        match self {
            ValidationGate::Approved => "approved",
            ValidationGate::Pending => "pending",
            ValidationGate::Rejected => "rejected",
            ValidationGate::Required => "required",
            ValidationGate::Skipped => "skipped",
        }
    }

    fn from_payload(payload: &AgentPayload, requires_review: bool) -> Self {
        match payload {
            AgentPayload::Validation(details) => {
                match normalize_validation_status(&details.status) {
                    NormalizedValidationStatus::Pass => {
                        if requires_review {
                            ValidationGate::Pending
                        } else {
                            ValidationGate::Approved
                        }
                    }
                    NormalizedValidationStatus::NeedsReview => ValidationGate::Required,
                    NormalizedValidationStatus::Blocked => ValidationGate::Rejected,
                    NormalizedValidationStatus::Pending => ValidationGate::Pending,
                }
            }
            _ => {
                if requires_review {
                    ValidationGate::Required
                } else {
                    ValidationGate::Skipped
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NormalizedValidationStatus {
    Pass,
    NeedsReview,
    Blocked,
    Pending,
}

fn normalize_validation_status(status: &str) -> NormalizedValidationStatus {
    match status.to_ascii_lowercase().as_str() {
        "pass" | "ok" | "approved" => NormalizedValidationStatus::Pass,
        "needs_review" | "review" | "manual" | "pending_review" => {
            NormalizedValidationStatus::NeedsReview
        }
        "blocked" | "fail" | "rejected" | "deny" => NormalizedValidationStatus::Blocked,
        "pending" | "waiting" => NormalizedValidationStatus::Pending,
        _ => NormalizedValidationStatus::Pending,
    }
}

impl NormalizedValidationStatus {
    fn as_str(self) -> &'static str {
        match self {
            NormalizedValidationStatus::Pass => "pass",
            NormalizedValidationStatus::NeedsReview => "needs_review",
            NormalizedValidationStatus::Blocked => "blocked",
            NormalizedValidationStatus::Pending => "pending",
        }
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
    pub payload: AgentPayload,
    pub lifecycle: AgentLifecycle,
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
                        "status": normalize_validation_status(&validation.status).as_str(),
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

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut buf = String::new();
    for (idx, ch) in text.chars().enumerate() {
        if idx >= max_chars {
            buf.push('…');
            return buf;
        }
        buf.push(ch);
    }
    buf
}

pub async fn persist_agent_memory(
    state: &AppState,
    validated: &ValidatedAgentMessage,
    summary: &Value,
) {
    if let Err(err) = persist_agent_memory_inner(state, validated, summary).await {
        warn!(
            target: "arw::modular",
            error = %err,
            agent_id = %validated.message.agent_id,
            turn_id = %validated.message.turn_id,
            "failed to persist modular turn memory"
        );
    }
}

async fn persist_agent_memory_inner(
    state: &AppState,
    validated: &ValidatedAgentMessage,
    summary: &Value,
) -> AnyhowResult<()> {
    let payload_kind = validated.payload.kind();
    let metrics = compute_loss_metrics(&validated.payload, summary);
    let summary_excerpt = summary
        .get("payload_summary")
        .and_then(|v| v.get("text_preview"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut record_map = Map::new();
    record_map.insert("turn_id".into(), json!(validated.message.turn_id));
    record_map.insert("agent_id".into(), json!(validated.message.agent_id));
    record_map.insert("intent".into(), json!(validated.message.intent));
    record_map.insert("payload_kind".into(), json!(payload_kind.as_str()));
    record_map.insert("confidence".into(), json!(validated.message.confidence));
    record_map.insert(
        "latency_budget_ms".into(),
        json!(validated.message.latency_budget_ms),
    );
    if let Some(created) = validated.message.created_ms {
        record_map.insert("created_ms".into(), json!(created));
    }
    record_map.insert("context_refs".into(), json!(validated.message.context_refs));
    record_map.insert("evidence_ids".into(), json!(validated.message.evidence_ids));
    if let Some(handoff) = &validated.message.handoff_state {
        record_map.insert("handoff_state".into(), handoff.clone());
    }
    record_map.insert("payload".into(), validated.message.payload.clone());
    if let Some(policy) = summary.get("policy_scope") {
        record_map.insert("policy_scope".into(), policy.clone());
    }
    if let Some(lifecycle) = summary.get("lifecycle") {
        record_map.insert("lifecycle".into(), lifecycle.clone());
    }
    if let Some(payload_summary) = summary.get("payload_summary") {
        record_map.insert("payload_summary".into(), payload_summary.clone());
    }
    record_map.insert("metrics".into(), metrics.clone());
    let base_value = Value::Object(record_map);

    let mut extra_map = Map::new();
    extra_map.insert("payload_kind".into(), json!(payload_kind.as_str()));
    extra_map.insert("metrics".into(), metrics.clone());
    if let Some(excerpt) = &summary_excerpt {
        extra_map.insert("summary_excerpt".into(), json!(excerpt));
    }
    extra_map.insert(
        "validation_gate".into(),
        json!(validated.lifecycle.validation_gate_str()),
    );
    extra_map.insert(
        "lifecycle_stage".into(),
        json!(validated.lifecycle.stage_str()),
    );
    if let Some(policy) = summary.get("policy_scope") {
        if let Some(capabilities) = policy.get("capabilities") {
            extra_map.insert("capabilities".into(), capabilities.clone());
        }
    }

    let text = extract_primary_text(&validated.payload);
    let keywords = make_keywords(payload_kind.as_str(), &validated.message.intent);
    let tags = make_tags(&validated.message.agent_id, &validated.message.intent);

    let mut short_term_input = MemoryUpsertInput {
        lane: "short_term".to_string(),
        kind: Some("conversation.turn".to_string()),
        key: Some(validated.message.turn_id.clone()),
        value: base_value.clone(),
        text: text.clone(),
        agent_id: Some(validated.message.agent_id.clone()),
        trust: Some(validated.message.confidence),
        ttl_s: Some(short_term_ttl_secs()),
        tags: tags.clone(),
        keywords: keywords.clone(),
        durability: Some("volatile".to_string()),
        privacy: Some("private".to_string()),
        source: json!({
            "kind": "modular_turn",
            "lane": "short_term",
            "turn_id": validated.message.turn_id,
            "agent_id": validated.message.agent_id,
        }),
        extra: Value::Object(extra_map.clone()),
        dedupe: true,
        ..Default::default()
    };
    if let Some(probability) = validated
        .message
        .confidence
        .is_finite()
        .then_some(validated.message.confidence)
    {
        short_term_input.prob = Some(probability);
    }

    let short_result =
        memory_service::upsert_memory(state, short_term_input, MODULAR_SHORT_TERM_SOURCE)
            .await
            .context("persist short-term modular memory")?;

    let mut episodic_extra = extra_map.clone();
    episodic_extra.insert("short_term_id".into(), json!(short_result.id));

    let mut episodic_input = MemoryUpsertInput {
        lane: "episodic".to_string(),
        kind: Some("conversation.turn".to_string()),
        key: Some(validated.message.turn_id.clone()),
        value: base_value,
        text,
        agent_id: Some(validated.message.agent_id.clone()),
        trust: Some(validated.message.confidence),
        tags,
        keywords,
        durability: Some("short".to_string()),
        privacy: Some("private".to_string()),
        source: json!({
            "kind": "modular_turn",
            "lane": "episodic",
            "turn_id": validated.message.turn_id,
            "agent_id": validated.message.agent_id,
        }),
        extra: Value::Object(episodic_extra),
        dedupe: true,
        ..Default::default()
    };
    if let Some(policy) = summary.get("policy_scope") {
        episodic_input.links = json!({
            "leases": policy.get("leases"),
        });
    }
    if let Some(probability) = validated
        .message
        .confidence
        .is_finite()
        .then_some(validated.message.confidence)
    {
        episodic_input.prob = Some(probability);
    }

    memory_service::upsert_memory(state, episodic_input, MODULAR_EPISODIC_SOURCE)
        .await
        .context("persist episodic modular memory")?;

    match state.kernel().list_recent_memory_async(None, 200).await {
        Ok(items) => {
            let bundle = read_models::build_memory_recent_bundle(items);
            read_models::publish_memory_bundle(&state.bus(), &bundle);
        }
        Err(err) => {
            warn!(
                target: "arw::modular",
                error = %err,
                "failed to refresh memory_recent snapshot after modular turn"
            );
        }
    }

    Ok(())
}

fn short_term_ttl_secs() -> i64 {
    std::env::var(SHORT_TERM_TTL_ENV)
        .ok()
        .and_then(|raw| raw.parse::<i64>().ok())
        .filter(|val| *val > 0)
        .unwrap_or(SHORT_TERM_TTL_SECS_DEFAULT)
}

fn compute_loss_metrics(payload: &AgentPayload, _summary: &Value) -> Value {
    match payload {
        AgentPayload::Chat(chat) => {
            let text_len = chat.text.chars().count() as u64;
            let summary_len = chat
                .summary
                .as_ref()
                .map(|s| s.chars().count() as u64)
                .unwrap_or(0);
            let ratio = if text_len > 0 {
                summary_len as f64 / text_len as f64
            } else {
                1.0
            };
            let loss = if text_len > 0 { 1.0 - ratio } else { 0.0 };
            histogram!("arw_modular_chat_text_len").record(text_len as f64);
            histogram!("arw_modular_chat_summary_ratio").record(ratio);
            json!({
                "text_chars": text_len,
                "summary_chars": summary_len,
                "summary_ratio": ratio,
                "loss": loss,
                "followups": chat.followups.len(),
                "citations": chat.citations.len(),
            })
        }
        AgentPayload::Recall(recall) => {
            let item_count = recall.items.len() as u64;
            let avg_score = if item_count > 0 {
                recall.items.iter().filter_map(|i| i.score).sum::<f64>() / item_count as f64
            } else {
                0.0
            };
            let unique_lanes: HashSet<_> =
                recall.items.iter().map(|item| item.lane.clone()).collect();
            histogram!("arw_modular_recall_items").record(item_count as f64);
            if avg_score.is_finite() {
                histogram!("arw_modular_recall_avg_score").record(avg_score);
            }
            json!({
                "items": item_count,
                "avg_score": avg_score,
                "unique_lanes": unique_lanes.len(),
                "exhausted": recall.exhausted.unwrap_or(false),
            })
        }
        AgentPayload::Compression(compression) => {
            let candidate_count = compression.candidates.len() as u64;
            let retained = compression.retained.len() as u64;
            let dropped = compression.dropped.len() as u64;
            let avg_loss = if candidate_count > 0 {
                compression
                    .candidates
                    .iter()
                    .filter_map(|c| c.loss_score)
                    .sum::<f64>()
                    / candidate_count as f64
            } else {
                0.0
            };
            histogram!("arw_modular_compression_candidates").record(candidate_count as f64);
            histogram!("arw_modular_compression_retained").record(retained as f64);
            if avg_loss.is_finite() {
                histogram!("arw_modular_compression_loss").record(avg_loss);
            }
            json!({
                "candidates": candidate_count,
                "retained": retained,
                "dropped": dropped,
                "avg_loss_score": avg_loss,
            })
        }
        AgentPayload::Interpretation(interpretation) => json!({
            "plan_steps": interpretation.plan_steps.len(),
            "risks": interpretation.risks.len(),
            "open_questions": interpretation.open_questions.len(),
            "has_brief": interpretation.brief.is_some(),
        }),
        AgentPayload::Validation(validation) => {
            let findings = validation.findings.len() as u64;
            histogram!("arw_modular_validation_findings").record(findings as f64);
            json!({
                "status": normalize_validation_status(&validation.status).as_str(),
                "findings": findings,
                "has_summary": validation.summary.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
            })
        }
        AgentPayload::ToolBroker(broker) => json!({
            "scheduled": broker.scheduled.len(),
            "completed": broker.completed.len(),
            "failed": broker.failed.len(),
        }),
        AgentPayload::OrchestratorSummary(summary_payload) => json!({
            "goal_length": summary_payload.goal.chars().count(),
            "has_logic_unit": summary_payload.logic_unit_id.is_some(),
            "has_hints": summary_payload.hints.is_some(),
            "has_training_meta": summary_payload.training_meta.is_some(),
        }),
        AgentPayload::Generic(value) => {
            let keys = value
                .as_object()
                .map(|map| map.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            json!({
                "type": "generic",
                "keys": keys,
            })
        }
    }
}

fn extract_primary_text(payload: &AgentPayload) -> Option<String> {
    match payload {
        AgentPayload::Chat(chat) => Some(chat.text.clone()),
        AgentPayload::Recall(recall) => {
            let mut parts = Vec::new();
            for item in &recall.items {
                if let Some(summary) = &item.summary {
                    if !summary.trim().is_empty() {
                        parts.push(summary.clone());
                        continue;
                    }
                }
                if let Some(snippet) = &item.snippet {
                    if !snippet.trim().is_empty() {
                        parts.push(snippet.clone());
                    }
                }
                if parts.len() >= 3 {
                    break;
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        AgentPayload::Compression(compression) => {
            let mut summaries = Vec::new();
            for cand in &compression.candidates {
                if let Some(sum) = &cand.summary {
                    if !sum.trim().is_empty() {
                        summaries.push(sum.clone());
                    }
                }
                if summaries.len() >= 3 {
                    break;
                }
            }
            if summaries.is_empty() {
                None
            } else {
                Some(summaries.join("\n"))
            }
        }
        AgentPayload::Interpretation(interpretation) => interpretation.brief.clone(),
        AgentPayload::Validation(validation) => validation.summary.clone(),
        AgentPayload::ToolBroker(_) => None,
        AgentPayload::OrchestratorSummary(summary_payload) => Some(summary_payload.goal.clone()),
        AgentPayload::Generic(value) => value
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| value.get("summary").and_then(|v| v.as_str()))
            .map(|s| s.to_string()),
    }
}

fn make_keywords(payload_kind: &str, intent: &str) -> Vec<String> {
    vec![
        payload_kind.to_string(),
        format!("intent:{}", sanitize_tag_fragment(intent)),
    ]
}

fn make_tags(agent_id: &str, intent: &str) -> Vec<String> {
    vec![
        "modular".to_string(),
        format!("agent:{}", sanitize_tag_fragment(agent_id)),
        format!("intent:{}", sanitize_tag_fragment(intent)),
    ]
}

fn sanitize_tag_fragment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_whitespace() || ch == '/' || ch == '\\' {
            out.push('_');
        }
    }
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
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
        match &validated.payload {
            AgentPayload::Chat(chat) => {
                assert_eq!(chat.text, "hi");
                assert!(chat.citations.is_empty());
            }
            _ => panic!("expected chat payload"),
        }
        assert_eq!(validated.lifecycle.stage_str(), "accepted");
        assert_eq!(validated.lifecycle.validation_gate_str(), "skipped");
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
    async fn validate_agent_message_rejects_empty_chat_text() {
        let (state, _tmp) = test_state().await;
        let lease_id = seed_lease(&state, "context:read").await;
        let body = json!({
            "agent_id": "assistant.chat",
            "turn_id": "turn-123",
            "intent": "draft_response",
            "payload": { "text": "   " },
            "context_refs": [],
            "evidence_ids": [],
            "confidence": 0.6,
            "latency_budget_ms": 800,
            "policy_scope": {
                "leases": [lease_id],
                "capabilities": ["context:read"]
            }
        });

        let err = validate_agent_message(&state, &body)
            .await
            .expect_err("validation fails");
        match err {
            ModularValidationError::Invalid(msg) => {
                assert!(msg.contains("chat payload"), "unexpected message: {msg}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn agent_message_summary_includes_lifecycle_and_payload_kind() {
        let (state, _tmp) = test_state().await;
        let lease_id = seed_lease(&state, "context:read").await;
        let body = json!({
            "agent_id": "assistant.chat",
            "turn_id": "turn-456",
            "intent": "draft_response",
            "payload": { "text": "hello world", "citations": [] },
            "context_refs": [],
            "evidence_ids": [],
            "confidence": 0.9,
            "latency_budget_ms": 600,
            "policy_scope": {
                "leases": [lease_id.clone()],
                "capabilities": ["context:read"]
            }
        });

        let validated = validate_agent_message(&state, &body)
            .await
            .expect("validation succeeds");
        let summary = agent_message_summary(&validated);
        assert_eq!(summary["payload_kind"], json!("chat"));
        assert_eq!(summary["lifecycle"]["stage"], json!("accepted"));
        assert_eq!(
            summary["lifecycle"]["lease_scopes"]
                .as_array()
                .expect("lease scopes array")
                .first()
                .and_then(|v| v.as_str()),
            Some("stack")
        );
        assert_eq!(
            summary["policy_scope"]["leases"]
                .as_array()
                .expect("leases array")
                .first()
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str()),
            Some(lease_id.as_str())
        );
        assert_eq!(
            summary["payload_summary"]["text_preview"]
                .as_str()
                .unwrap_or_default(),
            "hello world"
        );
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
