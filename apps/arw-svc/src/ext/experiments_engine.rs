use crate::AppState;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::RwLock;

// Minimal in-process registry for experiments and a helper to run A/B on goldens

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct VariantCfg {
    #[serde(default)]
    pub vote_k: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub retrieval_div: Option<f64>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
    // Strict budget (optional)
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
    // Context formatting
    #[serde(default)]
    pub context_format: Option<String>, // bullets|jsonl|inline|custom
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
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub variants: HashMap<String, VariantCfg>,
}

static REG: OnceCell<RwLock<HashMap<String, Experiment>>> = OnceCell::new();
fn reg() -> &'static RwLock<HashMap<String, Experiment>> {
    REG.get_or_init(|| RwLock::new(HashMap::new()))
}

pub async fn put(exp: Experiment) {
    reg().write().await.insert(exp.id.clone(), exp);
}

pub async fn list() -> Vec<Experiment> {
    reg().read().await.values().cloned().collect()
}

// ---- Persisted winners (last known) ----

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WinnerInfo {
    pub exp_id: String,
    pub proj: String,
    pub variant: String,
    pub time: String,
    pub passed: usize,
    pub total: usize,
    pub failed: usize,
    pub avg_latency_ms: u64,
    #[serde(default)]
    pub avg_ctx_tokens: u64,
    #[serde(default)]
    pub avg_ctx_items: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct WinnersState {
    #[serde(default)]
    winners: Vec<WinnerInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ScoreEntry {
    pub passed: usize,
    pub total: usize,
    pub failed: usize,
    pub avg_latency_ms: u64,
    #[serde(default)]
    pub avg_ctx_tokens: u64,
    #[serde(default)]
    pub avg_ctx_items: u64,
    #[serde(default)]
    pub time: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ScoreRow {
    pub exp_id: String,
    pub proj: String,
    pub variant: String,
    pub score: ScoreEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct ExpsState {
    #[serde(default)]
    winners: Vec<WinnerInfo>,
    #[serde(default)]
    scoreboard: Vec<ScoreRow>,
}

static WINNERS: OnceCell<RwLock<HashMap<String, WinnerInfo>>> = OnceCell::new();
fn winners_map() -> &'static RwLock<HashMap<String, WinnerInfo>> {
    WINNERS.get_or_init(|| RwLock::new(HashMap::new()))
}

static SCOREBOARD: OnceCell<RwLock<HashMap<String, HashMap<String, ScoreEntry>>>> = OnceCell::new();
fn scoreboard_map() -> &'static RwLock<HashMap<String, HashMap<String, ScoreEntry>>> {
    SCOREBOARD.get_or_init(|| RwLock::new(HashMap::new()))
}

fn winners_path() -> std::path::PathBuf {
    crate::ext::paths::state_dir().join("experiments_state.json")
}

pub async fn load_persisted() {
    if let Some(v) = crate::ext::io::load_json_file_async(&winners_path()).await {
        if let Ok(es) = serde_json::from_value::<ExpsState>(v.clone()) {
            // winners
            {
                let mut m = winners_map().write().await;
                m.clear();
                for w in es.winners.into_iter() {
                    m.insert(w.exp_id.clone(), w);
                }
            }
            // scoreboard
            {
                let mut sm = scoreboard_map().write().await;
                sm.clear();
                for row in es.scoreboard.into_iter() {
                    let ent = sm.entry(row.exp_id.clone()).or_insert_with(HashMap::new);
                    ent.insert(row.variant.clone(), row.score);
                }
            }
            return;
        }
        if let Ok(ws) = serde_json::from_value::<WinnersState>(v) {
            let mut m = winners_map().write().await;
            m.clear();
            for w in ws.winners.into_iter() {
                m.insert(w.exp_id.clone(), w);
            }
        }
    }
}

async fn persist_winners() {
    let winners: Vec<WinnerInfo> = winners_map().read().await.values().cloned().collect();
    let mut scoreboard: Vec<ScoreRow> = Vec::new();
    {
        let sm = scoreboard_map().read().await;
        for (exp_id, vmap) in sm.iter() {
            for (variant, score) in vmap.iter() {
                scoreboard.push(ScoreRow {
                    exp_id: exp_id.clone(),
                    proj: String::new(),
                    variant: variant.clone(),
                    score: score.clone(),
                });
            }
        }
    }
    let es = ExpsState {
        winners,
        scoreboard,
    };
    let _ = crate::ext::io::save_json_file_async(
        &winners_path(),
        &serde_json::to_value(es).unwrap_or(json!({})),
    )
    .await;
}

pub async fn list_winners() -> Vec<WinnerInfo> {
    winners_map().read().await.values().cloned().collect()
}

pub async fn list_scoreboard() -> Vec<ScoreRow> {
    let mut out: Vec<ScoreRow> = Vec::new();
    let sm = scoreboard_map().read().await;
    for (exp_id, vmap) in sm.iter() {
        for (variant, score) in vmap.iter() {
            out.push(ScoreRow {
                exp_id: exp_id.clone(),
                proj: String::new(),
                variant: variant.clone(),
                score: score.clone(),
            });
        }
    }
    out
}

async fn set_winner(info: WinnerInfo) {
    {
        let mut m = winners_map().write().await;
        m.insert(info.exp_id.clone(), info);
    }
    persist_winners().await;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunPlan {
    pub proj: String,
    pub exp_id: String,
    pub variants: Vec<String>,
    #[serde(default)]
    pub budget_total_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunOutcomeVariant {
    pub variant: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub avg_latency_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunOutcome {
    pub exp_id: String,
    pub proj: String,
    pub results: Vec<RunOutcomeVariant>,
    pub winner: Option<String>,
}

pub async fn run_ab_on_goldens(state: &AppState, plan: RunPlan) -> RunOutcome {
    let set = crate::ext::goldens::load(&plan.proj).await;
    // Copy the exp if present so we can read variant params
    let exp_opt = { reg().read().await.get(&plan.exp_id).cloned() };
    let mut results: Vec<RunOutcomeVariant> = Vec::new();
    for v in plan.variants.iter() {
        let cfg = exp_opt
            .as_ref()
            .and_then(|e| e.variants.get(v))
            .cloned()
            .unwrap_or_default();
        let opts = crate::ext::goldens::EvalOptions {
            limit: None,
            temperature: cfg.temperature,
            vote_k: cfg.vote_k,
            retrieval_k: cfg.retrieval_k,
            mmr_lambda: cfg.mmr_lambda.or(cfg.retrieval_div),
            compression_aggr: cfg.compression_aggr,
            context_budget_tokens: cfg.context_budget_tokens,
            context_item_budget_tokens: cfg.context_item_budget_tokens,
            context_format: cfg.context_format.clone(),
            include_provenance: cfg.include_provenance,
            context_item_template: cfg.context_item_template.clone(),
            context_header: cfg.context_header.clone(),
            context_footer: cfg.context_footer.clone(),
            joiner: cfg.joiner.clone(),
        };
        let summary =
            crate::ext::goldens::evaluate_chat_items(&set, &opts, Some(plan.proj.as_str())).await;
        results.push(RunOutcomeVariant {
            variant: v.clone(),
            total: summary.total,
            passed: summary.passed,
            failed: summary.failed,
            avg_latency_ms: summary.avg_latency_ms,
        });
        // Update persisted scoreboard (last-run snapshot per variant)
        {
            let mut sm = scoreboard_map().write().await;
            let ent = sm.entry(plan.exp_id.clone()).or_insert_with(HashMap::new);
            ent.insert(
                v.clone(),
                ScoreEntry {
                    passed: summary.passed,
                    total: summary.total,
                    failed: summary.failed,
                    avg_latency_ms: summary.avg_latency_ms,
                    avg_ctx_tokens: summary.avg_ctx_tokens,
                    avg_ctx_items: summary.avg_ctx_items,
                    time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                },
            );
        }
        // Emit per-variant result event
        let mut payload = json!({
            "exp_id": plan.exp_id,
            "proj": plan.proj,
            "variant": v,
            "passed": summary.passed,
            "failed": summary.failed,
            "total": summary.total,
            "avg_latency_ms": summary.avg_latency_ms,
            "avg_ctx_tokens": summary.avg_ctx_tokens,
            "avg_ctx_items": summary.avg_ctx_items,
            "knobs": {
                "retrieval_k": cfg.retrieval_k,
                "mmr_lambda": cfg.mmr_lambda.or(cfg.retrieval_div),
                "vote_k": cfg.vote_k,
                "compression_aggr": cfg.compression_aggr,
                "context_budget_tokens": cfg.context_budget_tokens,
                "context_item_budget_tokens": cfg.context_item_budget_tokens,
                "context_format": cfg.context_format,
                "include_provenance": cfg.include_provenance,
            }
        });
        crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Experiment.Result", &payload);
        // Optional budget guard (simple): break if avg latency * total exceeds budget_total_ms
        if let Some(b) = plan.budget_total_ms {
            let est = (summary.avg_latency_ms as u64) * (summary.total as u64);
            if est > b {
                break;
            }
        }
    }
    // Choose winner by highest passed, tie-breaker lower avg_latency_ms
    let winner = results
        .iter()
        .max_by(|a, b| match a.passed.cmp(&b.passed) {
            std::cmp::Ordering::Equal => b.avg_latency_ms.cmp(&a.avg_latency_ms),
            other => other,
        })
        .map(|r| r.variant.clone());

    if let Some(w) = &winner {
        // Persist winner summary
        if let Some(win_var) = results.iter().find(|r| &r.variant == w) {
            let info = WinnerInfo {
                exp_id: plan.exp_id.clone(),
                proj: plan.proj.clone(),
                variant: w.clone(),
                time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                passed: win_var.passed,
                total: win_var.total,
                failed: win_var.failed,
                avg_latency_ms: win_var.avg_latency_ms,
                avg_ctx_tokens: 0,
                avg_ctx_items: 0,
            };
            set_winner(info).await;
        }
        let mut payload = json!({"exp_id": plan.exp_id, "proj": plan.proj, "winner": w});
        crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Experiment.Winner", &payload);
    }
    RunOutcome {
        exp_id: plan.exp_id,
        proj: plan.proj,
        results,
        winner,
    }
}

// Lightweight API helpers
#[derive(Deserialize)]
pub struct StartReq {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub variants: HashMap<String, VariantCfg>,
}

pub async fn start(_state: &AppState, req: StartReq) -> Value {
    let exp = Experiment {
        id: req.id.clone(),
        name: req.name,
        variants: req.variants,
    };
    put(exp).await;
    json!({"ok": true, "id": req.id})
}

pub async fn get_variant(exp_id: &str, variant: &str) -> Option<VariantCfg> {
    reg()
        .read()
        .await
        .get(exp_id)
        .and_then(|e| e.variants.get(variant))
        .cloned()
}
