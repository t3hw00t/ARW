use super::{default_model, hints, ok};
use arw_macros::arw_admin;
use axum::{extract::{Query, State, Path}, response::IntoResponse, Json};
use crate::AppState;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct AssembleQs {
    pub proj: Option<String>,
    pub q: Option<String>,
    pub k: Option<usize>,
    // Evidence budget (number of belief items). Defaults to k
    pub evidence_k: Option<usize>,
    // Diversity knob for beliefs selection (0..1). Optional.
    pub div: Option<f64>,
    // Slot budgets (token ceilings); passthrough for UI/planner; server echoes them.
    pub s_inst: Option<usize>, // instructions
    pub s_plan: Option<usize>, // plan
    pub s_policy: Option<usize>, // safety/policy
    pub s_evid: Option<usize>, // evidence
    pub s_nice: Option<usize>, // nice-to-have
    // Optional per-lane caps for recents (tokens)
    pub s_intents: Option<usize>,
    pub s_actions: Option<usize>,
    pub s_files: Option<usize>,
    // Optional total prompt budget (tokens), informational
    pub s_total: Option<usize>,
}

#[arw_admin(
    method = "GET",
    path = "/admin/context/assemble",
    summary = "Assemble minimal context: top‑K beliefs + policy/model hints"
)]
pub async fn assemble_get(State(state): State<AppState>, Query(q): Query<AssembleQs>) -> impl IntoResponse {
    let proj_opt = q.proj.as_deref();
    // Read current policy hints (may include mode/slo_ms)
    let policy_hints = { hints().read().await.clone() };
    // Mode-driven defaults for retrieval and reasoning gates
    let mode_s = policy_hints
        .mode
        .as_deref()
        .unwrap_or("balanced")
        .to_ascii_lowercase();
    let (mode_k, mode_div, verify_pass, vote_k) = match mode_s.as_str() {
        // Quick: smaller k, diversity on, no verify, no self-consistency
        "quick" => (6usize, Some(0.3f64), false, 0u8),
        // Deep: larger k and enable heavier self-consistency
        "deep" => (20usize, Some(0.3f64), false, 5u8),
        // Verified: larger k and a verifier pass flag
        "verified" => (20usize, Some(0.3f64), true, 3u8),
        // Balanced: default k=12, diversity on, light self-consistency
        _ => (12usize, Some(0.3f64), false, 3u8),
    };
    let k_default = q.k.unwrap_or(mode_k);
    let evid_k = q.evidence_k.unwrap_or(k_default);
    let div_used: Option<f64> = match q.div {
        Some(d) => Some(d),
        None => mode_div,
    };
    // Estimate pool size for coverage metrics (top-50 as proxy)
    let pool: Vec<serde_json::Value> = super::world::select_top_claims(
        proj_opt,
        q.q.as_deref().unwrap_or(""),
        50,
    )
    .await;
    // Beliefs: use diversity-aware selection when requested or defaulted by mode
    let items_initial = if let Some(div) = div_used {
        super::world::select_top_claims_diverse(proj_opt, q.q.as_deref().unwrap_or(""), evid_k, div).await
    } else {
        super::world::select_top_claims(proj_opt, q.q.as_deref().unwrap_or(""), evid_k).await
    };
    // Evidence slot budget: approximate tokens and cap selection
    fn est_tokens_value(v: &serde_json::Value) -> u64 {
        match v {
            serde_json::Value::String(s) => ((s.len() as u64) + 3) / 4, // ~4 chars/token
            serde_json::Value::Number(_) => 1,
            serde_json::Value::Bool(_) => 1,
            serde_json::Value::Array(a) => a.iter().map(est_tokens_value).sum(),
            serde_json::Value::Object(o) => o.values().map(est_tokens_value).sum(),
            _ => 0,
        }
    }
    fn est_tokens_belief(v: &serde_json::Value) -> u64 {
        let mut t = 6; // overhead
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) { t += ((id.len() as u64)+3)/4; }
        if let Some(props) = v.get("props") { t += est_tokens_value(props); }
        if let Some(name) = v.get("name") { t += est_tokens_value(name); }
        if let Some(text) = v.get("text") { t += est_tokens_value(text); }
        if let Some(trace) = v.get("trace") { t += est_tokens_value(trace); }
        t.min(512) // cap per item
    }
    let mut used_tokens: u64 = 0;
    let budget_tokens: Option<u64> = q.s_evid.map(|n| n as u64).filter(|n| *n > 0);
    let mut items: Vec<serde_json::Value> = Vec::new();
    for it in items_initial.into_iter() {
        let est = est_tokens_belief(&it);
        if let Some(b) = budget_tokens {
            if used_tokens.saturating_add(est) > b { break; }
        }
        used_tokens = used_tokens.saturating_add(est);
        items.push(it);
    }
    // Include minimal policy + model context (policy_hints already loaded)
    let model_default = { default_model().read().await.clone() };
    let proj = proj_opt.map(|s| s.to_string());
    let notes_path = proj
        .as_deref()
        .and_then(|p| super::paths::project_notes_path(p))
        .map(|p| p.to_string_lossy().to_string());
    // Recent files (from world model entities)
    let mut files = super::world::select_recent_files(proj_opt, 20).await;
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
    // Estimate tokens for recents and apply optional per-lane caps before attaching ptrs
    fn est_tokens_event(ev: &serde_json::Value) -> u64 {
        let mut t = 4;
        if let Some(k) = ev.get("kind") { t += est_tokens_value(k); }
        if let Some(p) = ev.get("payload") { t += est_tokens_value(p); }
        if let Some(ts) = ev.get("time") { t += est_tokens_value(ts); }
        t.min(1024)
    }
    fn est_tokens_file(f: &serde_json::Value) -> u64 {
        let mut t = 2;
        if let Some(p) = f.get("path") { t += est_tokens_value(p); }
        if let Some(id) = f.get("id") { t += est_tokens_value(id); }
        t.min(256)
    }
    fn cap_by_tokens(mut items: Vec<serde_json::Value>, budget: Option<u64>, est: fn(&serde_json::Value) -> u64) -> (Vec<serde_json::Value>, u64) {
        if let Some(b) = budget { if b == 0 { return (Vec::new(), 0); } }
        let Some(b) = budget else {
            let used = items.iter().map(est).sum();
            return (items, used);
        };
        let mut out: Vec<serde_json::Value> = Vec::with_capacity(items.len());
        let mut used: u64 = 0;
        for it in items.drain(..) {
            let t = est(&it);
            if used.saturating_add(t) > b { break; }
            used = used.saturating_add(t);
            out.push(it);
        }
        (out, used)
    }
    let intents_budget = q.s_intents.map(|x| x as u64).filter(|x| *x > 0);
    let actions_budget = q.s_actions.map(|x| x as u64).filter(|x| *x > 0);
    let files_budget = q.s_files.map(|x| x as u64).filter(|x| *x > 0);
    let (new_intents, intents_tokens) = cap_by_tokens(intents, intents_budget, est_tokens_event);
    intents = new_intents;
    let (new_actions, actions_tokens) = cap_by_tokens(actions, actions_budget, est_tokens_event);
    actions = new_actions;
    let (new_files, files_tokens) = cap_by_tokens(files, files_budget, est_tokens_file);
    files = new_files;
    // Attach stable pointers alongside included items
    fn with_ptrs_beliefs(mut v: Vec<serde_json::Value>, proj: Option<&str>) -> Vec<serde_json::Value> {
        for it in v.iter_mut() {
            if let Some(id) = it.get("id").and_then(|x| x.as_str()) {
                it["ptr"] = json!({
                    "kind": "world_belief",
                    "id": id,
                    "source": "/admin/state/world",
                    "proj": proj,
                });
            }
        }
        v
    }
    fn with_ptrs_files(mut v: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        for it in v.iter_mut() {
            if let Some(path) = it.get("path").and_then(|x| x.as_str()) {
                it["ptr"] = json!({"kind": "file", "path": path});
            }
        }
        v
    }
    fn with_ptrs_events(mut v: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
        for it in v.iter_mut() {
            let cid = it
                .get("payload")
                .and_then(|p| p.get("corr_id"))
                .and_then(|x| x.as_str());
            if let Some(c) = cid {
                it["ptr"] = json!({"kind": "episode", "corr_id": c, "source": "/admin/state/episodes"});
            } else if let (Some(t), Some(k)) = (it.get("time"), it.get("kind")) {
                it["ptr"] = json!({"kind": "event", "time": t, "code": k});
            }
        }
        v
    }
    let beliefs = with_ptrs_beliefs(items, proj_opt);
    let files = with_ptrs_files(files);
    let intents = with_ptrs_events(intents);
    let actions = with_ptrs_events(actions);
    let coverage_pool = pool.len();
    let coverage_selected = beliefs.len();
    let coverage_omitted = coverage_pool.saturating_sub(coverage_selected);
    // Emit assembly summary event
    {
        let total_tokens_used: u64 = used_tokens + intents_tokens + actions_tokens + files_tokens;
        let mut ev = json!({
            "proj": proj,
            "query": q.q,
            "k": k_default,
            "evidence_k": evid_k,
            "diversity": div_used,
            "counts": {"beliefs": coverage_selected, "files": files.len(), "intents": intents.len(), "actions": actions.len()},
            "coverage": {"pool": coverage_pool, "omitted": coverage_omitted, "recall_risk": coverage_omitted > 0},
            "usage": {"evidence_tokens": used_tokens, "evidence_budget": budget_tokens, "recent_intents_tokens": intents_tokens, "recent_actions_tokens": actions_tokens, "recent_files_tokens": files_tokens, "total_tokens": total_tokens_used},
        });
        crate::ext::corr::ensure_corr(&mut ev);
        state.bus.publish("Context.Assembled", &ev);
        if coverage_omitted > 0 {
            let mut ev2 = json!({
                "proj": ev["proj"].clone(),
                "query": ev["query"].clone(),
                "pool": coverage_pool,
                "omitted": coverage_omitted,
            });
            crate::ext::corr::ensure_corr(&mut ev2);
            state.bus.publish("Context.Coverage", &ev2);
        }
    }
    // Planner meta (mode-driven suggestions for downstream agents)
    let planner_meta = json!({
        "mode": mode_s,
        "verify_pass": verify_pass,
        "consistency": { "vote_k": vote_k },
        "retrieval": { "k": k_default, "evidence_k": evid_k, "div": div_used }
    });
    ok(json!({
        "beliefs": beliefs,
        "recent": { "intents": intents, "actions": actions, "files": files },
        "policy": { "hints": policy_hints, "planner": planner_meta },
        "model": { "default": model_default },
        "project": { "name": proj, "notes": notes_path },
        "budget": { "slots": { "instructions": q.s_inst, "plan": q.s_plan, "policy": q.s_policy, "evidence": q.s_evid, "nice": q.s_nice, "intents": q.s_intents, "actions": q.s_actions, "files": q.s_files, "total": q.s_total },
                     "requested": { "k": k_default, "evidence_k": evid_k, "diversity": div_used },
                     "usage": { "evidence_tokens": used_tokens, "recent_intents_tokens": intents_tokens, "recent_actions_tokens": actions_tokens, "recent_files_tokens": files_tokens, "total_tokens": used_tokens + intents_tokens + actions_tokens + files_tokens } },
        "coverage": { "pool": coverage_pool, "selected": coverage_selected, "omitted": coverage_omitted, "recall_risk": coverage_omitted > 0, "evidence_tokens_used": used_tokens, "evidence_tokens_budget": budget_tokens },
        "params": { "proj": q.proj, "q": q.q, "k": k_default, "evidence_k": evid_k, "div": div_used, "s_inst": q.s_inst, "s_plan": q.s_plan, "s_policy": q.s_policy, "s_evid": q.s_evid, "s_nice": q.s_nice }
    }))
}

// ------------- Rehydrate API -------------

#[derive(Deserialize, utoipa::ToSchema)]
pub struct RehydrateReq {
    ptr: serde_json::Value,
}

#[arw_admin(
    method = "POST",
    path = "/admin/context/rehydrate",
    summary = "Rehydrate a pointer (belief/file) into full content"
)]
pub async fn rehydrate_post(State(state): State<AppState>, Json(req): Json<RehydrateReq>) -> impl IntoResponse {
    use tokio::fs as afs;
    let kind = req.ptr.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "world_belief" => {
            let id = match req.ptr.get("id").and_then(|v| v.as_str()) { Some(s) => s, None => return super::ApiError::bad_request("missing id").into_response() };
            let proj = req.ptr.get("proj").and_then(|v| v.as_str());
            if let Some(node) = super::world::get_belief_node(proj, id) {
                let v = serde_json::to_value(&node).unwrap_or_else(|_| serde_json::json!({"id": id}));
                return ok(serde_json::json!({"ptr": req.ptr, "belief": v})).into_response();
            }
            super::ApiError::not_found("belief not found").into_response()
        }
        "episode" => {
            let cid = match req.ptr.get("corr_id").and_then(|v| v.as_str()) { Some(s) => s.to_string(), None => return super::ApiError::bad_request("missing corr_id").into_response() };
            // Delegate to episode snapshot endpoint
            return super::state_api::episode_snapshot_get(State(state), Path(cid)).await.into_response();
        }
        "file" => {
            // Gate file rehydrate via policy key; deny by default
            if !arw_core::gating::allowed("context:rehydrate:file") {
                return super::ApiError::forbidden("gated").into_response();
            }
            let path = match req.ptr.get("path").and_then(|v| v.as_str()) { Some(s) => std::path::PathBuf::from(s), None => return super::ApiError::bad_request("missing path").into_response() };
            // Best-effort safeguard: limit to reasonably-sized files and return head only
            let meta = match afs::metadata(&path).await { Ok(m) => m, Err(_) => return super::ApiError::not_found("file not found").into_response() };
            if !meta.is_file() { return super::ApiError::bad_request("not a file").into_response(); }
            let size = meta.len();
            let cap: u64 = std::env::var("ARW_REHYDRATE_FILE_HEAD_KB").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(64) * 1024;
            let take = size.min(cap);
            let mut f = match afs::File::open(&path).await { Ok(f) => f, Err(_) => return super::ApiError::not_found("file open failed").into_response() };
            let mut buf = vec![0u8; take as usize];
            use tokio::io::AsyncReadExt;
            let n = match f.read(&mut buf).await { Ok(n) => n, Err(_) => 0 };
            let content = String::from_utf8_lossy(&buf[..n]).to_string();
            return ok(serde_json::json!({
                "ptr": req.ptr,
                "file": {"path": path.to_string_lossy(), "size": size, "head_bytes": n as u64, "truncated": size > n as u64 },
                "content": content
            })).into_response();
        }
        _ => super::ApiError::bad_request("unsupported ptr kind").into_response(),
    }
}
