use super::{default_model, hints, ok};
use arw_macros::arw_admin;
use axum::{extract::Query, response::IntoResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct AssembleQs {
    pub proj: Option<String>,
    pub q: Option<String>,
    pub k: Option<usize>,
}

#[arw_admin(
    method = "GET",
    path = "/admin/context/assemble",
    summary = "Assemble minimal context: top‑K beliefs + policy/model hints"
)]
pub async fn assemble_get(Query(q): Query<AssembleQs>) -> impl IntoResponse {
    let proj_opt = q.proj.as_deref();
    let items = super::world::select_top_claims(proj_opt, q.q.as_deref().unwrap_or(""), q.k.unwrap_or(8)).await;
    // Include minimal policy + model context
    let policy_hints = { hints().read().await.clone() };
    let model_default = { default_model().read().await.clone() };
    let proj = proj_opt.map(|s| s.to_string());
    let notes_path = proj
        .as_deref()
        .and_then(|p| super::paths::project_notes_path(p))
        .map(|p| p.to_string_lossy().to_string());
    // Recent files (from world model entities)
    let files = super::world::select_recent_files(proj_opt, 20).await;
    // Include recent intents/actions (size-bounded), optionally filtered by proj when present
    let mut intents = super::state_api::intents_snapshot().await;
    let mut actions = super::state_api::actions_snapshot().await;
    if let Some(p) = proj_opt {
        let pv = serde_json::Value::String(p.to_string());
        intents.retain(|it| it.get("payload").and_then(|v| v.get("proj")).unwrap_or(&serde_json::Value::Null) == &pv);
        actions.retain(|it| it.get("payload").and_then(|v| v.get("proj")).unwrap_or(&serde_json::Value::Null) == &pv);
    }
    // Keep most recent 20 each
    if intents.len() > 20 { intents = intents[intents.len()-20..].to_vec(); }
    if actions.len() > 20 { actions = actions[actions.len()-20..].to_vec(); }
    ok(json!({
        "beliefs": items,
        "recent": { "intents": intents, "actions": actions, "files": files },
        "policy": { "hints": policy_hints },
        "model": { "default": model_default },
        "project": { "name": proj, "notes": notes_path },
        "params": { "proj": q.proj, "q": q.q, "k": q.k.unwrap_or(8) }
    }))
}
