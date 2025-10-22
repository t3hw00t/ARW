use std::collections::HashSet;

use anyhow::{Context, Result as AnyhowResult};
use metrics::histogram;
use serde_json::{json, Map, Value};
use tracing::warn;

use crate::{memory_service, read_models, AppState};
use memory_service::MemoryUpsertInput;

use super::types::{
    flatten_capability_requirements, validation_status_label, AgentPayload, ValidatedAgentMessage,
    ValidatedToolInvocation,
};

const SHORT_TERM_TTL_SECS_DEFAULT: i64 = 900;
const SHORT_TERM_TTL_ENV: &str = "ARW_MEMORY_SHORT_TTL_SECS";
const MODULAR_SHORT_TERM_SOURCE: &str = "modular.agent.short_term";
const MODULAR_EPISODIC_SOURCE: &str = "modular.agent.episodic";
const MODULAR_TOOL_SHORT_TERM_SOURCE: &str = "modular.tool.short_term";
const MODULAR_TOOL_EPISODIC_SOURCE: &str = "modular.tool.episodic";

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
    let persona_id = validated.message.persona_id.clone();

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
        persona_id: persona_id.clone(),
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
            "persona_id": persona_id.clone(),
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
        persona_id: persona_id.clone(),
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
            "persona_id": persona_id,
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

pub async fn persist_tool_memory(
    state: &AppState,
    validated: &ValidatedToolInvocation,
    summary: &Value,
) {
    if let Err(err) = persist_tool_memory_inner(state, validated, summary).await {
        warn!(
            target: "arw::modular",
            error = %err,
            tool_id = %validated.invocation.tool_id,
            invocation_id = %validated.invocation.invocation_id,
            "failed to persist modular tool invocation memory"
        );
    }
}

async fn persist_tool_memory_inner(
    state: &AppState,
    validated: &ValidatedToolInvocation,
    summary: &Value,
) -> AnyhowResult<()> {
    let invocation = &validated.invocation;
    let persona_id = invocation.persona_id.clone();
    let sandbox_value =
        serde_json::to_value(&invocation.sandbox_requirements).unwrap_or_else(|_| json!({}));
    let policy_scope_value = summary.get("policy_scope").cloned().unwrap_or_else(|| {
        json!({
            "leases": validated
                .leases
                .iter()
                .map(|lease| lease.to_value())
                .collect::<Vec<_>>(),
            "capabilities": invocation
                .policy_scope
                .capabilities
                .clone()
                .unwrap_or_default(),
            "requires_human_review": invocation.policy_scope.requires_human_review(),
        })
    });
    let required_caps = flatten_capability_requirements(&validated.required_capabilities);
    let result_status = summary
        .get("result_status")
        .cloned()
        .unwrap_or(json!("pending"));

    let mut record_map = Map::new();
    record_map.insert("invocation_id".into(), json!(invocation.invocation_id));
    record_map.insert("requested_by".into(), json!(invocation.requested_by));
    record_map.insert("tool_id".into(), json!(invocation.tool_id));
    record_map.insert("operation_id".into(), json!(invocation.operation_id));
    record_map.insert("input_payload".into(), invocation.input_payload.clone());
    record_map.insert("sandbox_requirements".into(), sandbox_value.clone());
    record_map.insert("policy_scope".into(), policy_scope_value.clone());
    record_map.insert("required_capabilities".into(), json!(required_caps.clone()));
    record_map.insert("summary".into(), summary.clone());
    record_map.insert("payload_kind".into(), json!("tool_invocation"));
    record_map.insert("result_status".into(), result_status.clone());
    if let Some(lifecycle) = summary.get("lifecycle").cloned() {
        record_map.insert("lifecycle".into(), lifecycle);
    }
    if let Some(payload_summary) = summary.get("payload_summary").cloned() {
        record_map.insert("payload_summary".into(), payload_summary);
    }
    if let Some(value) = summary.get("result_latency_ms").cloned() {
        record_map.insert("result_latency_ms".into(), value);
    }
    if let Some(value) = summary.get("result_error").cloned() {
        record_map.insert("result_error".into(), value);
    }
    if let Some(value) = summary.get("result_output_keys").cloned() {
        record_map.insert("result_output_keys".into(), value);
    }
    if let Some(value) = summary.get("started_ms").cloned() {
        record_map.insert("started_ms".into(), value);
    }
    if let Some(value) = summary.get("completed_ms").cloned() {
        record_map.insert("completed_ms".into(), value);
    }
    if let Some(value) = summary.get("created_ms").cloned() {
        record_map.insert("created_ms".into(), value);
    }
    record_map.insert("evidence_id".into(), json!(invocation.evidence_id));

    let base_value = Value::Object(record_map.clone());
    let summary_excerpt = format!(
        "{} · {}",
        invocation.tool_id,
        result_status.as_str().unwrap_or("pending")
    );

    let mut extra_map = Map::new();
    extra_map.insert("payload_kind".into(), json!("tool_invocation"));
    extra_map.insert("summary_excerpt".into(), json!(summary_excerpt.clone()));
    if !required_caps.is_empty() {
        extra_map.insert("required_capabilities".into(), json!(required_caps.clone()));
    }

    let short_text = format!(
        "tool {} ({})",
        invocation.tool_id,
        result_status.as_str().unwrap_or("pending")
    );
    let mut tags = vec!["modular".to_string(), "tool_invocation".to_string()];
    tags.push(invocation.tool_id.clone());
    tags.retain(|tag| !tag.is_empty());
    tags.sort();
    tags.dedup();
    let mut keywords = vec![
        "modular".to_string(),
        "tool".to_string(),
        invocation.tool_id.clone(),
        invocation.operation_id.clone(),
    ];
    keywords.sort();
    keywords.dedup();

    let short_term_input = MemoryUpsertInput {
        lane: "short_term".to_string(),
        kind: Some("tool.invocation".to_string()),
        key: Some(invocation.invocation_id.clone()),
        value: base_value.clone(),
        text: Some(short_text.clone()),
        agent_id: Some(invocation.requested_by.clone()),
        persona_id: persona_id.clone(),
        ttl_s: Some(short_term_ttl_secs()),
        tags: tags.clone(),
        keywords: keywords.clone(),
        durability: Some("volatile".to_string()),
        privacy: Some("private".to_string()),
        source: json!({
            "kind": "modular_tool_invocation",
            "lane": "short_term",
            "invocation_id": invocation.invocation_id,
            "tool_id": invocation.tool_id,
            "persona_id": persona_id.clone(),
        }),
        extra: Value::Object(extra_map.clone()),
        dedupe: true,
        ..Default::default()
    };
    let short_result =
        memory_service::upsert_memory(state, short_term_input, MODULAR_TOOL_SHORT_TERM_SOURCE)
            .await
            .context("persist short-term tool invocation memory")?;

    let mut episodic_extra = extra_map;
    episodic_extra.insert("short_term_id".into(), json!(short_result.id));
    let episodic_input = MemoryUpsertInput {
        lane: "episodic".to_string(),
        kind: Some("tool.invocation".to_string()),
        key: Some(invocation.invocation_id.clone()),
        value: base_value,
        text: Some(short_text),
        agent_id: Some(invocation.requested_by.clone()),
        persona_id,
        tags,
        keywords,
        durability: Some("short".to_string()),
        privacy: Some("private".to_string()),
        source: json!({
            "kind": "modular_tool_invocation",
            "lane": "episodic",
            "invocation_id": invocation.invocation_id,
            "tool_id": invocation.tool_id,
            "persona_id": invocation.persona_id.clone(),
        }),
        extra: Value::Object(episodic_extra),
        dedupe: true,
        ..Default::default()
    };
    memory_service::upsert_memory(state, episodic_input, MODULAR_TOOL_EPISODIC_SOURCE)
        .await
        .context("persist episodic tool invocation memory")?;

    match state.kernel().list_recent_memory_async(None, 200).await {
        Ok(items) => {
            let bundle = read_models::build_memory_recent_bundle(items);
            read_models::publish_memory_bundle(&state.bus(), &bundle);
        }
        Err(err) => {
            warn!(
                target: "arw::modular",
                error = %err,
                "failed to refresh memory_recent snapshot after modular tool invocation"
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
                "status": validation_status_label(&validation.status),
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
