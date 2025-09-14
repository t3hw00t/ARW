use super::{default_model, hints, ok};
use crate::AppState;
use arw_macros::arw_admin;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
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
    pub s_inst: Option<usize>,   // instructions
    pub s_plan: Option<usize>,   // plan
    pub s_policy: Option<usize>, // safety/policy
    pub s_evid: Option<usize>,   // evidence
    pub s_nice: Option<usize>,   // nice-to-have
    // Optional per-lane caps for recents (tokens)
    pub s_intents: Option<usize>,
    pub s_actions: Option<usize>,
    pub s_files: Option<usize>,
    // Optional total prompt budget (tokens), informational
    pub s_total: Option<usize>,
    // Per-call context formatting/budget overrides (non-persistent)
    #[serde(default)]
    pub context_format: Option<String>,
    #[serde(default)]
    pub include_provenance: Option<bool>,
    #[serde(default)]
    pub context_item_template: Option<String>,
    #[serde(default)]
    pub context_header: Option<String>,
    #[serde(default)]
    pub context_footer: Option<String>,
    #[serde(default)]
    pub joiner: Option<String>,
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
}

#[arw_admin(
    method = "GET",
    path = "/admin/context/assemble",
    summary = "Assemble minimal context: top‑K beliefs + policy/model hints"
)]
pub async fn assemble_get(
    State(state): State<AppState>,
    Query(q): Query<AssembleQs>,
) -> impl IntoResponse {
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
    // Apply optional overrides from policy hints (retrieval K/diversity; MMR lambda)
    let k_default = q.k.or(policy_hints.retrieval_k).unwrap_or(mode_k);
    let evid_k = q.evidence_k.unwrap_or(k_default);
    let mmr_lambda = q
        .div
        .or(policy_hints.mmr_lambda)
        .or(policy_hints.retrieval_div)
        .or(mode_div);
    // Estimate pool size for coverage metrics (top-50 as proxy)
    let pool: Vec<serde_json::Value> =
        super::world::select_top_claims(proj_opt, q.q.as_deref().unwrap_or(""), 50).await;
    // Beliefs: use diversity-aware selection when requested or defaulted by mode
    let items_initial = if let Some(lambda) = mmr_lambda {
        super::world::select_top_claims_diverse(
            proj_opt,
            q.q.as_deref().unwrap_or(""),
            evid_k,
            lambda,
        )
        .await
    } else {
        super::world::select_top_claims(proj_opt, q.q.as_deref().unwrap_or(""), evid_k).await
    };
    // Evidence slot budget: approximate tokens and cap selection
    fn est_tokens_value(v: &serde_json::Value) -> u64 {
        match v {
            serde_json::Value::String(s) => (s.len() as u64).div_ceil(4), // ~4 chars/token
            serde_json::Value::Number(_) => 1,
            serde_json::Value::Bool(_) => 1,
            serde_json::Value::Array(a) => a.iter().map(est_tokens_value).sum(),
            serde_json::Value::Object(o) => o.values().map(est_tokens_value).sum(),
            _ => 0,
        }
    }
    fn est_tokens_belief(v: &serde_json::Value) -> u64 {
        let mut t = 6; // overhead
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            t += (id.len() as u64).div_ceil(4);
        }
        if let Some(props) = v.get("props") {
            t += est_tokens_value(props);
        }
        if let Some(name) = v.get("name") {
            t += est_tokens_value(name);
        }
        if let Some(text) = v.get("text") {
            t += est_tokens_value(text);
        }
        if let Some(trace) = v.get("trace") {
            t += est_tokens_value(trace);
        }
        t.min(512) // cap per item
    }
    let mut used_tokens: u64 = 0;
    let budget_tokens: Option<u64> = q.s_evid.map(|n| n as u64).filter(|n| *n > 0);
    let mut items: Vec<serde_json::Value> = Vec::new();
    for it in items_initial.into_iter() {
        let est = est_tokens_belief(&it);
        if let Some(b) = budget_tokens {
            if used_tokens.saturating_add(est) > b {
                break;
            }
        }
        used_tokens = used_tokens.saturating_add(est);
        items.push(it);
    }
    // Optional compression pass (hints.compression_aggr in 0..1): shorten verbose fields
    if let Some(aggr) = policy_hints.compression_aggr {
        let a = aggr.clamp(0.0, 1.0);
        if a > 0.0 {
            fn trim_field(obj: &mut serde_json::Map<String, serde_json::Value>, key: &str, a: f64) {
                if let Some(serde_json::Value::String(s)) = obj.get_mut(key) {
                    let len = s.len();
                    if len > 0 {
                        // Keep fraction (1 - 0.6*a): at a=1 keep ~40%
                        let keep = ((len as f64) * (1.0 - 0.6 * a)).max(32.0) as usize;
                        if len > keep {
                            let mut t = s.chars().take(keep).collect::<String>();
                            t.push('…');
                            *s = t;
                        }
                    }
                }
            }
            for v in items.iter_mut() {
                if let Some(o) = v.as_object_mut() {
                    trim_field(o, "text", a);
                    trim_field(o, "trace", a);
                    if let Some(serde_json::Value::Object(props)) = o.get_mut("props") {
                        for k in ["note", "summary", "details", "desc"] {
                            trim_field(props, k, a);
                        }
                    }
                }
            }
            // Recompute evidence tokens after compression
            used_tokens = 0;
            for it in items.iter() {
                used_tokens = used_tokens.saturating_add(est_tokens_belief(it));
            }
        }
    }
    // Strict budget pack (parity with evaluator) when hints or per-call overrides specify a total token budget
    // pre-pack token snapshot (unused)
    let mut aux_pack_before_tokens: Option<u64> = None;
    let mut aux_pack_after_tokens: Option<u64> = None;
    let mut aux_items_before: Option<usize> = None;
    let mut aux_items_after: Option<usize> = None;
    let mut aux_per_item_cap: Option<u64> = None;
    fn estimate_tokens_item(v: &serde_json::Value) -> u64 {
        let mut t = 6;
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            t += (id.len() as u64).div_ceil(4);
        }
        if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
            t += (s.len() as u64).div_ceil(4);
        }
        if let Some(s) = v.get("trace").and_then(|x| x.as_str()) {
            t += (s.len() as u64).div_ceil(4);
        }
        if let Some(props) = v.get("props").and_then(|x| x.as_object()) {
            for (_k, vv) in props.iter() {
                if let Some(s) = vv.as_str() {
                    t += (s.len() as u64).div_ceil(4);
                }
            }
        }
        t
    }
    fn trim_value_string_to_tokens(val: &mut String, max_tokens: u64) {
        let max_chars = (max_tokens * 4).max(24) as usize;
        if val.len() > max_chars {
            let mut t = val.chars().take(max_chars).collect::<String>();
            t.push('…');
            *val = t;
        }
    }
    fn trim_item_to_tokens(v: &mut serde_json::Value, cap_tokens: u64) {
        if let Some(o) = v.as_object_mut() {
            if let Some(serde_json::Value::String(s)) = o.get_mut("text") {
                trim_value_string_to_tokens(s, cap_tokens);
            }
            if let Some(serde_json::Value::String(s)) = o.get_mut("trace") {
                trim_value_string_to_tokens(s, cap_tokens / 2);
            }
            if let Some(serde_json::Value::Object(props)) = o.get_mut("props") {
                for k in ["summary", "note", "details", "desc"] {
                    if let Some(serde_json::Value::String(s)) = props.get_mut(k) {
                        trim_value_string_to_tokens(s, cap_tokens / 4);
                    }
                }
            }
        }
    }
    let eff_budget_tokens = q
        .context_budget_tokens
        .or(policy_hints.context_budget_tokens)
        .unwrap_or(0);
    if eff_budget_tokens > 0 {
        let total_budget = eff_budget_tokens;
        let mut toks: Vec<u64> = items.iter().map(estimate_tokens_item).collect();
        let mut total: i64 = toks.iter().sum::<u64>() as i64;
        let budget = total_budget as i64;
        aux_pack_before_tokens = Some(total as u64);
        aux_items_before = Some(items.len());
        if total > budget {
            let per_cap = q
                .context_item_budget_tokens
                .or(policy_hints.context_item_budget_tokens)
                .map(|x| x as u64)
                .unwrap_or_else(|| (total_budget as u64 / (items.len().max(1) as u64)).max(24));
            aux_per_item_cap = Some(per_cap);
            for (i, it) in items.iter_mut().enumerate() {
                if toks[i] > per_cap {
                    trim_item_to_tokens(it, per_cap);
                    toks[i] = estimate_tokens_item(it);
                }
            }
            total = toks.iter().sum::<u64>() as i64;
            while total > budget && !items.is_empty() {
                items.pop();
                toks.pop();
                total = toks.iter().sum::<u64>() as i64;
            }
        }
        used_tokens = toks.iter().sum::<u64>();
        aux_pack_after_tokens = Some(used_tokens);
        aux_items_after = Some(items.len());
    }
    // Include minimal policy + model context (policy_hints already loaded)
    let model_default = { default_model().read().await.clone() };
    let proj = proj_opt.map(|s| s.to_string());
    let notes_path = proj
        .as_deref()
        .and_then(super::paths::project_notes_path)
        .map(|p| p.to_string_lossy().to_string());
    // Recent files (from world model entities)
    let mut files = super::world::select_recent_files(proj_opt, 20).await;
    // Include recent intents/actions (size-bounded), optionally filtered by proj when present
    let mut intents = super::state_api::intents_snapshot().await;
    let mut actions = super::state_api::actions_snapshot().await;
    if let Some(p) = proj_opt {
        let pv = serde_json::Value::String(p.to_string());
        intents.retain(|it| {
            it.get("payload")
                .and_then(|v| v.get("proj"))
                .unwrap_or(&serde_json::Value::Null)
                == &pv
        });
        actions.retain(|it| {
            it.get("payload")
                .and_then(|v| v.get("proj"))
                .unwrap_or(&serde_json::Value::Null)
                == &pv
        });
    }
    // Keep most recent 20 each
    if intents.len() > 20 {
        intents = intents[intents.len() - 20..].to_vec();
    }
    if actions.len() > 20 {
        actions = actions[actions.len() - 20..].to_vec();
    }
    // Estimate tokens for recents and apply optional per-lane caps before attaching ptrs
    fn est_tokens_event(ev: &serde_json::Value) -> u64 {
        let mut t = 4;
        if let Some(k) = ev.get("kind") {
            t += est_tokens_value(k);
        }
        if let Some(p) = ev.get("payload") {
            t += est_tokens_value(p);
        }
        if let Some(ts) = ev.get("time") {
            t += est_tokens_value(ts);
        }
        t.min(1024)
    }
    fn est_tokens_file(f: &serde_json::Value) -> u64 {
        let mut t = 2;
        if let Some(p) = f.get("path") {
            t += est_tokens_value(p);
        }
        if let Some(id) = f.get("id") {
            t += est_tokens_value(id);
        }
        t.min(256)
    }
    fn cap_by_tokens(
        mut items: Vec<serde_json::Value>,
        budget: Option<u64>,
        est: fn(&serde_json::Value) -> u64,
    ) -> (Vec<serde_json::Value>, u64) {
        if let Some(b) = budget {
            if b == 0 {
                return (Vec::new(), 0);
            }
        }
        let Some(b) = budget else {
            let used = items.iter().map(est).sum();
            return (items, used);
        };
        let mut out: Vec<serde_json::Value> = Vec::with_capacity(items.len());
        let mut used: u64 = 0;
        for it in items.drain(..) {
            let t = est(&it);
            if used.saturating_add(t) > b {
                break;
            }
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
    fn with_ptrs_beliefs(
        mut v: Vec<serde_json::Value>,
        proj: Option<&str>,
    ) -> Vec<serde_json::Value> {
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
                it["ptr"] =
                    json!({"kind": "episode", "corr_id": c, "source": "/admin/state/episodes"});
            } else if let (Some(t), Some(k)) = (it.get("time"), it.get("kind")) {
                it["ptr"] = json!({"kind": "event", "time": t, "code": k});
            }
        }
        v
    }
    let beliefs = with_ptrs_beliefs(items.clone(), proj_opt);
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
            "diversity": mmr_lambda,
            "counts": {"beliefs": coverage_selected, "files": files.len(), "intents": intents.len(), "actions": actions.len()},
            "coverage": {"pool": coverage_pool, "omitted": coverage_omitted, "recall_risk": coverage_omitted > 0},
            "usage": {"evidence_tokens": used_tokens, "evidence_budget": budget_tokens, "recent_intents_tokens": intents_tokens, "recent_actions_tokens": actions_tokens, "recent_files_tokens": files_tokens, "total_tokens": total_tokens_used},
            "pack": {
                "budget_tokens": if eff_budget_tokens>0 { Some(eff_budget_tokens) } else { None::<usize> },
                "before_tokens": aux_pack_before_tokens,
                "after_tokens": aux_pack_after_tokens,
                "items_before": aux_items_before,
                "items_after": aux_items_after,
                "items_dropped": match (aux_items_before, aux_items_after) { (Some(a), Some(b)) => Some(a.saturating_sub(b) as u64), _ => None },
                "per_item_cap_tokens": aux_per_item_cap,
            },
        });
        crate::ext::corr::ensure_corr(&mut ev);
        state
            .bus
            .publish(crate::ext::topics::TOPIC_CONTEXT_ASSEMBLED, &ev);
        if coverage_omitted > 0 {
            let mut ev2 = json!({
                "proj": ev["proj"].clone(),
                "query": ev["query"].clone(),
                "pool": coverage_pool,
                "omitted": coverage_omitted,
            });
            crate::ext::corr::ensure_corr(&mut ev2);
            state
                .bus
                .publish(crate::ext::topics::TOPIC_CONTEXT_COVERAGE, &ev2);
        }
    }
    // Render context preview string (format driven by hints)
    fn render_context(items: &[serde_json::Value], hints: &super::Hints) -> String {
        let fmt = hints
            .context_format
            .as_deref()
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "bullets".to_string());
        let include_prov = hints.include_provenance.unwrap_or(false);
        let joiner = hints.joiner.as_deref().unwrap_or("\n");
        match fmt.as_str() {
            "jsonl" => {
                let lines: Vec<String> = items
                    .iter()
                    .map(|v| {
                        let mut o = serde_json::Map::new();
                        if let Some(id) = v.get("id") {
                            o.insert("id".into(), id.clone());
                        }
                        if let Some(t) = v.get("text") {
                            o.insert("text".into(), t.clone());
                        }
                        if let Some(props) = v.get("props") {
                            if include_prov {
                                o.insert("props".into(), props.clone());
                            }
                        }
                        if let Some(c) = v.get("confidence") {
                            o.insert("confidence".into(), c.clone());
                        }
                        serde_json::to_string(&serde_json::Value::Object(o)).unwrap_or_default()
                    })
                    .collect();
                lines.join(joiner)
            }
            "inline" => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|v| {
                        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                        let txt = v
                            .get("text")
                            .and_then(|x| x.as_str())
                            .or_else(|| {
                                v.get("props")
                                    .and_then(|p| p.get("summary"))
                                    .and_then(|x| x.as_str())
                            })
                            .unwrap_or("");
                        format!("[{}] {}", id, txt)
                    })
                    .collect();
                parts.join(" · ")
            }
            "custom" => {
                let tpl = hints
                    .context_item_template
                    .as_deref()
                    .unwrap_or("- [{{id}}] {{text}}");
                let mut out: Vec<String> = Vec::with_capacity(items.len());
                for v in items.iter() {
                    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let txt = v
                        .get("text")
                        .and_then(|x| x.as_str())
                        .or_else(|| {
                            v.get("props")
                                .and_then(|p| p.get("summary"))
                                .and_then(|x| x.as_str())
                        })
                        .unwrap_or("");
                    let conf = v
                        .get("confidence")
                        .and_then(|x| x.as_f64())
                        .map(|c| format!("{:.2}", c))
                        .unwrap_or_default();
                    let mut line = tpl
                        .replace("{{id}}", id)
                        .replace("{{text}}", txt)
                        .replace("{{summary}}", txt)
                        .replace("{{confidence}}", &conf);
                    if include_prov {
                        if let Some(props) = v.get("props").and_then(|p| p.as_object()) {
                            if let Some(serde_json::Value::String(pv)) =
                                props.get("provenance").cloned()
                            {
                                line = line.replace("{{provenance}}", &pv);
                            }
                        }
                    }
                    out.push(line);
                }
                out.join(joiner)
            }
            _ => {
                let lines: Vec<String> = items
                    .iter()
                    .map(|v| {
                        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                        let txt = v
                            .get("text")
                            .and_then(|x| x.as_str())
                            .or_else(|| {
                                v.get("props")
                                    .and_then(|p| p.get("summary"))
                                    .and_then(|x| x.as_str())
                            })
                            .unwrap_or("");
                        format!("- [{}] {}", id, txt)
                    })
                    .collect();
                lines.join(joiner)
            }
        }
    }
    let preview_context = if items.is_empty() {
        String::new()
    } else {
        let body = render_context(&items, &policy_hints);
        let header = policy_hints
            .context_header
            .clone()
            .unwrap_or_else(|| "You may use the following context.".to_string());
        let footer = policy_hints.context_footer.clone().unwrap_or_default();
        if footer.is_empty() {
            format!("{}\n{}", header, body)
        } else {
            format!("{}\n{}\n{}", header, body, footer)
        }
    };

    // Planner meta (mode-driven suggestions for downstream agents)
    let planner_meta = json!({
        "mode": mode_s,
        "verify_pass": verify_pass,
        "consistency": { "vote_k": vote_k },
        "retrieval": { "k": k_default, "evidence_k": evid_k, "lambda": mmr_lambda, "hints": {"k": policy_hints.retrieval_k, "div": policy_hints.retrieval_div, "lambda": policy_hints.mmr_lambda} },
        "compression": { "aggr": policy_hints.compression_aggr }
    });
    ok(json!({
        "beliefs": beliefs,
        "recent": { "intents": intents, "actions": actions, "files": files },
        "policy": { "hints": policy_hints, "planner": planner_meta },
        "model": { "default": model_default },
        "project": { "name": proj, "notes": notes_path },
        "budget": { "slots": { "instructions": q.s_inst, "plan": q.s_plan, "policy": q.s_policy, "evidence": q.s_evid, "nice": q.s_nice, "intents": q.s_intents, "actions": q.s_actions, "files": q.s_files, "total": q.s_total },
                     "requested": { "k": k_default, "evidence_k": evid_k, "diversity": mmr_lambda },
                     "usage": { "evidence_tokens": used_tokens, "recent_intents_tokens": intents_tokens, "recent_actions_tokens": actions_tokens, "recent_files_tokens": files_tokens, "total_tokens": used_tokens + intents_tokens + actions_tokens + files_tokens } },
        "coverage": { "pool": coverage_pool, "selected": coverage_selected, "omitted": coverage_omitted, "recall_risk": coverage_omitted > 0, "evidence_tokens_used": used_tokens, "evidence_tokens_budget": budget_tokens },
        "params": { "proj": q.proj, "q": q.q, "k": k_default, "evidence_k": evid_k, "div": mmr_lambda, "s_inst": q.s_inst, "s_plan": q.s_plan, "s_policy": q.s_policy, "s_evid": q.s_evid, "s_nice": q.s_nice },
        "context_preview": preview_context,
        "aux": {
            "context": {
                "budget_tokens": if eff_budget_tokens>0 { Some(eff_budget_tokens) } else { None::<usize> },
                "before_tokens": aux_pack_before_tokens,
                "after_tokens": aux_pack_after_tokens,
                "items_before": aux_items_before,
                "items_after": aux_items_after,
                "items_dropped": match (aux_items_before, aux_items_after) { (Some(a), Some(b)) => Some(a.saturating_sub(b) as u64), _ => None },
                "per_item_cap_tokens": aux_per_item_cap,
                "compression_aggr": policy_hints.compression_aggr,
                "retrieval": { "k": evid_k, "lambda": mmr_lambda }
            },
            "recents": { "tokens": intents_tokens + actions_tokens + files_tokens }
        }
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
pub async fn rehydrate_post(
    State(state): State<AppState>,
    Json(req): Json<RehydrateReq>,
) -> impl IntoResponse {
    use tokio::fs as afs;
    let kind = req.ptr.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "world_belief" => {
            let id = match req.ptr.get("id").and_then(|v| v.as_str()) {
                Some(s) => s,
                None => return super::ApiError::bad_request("missing id").into_response(),
            };
            let proj = req.ptr.get("proj").and_then(|v| v.as_str());
            if let Some(node) = super::world::get_belief_node(proj, id) {
                let v =
                    serde_json::to_value(&node).unwrap_or_else(|_| serde_json::json!({"id": id}));
                return ok(serde_json::json!({"ptr": req.ptr, "belief": v})).into_response();
            }
            super::ApiError::not_found("belief not found").into_response()
        }
        "episode" => {
            let cid = match req.ptr.get("corr_id").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => return super::ApiError::bad_request("missing corr_id").into_response(),
            };
            // Delegate to episode snapshot endpoint
            super::state_api::episode_snapshot_get(State(state), Path(cid))
                .await
                .into_response()
        }
        "file" => {
            // Gate file rehydrate via policy key; deny by default
            if !arw_core::gating::allowed("context:rehydrate:file") {
                return super::ApiError::forbidden("gated").into_response();
            }
            let path = match req.ptr.get("path").and_then(|v| v.as_str()) {
                Some(s) => std::path::PathBuf::from(s),
                None => return super::ApiError::bad_request("missing path").into_response(),
            };
            // Best-effort safeguard: limit to reasonably-sized files and return head only
            let meta = match afs::metadata(&path).await {
                Ok(m) => m,
                Err(_) => return super::ApiError::not_found("file not found").into_response(),
            };
            if !meta.is_file() {
                return super::ApiError::bad_request("not a file").into_response();
            }
            let size = meta.len();
            let cap: u64 = std::env::var("ARW_REHYDRATE_FILE_HEAD_KB")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(64)
                * 1024;
            let take = size.min(cap);
            let mut f = match afs::File::open(&path).await {
                Ok(f) => f,
                Err(_) => return super::ApiError::not_found("file open failed").into_response(),
            };
            let mut buf = vec![0u8; take as usize];
            use tokio::io::AsyncReadExt;
            let n: usize = f.read(&mut buf).await.unwrap_or_default();
            let content = String::from_utf8_lossy(&buf[..n]).to_string();
            ok(serde_json::json!({
                "ptr": req.ptr,
                "file": {"path": path.to_string_lossy(), "size": size, "head_bytes": n as u64, "truncated": size > n as u64 },
                "content": content
            })).into_response()
        }
        _ => super::ApiError::bad_request("unsupported ptr kind").into_response(),
    }
}
