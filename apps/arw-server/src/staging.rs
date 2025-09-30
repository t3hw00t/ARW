use anyhow::{anyhow, Result};
use chrono::SecondsFormat;
use serde_json::json;
use std::collections::HashSet;

use crate::AppState;
use arw_topics as topics;

fn staging_mode() -> StageMode {
    std::env::var("ARW_ACTION_STAGING_MODE")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .map(|s| match s.as_str() {
            "always" => StageMode::Always,
            "ask" => StageMode::Ask,
            _ => StageMode::Auto,
        })
        .unwrap_or(StageMode::Auto)
}

fn staging_allow_set() -> HashSet<String> {
    std::env::var("ARW_ACTION_STAGING_ALLOW")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|item| {
                    let trimmed = item.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_project(input: &serde_json::Value) -> Option<String> {
    for key in ["project", "proj", "project_id", "workspace"] {
        if let Some(val) = input.get(key) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

fn extract_evidence(input: &serde_json::Value) -> Option<serde_json::Value> {
    input.get("evidence").cloned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageMode {
    Auto,
    Ask,
    Always,
}

fn should_stage(kind: &str) -> bool {
    match staging_mode() {
        StageMode::Auto => false,
        StageMode::Always => true,
        StageMode::Ask => {
            let allow = staging_allow_set();
            !allow.contains(kind)
        }
    }
}

pub async fn maybe_stage_action(
    state: &AppState,
    kind: &str,
    input: &serde_json::Value,
) -> Result<Option<String>> {
    if !state.kernel_enabled() {
        return Ok(None);
    }
    if !should_stage(kind) {
        return Ok(None);
    }
    let project = extract_project(input);
    let evidence = extract_evidence(input);
    let requested_by = std::env::var("ARW_ACTION_STAGING_ACTOR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| Some("local".to_string()));
    let id = state
        .kernel()
        .insert_staging_action_async(
            kind.to_string(),
            input.clone(),
            project.clone(),
            requested_by.clone(),
            evidence,
        )
        .await?;
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    state.bus().publish(
        topics::TOPIC_STAGING_PENDING,
        &json!({
            "id": id,
            "kind": kind,
            "project": project,
            "requested_by": requested_by,
            "time": now,
        }),
    );
    Ok(Some(id))
}

pub async fn approve_action(
    state: &AppState,
    staging_id: &str,
    decided_by: Option<String>,
) -> Result<String> {
    let Some(record) = state
        .kernel()
        .get_staging_action_async(staging_id.to_string())
        .await?
    else {
        return Err(anyhow!("staging entry not found"));
    };
    if record.status != "pending" {
        return Err(anyhow!("staging entry is not pending"));
    }
    let action_id = uuid::Uuid::new_v4().to_string();
    state
        .kernel()
        .insert_action_async(
            &action_id,
            &record.action_kind,
            &record.action_input,
            None,
            None,
            "queued",
        )
        .await?;
    state.signal_action_queue();
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    state
        .kernel()
        .update_staging_action_status_async(
            staging_id.to_string(),
            "approved".to_string(),
            Some("approved".to_string()),
            decided_by.clone(),
            Some(now.clone()),
            Some(action_id.clone()),
        )
        .await?;
    state
        .kernel()
        .append_contribution_async(
            "local",
            "task.submit",
            1.0,
            "task",
            None,
            record.project.as_deref(),
            None,
        )
        .await?;
    state.bus().publish(
        topics::TOPIC_STAGING_DECIDED,
        &json!({
            "id": staging_id,
            "decision": "approved",
            "action_id": action_id,
            "kind": record.action_kind,
            "project": record.project,
            "decided_by": decided_by,
            "time": now,
        }),
    );
    // Surface the usual action submitted event so downstream consumers stay in sync
    let submitted_payload = json!({
        "id": action_id,
        "kind": record.action_kind,
        "status": "queued",
    });
    let submitted_env = arw_events::Envelope {
        time: now,
        kind: topics::TOPIC_ACTIONS_SUBMITTED.into(),
        payload: submitted_payload,
        policy: None,
        ce: None,
    };
    state
        .bus()
        .publish(&submitted_env.kind, &submitted_env.payload);
    Ok(action_id)
}

pub async fn deny_action(
    state: &AppState,
    staging_id: &str,
    reason: Option<String>,
    decided_by: Option<String>,
) -> Result<()> {
    let Some(record) = state
        .kernel()
        .get_staging_action_async(staging_id.to_string())
        .await?
    else {
        return Err(anyhow!("staging entry not found"));
    };
    if record.status != "pending" {
        return Err(anyhow!("staging entry is not pending"));
    }
    let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    state
        .kernel()
        .update_staging_action_status_async(
            staging_id.to_string(),
            "denied".to_string(),
            reason.clone(),
            decided_by.clone(),
            Some(now.clone()),
            None,
        )
        .await?;
    state.bus().publish(
        topics::TOPIC_STAGING_DECIDED,
        &json!({
            "id": staging_id,
            "decision": "denied",
            "reason": reason,
            "kind": record.action_kind,
            "project": record.project,
            "decided_by": decided_by,
            "time": now,
        }),
    );
    Ok(())
}

pub fn mode_label() -> &'static str {
    match staging_mode() {
        StageMode::Auto => "auto",
        StageMode::Ask => "ask",
        StageMode::Always => "always",
    }
}
