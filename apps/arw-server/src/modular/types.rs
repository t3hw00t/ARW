use std::collections::{HashMap, HashSet};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::error::ModularValidationError;

#[derive(Debug, Deserialize)]
pub struct PolicyScope {
    pub leases: Vec<String>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
    #[serde(default)]
    pub requires_human_review: Option<bool>,
}

impl PolicyScope {
    pub(crate) fn requires_human_review(&self) -> bool {
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
    pub(crate) fn as_str(&self) -> &'static str {
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
    pub(crate) fn kind(&self) -> AgentPayloadKind {
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

    pub(crate) fn from_message(
        message: &ModularAgentMessage,
    ) -> Result<Self, ModularValidationError> {
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
    pub(crate) fn new(
        payload: &AgentPayload,
        requires_human_review: bool,
        leases: &[ValidatedLease],
    ) -> Self {
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

    pub(crate) fn to_value(&self) -> Value {
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
    pub persona_id: Option<String>,
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
    pub(crate) fn from_row(value: &Value) -> Result<Self, ModularValidationError> {
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

#[derive(Debug, Deserialize, Serialize)]
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

impl InvocationStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            InvocationStatus::Pending => "pending",
            InvocationStatus::Ok => "ok",
            InvocationStatus::Error => "error",
        }
    }
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
    pub policy_scope: PolicyScope,
    #[serde(default)]
    pub persona_id: Option<String>,
    #[serde(default)]
    pub result: Option<InvocationResult>,
    #[serde(default)]
    pub started_ms: Option<i64>,
    #[serde(default)]
    pub completed_ms: Option<i64>,
}

#[derive(Debug)]
pub struct ValidatedToolInvocation {
    pub invocation: ModularToolInvocation,
    pub leases: Vec<ValidatedLease>,
    pub required_capabilities: Vec<CapabilityRequirement>,
}

#[derive(Debug, Clone)]
pub struct CapabilityRequirement {
    any_of: Vec<String>,
}

impl CapabilityRequirement {
    fn any_of<I, S>(caps: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut any_of = caps.into_iter().map(Into::into).collect::<Vec<_>>();
        any_of.retain(|cap| !cap.trim().is_empty());
        any_of.sort();
        any_of.dedup();
        Self { any_of }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.any_of.is_empty()
    }

    pub(crate) fn satisfied_by_caps(&self, caps: &[String]) -> bool {
        self.any_of
            .iter()
            .any(|required| caps.iter().any(|cap| required == cap))
    }

    pub(crate) fn satisfied_by_leases(&self, leases: &[ValidatedLease]) -> bool {
        self.any_of.iter().any(|required| {
            leases
                .iter()
                .any(|lease| capability_satisfies(required, &lease.capability))
        })
    }

    pub(crate) fn representative(&self) -> String {
        self.any_of
            .first()
            .cloned()
            .unwrap_or_else(|| "capability".to_string())
    }

    pub(crate) fn options(&self) -> &[String] {
        &self.any_of
    }
}

pub(crate) fn derive_capability_requirements(
    req: &SandboxRequirements,
) -> Vec<CapabilityRequirement> {
    let mut requirements = Vec::new();
    if req.needs_network.unwrap_or(false) {
        let net_caps = vec!["net:https", "net:http", "io:egress"];
        let req = CapabilityRequirement::any_of(net_caps);
        if !req.is_empty() {
            requirements.push(req);
        }
    }
    if !req.filesystem_scopes.is_empty() {
        let fs_caps = vec!["fs", "fs:read", "fs:write", "fs:patch"];
        let req = CapabilityRequirement::any_of(fs_caps);
        if !req.is_empty() {
            requirements.push(req);
        }
    }
    requirements
}

pub(crate) fn flatten_capability_requirements(reqs: &[CapabilityRequirement]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut flattened = Vec::new();
    for req in reqs {
        for cap in req.options() {
            if seen.insert(cap) {
                flattened.push(cap.clone());
            }
        }
    }
    flattened
}

fn capability_satisfies(required: &str, lease_capability: &str) -> bool {
    if required == lease_capability {
        return true;
    }
    match required {
        "net:http" | "net:https" => {
            lease_capability == "io:egress" || lease_capability.starts_with("net:")
        }
        "io:egress" => lease_capability == "io:egress",
        "fs" | "fs:read" | "fs:write" | "fs:patch" => lease_capability.starts_with("fs"),
        _ => false,
    }
}

pub(crate) fn validation_status_label(status: &str) -> &'static str {
    normalize_validation_status(status).as_str()
}
