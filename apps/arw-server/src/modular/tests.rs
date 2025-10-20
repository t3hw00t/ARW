use super::*;
use crate::modular::types::AgentPayload;
use crate::test_support;
use crate::AppState;
use chrono::{Duration, SecondsFormat, Utc};
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
async fn requires_human_review_sets_pending_stage() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "context:read").await;
    let body = json!({
        "agent_id": "assistant.chat",
        "turn_id": "turn-789",
        "intent": "draft_response",
        "payload": { "text": "needs approver" },
        "context_refs": [],
        "evidence_ids": [],
        "confidence": 0.7,
        "latency_budget_ms": 400,
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": ["context:read"],
            "requires_human_review": true
        }
    });

    let validated = validate_agent_message(&state, &body)
        .await
        .expect("validation succeeds");
    assert_eq!(validated.lifecycle.stage_str(), "pending_human_review");
    assert_eq!(validated.lifecycle.validation_gate_str(), "required");

    let summary = agent_message_summary(&validated);
    assert_eq!(summary["lifecycle"]["stage"], json!("pending_human_review"));
    assert_eq!(summary["lifecycle"]["validation_gate"], json!("required"));
}

#[tokio::test]
async fn validation_agent_with_blocked_status_sets_blocked_stage() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "validation:run").await;
    let body = json!({
        "agent_id": "validation.guard",
        "turn_id": "turn-901",
        "intent": "validation_report",
        "payload": {
            "status": "blocked",
            "findings": [],
            "summary": "guard failed"
        },
        "context_refs": [],
        "evidence_ids": [],
        "confidence": 0.4,
        "latency_budget_ms": 250,
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": ["validation:run"],
            "requires_human_review": false
        }
    });

    let validated = validate_agent_message(&state, &body)
        .await
        .expect("validation succeeds");
    assert_eq!(validated.lifecycle.stage_str(), "blocked");
    assert_eq!(validated.lifecycle.validation_gate_str(), "rejected");

    let summary = agent_message_summary(&validated);
    assert_eq!(summary["lifecycle"]["stage"], json!("blocked"));
    assert_eq!(summary["lifecycle"]["validation_gate"], json!("rejected"));
    assert_eq!(summary["payload_summary"]["status"], json!("blocked"));
}

#[tokio::test]
async fn missing_capability_causes_validation_error() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "tool:execute").await;
    let body = json!({
        "agent_id": "assistant.chat",
        "turn_id": "turn-333",
        "intent": "draft_response",
        "payload": { "text": "capability mismatch" },
        "context_refs": [],
        "evidence_ids": [],
        "confidence": 0.5,
        "latency_budget_ms": 500,
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": ["context:read"],
            "requires_human_review": false
        }
    });

    let err = validate_agent_message(&state, &body)
        .await
        .expect_err("validation should fail");
    match err {
        ModularValidationError::MissingCapability { capability } => {
            assert_eq!(capability, "context:read");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn validate_tool_invocation_accepts_basic_payload() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "sandbox:tool").await;
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
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": []
        },
        "evidence_id": "evidence-456"
    });

    let validated = validate_tool_invocation(&state, &body)
        .await
        .expect("tool invocation valid");
    assert_eq!(validated.invocation.operation_id, "memory.search@1.0.0");
}

#[tokio::test]
async fn validate_tool_invocation_requires_active_lease() {
    let (state, _tmp) = test_state().await;
    let body = json!({
        "invocation_id": "invoke-999",
        "requested_by": "agent.validation",
        "tool_id": "http.fetch",
        "operation_id": "http.fetch@1.0.0",
        "input_payload": { "url": "https://example.com" },
        "sandbox_requirements": {
            "needs_network": true,
            "filesystem_scopes": []
        },
        "policy_scope": {
            "leases": ["missing-lease"],
            "capabilities": ["net:http", "io:egress"]
        },
        "evidence_id": "evidence-lease"
    });

    let err = validate_tool_invocation(&state, &body)
        .await
        .expect_err("validation should fail");
    match err {
        ModularValidationError::MissingLease { id } => {
            assert_eq!(id, "missing-lease");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn validate_tool_invocation_enforces_capability_declaration() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "net:http").await;
    let body = json!({
        "invocation_id": "invoke-124",
        "requested_by": "agent.validation",
        "tool_id": "http.fetch",
        "operation_id": "http.fetch@1.0.0",
        "input_payload": { "url": "https://example.com" },
        "sandbox_requirements": {
            "needs_network": true,
            "filesystem_scopes": []
        },
        "policy_scope": {
            "leases": [lease_id.clone()],
            "capabilities": []
        },
        "evidence_id": "evidence-457"
    });

    let err = validate_tool_invocation(&state, &body)
        .await
        .expect_err("validation should fail");
    match err {
        ModularValidationError::MissingCapability { capability } => {
            assert_eq!(capability, "io:egress");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let lease_id = seed_lease(&state, "fs").await;
    let body = json!({
        "invocation_id": "invoke-125",
        "requested_by": "agent.validation",
        "tool_id": "http.fetch",
        "operation_id": "http.fetch@1.0.0",
        "input_payload": { "url": "https://example.com" },
        "sandbox_requirements": {
            "needs_network": true,
            "filesystem_scopes": []
        },
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": ["net:http", "io:egress"]
        },
        "evidence_id": "evidence-458"
    });

    let err = validate_tool_invocation(&state, &body)
        .await
        .expect_err("validation should fail");
    match err {
        ModularValidationError::MissingCapability { capability } => {
            assert_eq!(capability, "io:egress");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn validate_tool_invocation_accepts_io_egress_alias() {
    let (state, _tmp) = test_state().await;
    let lease_id = seed_lease(&state, "io:egress").await;
    let body = json!({
        "invocation_id": "invoke-126",
        "requested_by": "agent.validation",
        "tool_id": "http.fetch",
        "operation_id": "http.fetch@1.0.0",
        "input_payload": { "url": "https://example.com" },
        "sandbox_requirements": {
            "needs_network": true,
            "filesystem_scopes": []
        },
        "policy_scope": {
            "leases": [lease_id],
            "capabilities": ["io:egress"]
        },
        "evidence_id": "evidence-459"
    });

    let validated = validate_tool_invocation(&state, &body)
        .await
        .expect("validation succeeds with io:egress lease");
    assert_eq!(validated.invocation.tool_id, "http.fetch");
}

#[tokio::test]
async fn tool_invocation_summary_includes_policy_scope() {
    let (state, _tmp) = test_state().await;
    let net_lease = seed_lease(&state, "io:egress").await;
    let fs_lease = seed_lease(&state, "fs").await;
    let body = json!({
        "invocation_id": "invoke-200",
        "requested_by": "agent.tools",
        "tool_id": "fs.patch",
        "operation_id": "fs.patch@1.0.0",
        "input_payload": {
            "path": "project://notes.md",
            "contents": "updated"
        },
        "sandbox_requirements": {
            "needs_network": true,
            "filesystem_scopes": ["project://notes.md"]
        },
        "policy_scope": {
            "leases": [net_lease.clone(), fs_lease.clone()],
            "capabilities": ["io:egress", "fs"]
        },
        "evidence_id": "evidence-summary"
    });

    let validated = validate_tool_invocation(&state, &body)
        .await
        .expect("tool invocation valid");
    let summary = tool_invocation_summary(&validated);
    assert_eq!(summary["payload_kind"], json!("tool_invocation"));
    assert_eq!(summary["tool_id"], json!("fs.patch"));
    let caps = summary["required_capabilities"]
        .as_array()
        .expect("required caps array");
    assert!(caps.iter().any(|cap| cap == "io:egress"));
    assert!(caps.iter().any(|cap| cap == "fs" || cap == "fs:read"));
    let leases = summary["policy_scope"]["leases"]
        .as_array()
        .expect("leases array");
    assert_eq!(leases.len(), 2);
    assert!(summary["sandbox_requirements"].is_object());
}
