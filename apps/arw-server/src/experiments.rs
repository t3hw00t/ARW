use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use arw_events::Bus;
use arw_topics as topics;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use utoipa::ToSchema;

use crate::{feedback::save_bytes_atomic, goldens, governor::GovernorState, responses, util};
use uuid::Uuid;

const EVENTS_CAP: usize = 128;

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
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
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
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
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub variants: HashMap<String, VariantCfg>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
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

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
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

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
pub struct ScoreRow {
    pub exp_id: String,
    pub proj: String,
    pub variant: String,
    pub score: ScoreEntry,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct PersistedState {
    #[serde(default)]
    winners: Vec<WinnerInfo>,
    #[serde(default)]
    scoreboard: Vec<ScoreRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, ToSchema)]
pub struct RunPlan {
    pub proj: String,
    pub exp_id: String,
    pub variants: Vec<String>,
    #[serde(default)]
    pub budget_total_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RunOutcomeVariant {
    pub variant: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub avg_latency_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct RunOutcome {
    pub exp_id: String,
    pub proj: String,
    pub results: Vec<RunOutcomeVariant>,
    pub winner: Option<String>,
}

pub struct Experiments {
    experiments: RwLock<HashMap<String, Experiment>>,
    winners: RwLock<HashMap<String, WinnerInfo>>,
    scoreboard: RwLock<HashMap<String, HashMap<String, ScoreEntry>>>,
    events: RwLock<VecDeque<Value>>,
    bus: Bus,
    governor: Arc<GovernorState>,
    state_path: PathBuf,
}

impl Experiments {
    pub async fn new(bus: Bus, governor: Arc<GovernorState>) -> Arc<Self> {
        let state_path = util::state_dir().join("experiments_state.json");
        let this = Arc::new(Self {
            experiments: RwLock::new(HashMap::new()),
            winners: RwLock::new(HashMap::new()),
            scoreboard: RwLock::new(HashMap::new()),
            events: RwLock::new(VecDeque::with_capacity(EVENTS_CAP)),
            bus,
            governor,
            state_path,
        });
        this.load_persisted().await;
        this
    }

    pub async fn define(&self, exp: Experiment) {
        self.experiments.write().await.insert(exp.id.clone(), exp);
    }

    pub async fn list(&self) -> Vec<Experiment> {
        self.experiments.read().await.values().cloned().collect()
    }

    pub async fn run_on_goldens(&self, plan: RunPlan) -> RunOutcome {
        let set = goldens::load(&plan.proj).await;
        let experiments = self.experiments.read().await;
        let exp_cfg = experiments.get(&plan.exp_id).cloned();
        drop(experiments);

        let mut results = Vec::new();
        for variant in plan.variants.iter() {
            let cfg = exp_cfg
                .as_ref()
                .and_then(|exp| exp.variants.get(variant))
                .cloned()
                .unwrap_or_default();
            let opts = goldens::EvalOptions {
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
            let summary = goldens::evaluate_chat_items(&set, &opts, Some(plan.proj.as_str())).await;
            results.push(RunOutcomeVariant {
                variant: variant.clone(),
                total: summary.total,
                passed: summary.passed,
                failed: summary.failed,
                avg_latency_ms: summary.avg_latency_ms,
            });
            self.update_scoreboard(&plan.exp_id, variant, &summary)
                .await;
            let mut payload = json!({
                "exp_id": plan.exp_id,
                "proj": plan.proj,
                "variant": variant,
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
            responses::attach_corr(&mut payload);
            self.bus.publish(topics::TOPIC_EXPERIMENT_RESULT, &payload);
            self.record_event(payload).await;
            if let Some(budget) = plan.budget_total_ms {
                let est = (summary.avg_latency_ms as u64) * (summary.total as u64);
                if est > budget {
                    break;
                }
            }
        }
        let winner = results
            .iter()
            .max_by(|a, b| match a.passed.cmp(&b.passed) {
                std::cmp::Ordering::Equal => b.avg_latency_ms.cmp(&a.avg_latency_ms),
                other => other,
            })
            .map(|row| row.variant.clone());
        if let Some(win) = &winner {
            if let Some(row) = results.iter().find(|r| &r.variant == win) {
                let info = WinnerInfo {
                    exp_id: plan.exp_id.clone(),
                    proj: plan.proj.clone(),
                    variant: win.clone(),
                    time: now_iso(),
                    passed: row.passed,
                    total: row.total,
                    failed: row.failed,
                    avg_latency_ms: row.avg_latency_ms,
                    avg_ctx_tokens: 0,
                    avg_ctx_items: 0,
                };
                self.set_winner(info).await;
            }
            let mut payload = json!({
                "exp_id": plan.exp_id,
                "proj": plan.proj,
                "winner": win,
            });
            responses::attach_corr(&mut payload);
            self.bus.publish(topics::TOPIC_EXPERIMENT_WINNER, &payload);
            self.record_event(payload).await;
        }
        RunOutcome {
            exp_id: plan.exp_id,
            proj: plan.proj,
            results,
            winner,
        }
    }

    pub async fn activate(&self, id: &str, variant: &str) -> Result<(), String> {
        let experiments = self.experiments.read().await;
        let cfg = experiments
            .get(id)
            .and_then(|exp| exp.variants.get(variant))
            .cloned()
            .ok_or_else(|| "unknown variant".to_string())?;
        drop(experiments);
        self.governor
            .apply_hints(
                &self.bus,
                None,
                None,
                None,
                None,
                None,
                cfg.retrieval_k,
                cfg.retrieval_div,
                cfg.mmr_lambda,
                cfg.compression_aggr,
                cfg.vote_k.map(|v| v.clamp(0, 8) as u8),
                cfg.context_budget_tokens,
                cfg.context_item_budget_tokens,
                cfg.context_format.clone(),
                cfg.include_provenance,
                cfg.context_item_template.clone(),
                cfg.context_header.clone(),
                cfg.context_footer.clone(),
                cfg.joiner.clone(),
            )
            .await;
        let mut payload = json!({
            "id": id,
            "variant": variant,
            "applied": cfg,
        });
        responses::attach_corr(&mut payload);
        self.bus
            .publish(topics::TOPIC_EXPERIMENT_ACTIVATED, &payload);
        self.record_event(payload).await;
        Ok(())
    }

    pub async fn list_winners(&self) -> Vec<WinnerInfo> {
        self.winners.read().await.values().cloned().collect()
    }

    pub async fn list_scoreboard(&self) -> Vec<ScoreRow> {
        let map = self.scoreboard.read().await;
        let mut out = Vec::new();
        for (exp_id, variants) in map.iter() {
            for (variant, score) in variants.iter() {
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

    pub async fn state_events(&self) -> Vec<Value> {
        self.events.read().await.iter().cloned().collect()
    }

    pub async fn publish_start(
        &self,
        name: String,
        variants: Vec<String>,
        assignment: Option<Value>,
        budgets: Option<Value>,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let mut payload = json!({
            "id": id,
            "name": name,
            "variants": variants,
            "assignment": assignment,
            "budgets": budgets,
        });
        responses::attach_corr(&mut payload);
        self.bus.publish(topics::TOPIC_EXPERIMENT_STARTED, &payload);
        self.record_event(payload).await;
        id
    }

    pub async fn publish_stop(&self, id: String) {
        let mut payload = json!({"id": id});
        responses::attach_corr(&mut payload);
        self.bus
            .publish(topics::TOPIC_EXPERIMENT_COMPLETED, &payload);
        self.record_event(payload).await;
    }

    pub async fn publish_assign(&self, id: String, variant: String, agent: Option<String>) {
        let mut payload = json!({"id": id, "variant": variant, "agent": agent});
        responses::attach_corr(&mut payload);
        self.bus
            .publish(topics::TOPIC_EXPERIMENT_VARIANT_CHOSEN, &payload);
        self.record_event(payload).await;
    }

    async fn record_event(&self, payload: Value) {
        let mut guard = self.events.write().await;
        if guard.len() == EVENTS_CAP {
            guard.pop_front();
        }
        guard.push_back(json!({"time": now_iso(), "event": payload}));
    }

    async fn update_scoreboard(&self, exp_id: &str, variant: &str, summary: &goldens::EvalSummary) {
        let mut map = self.scoreboard.write().await;
        let entry = map.entry(exp_id.to_string()).or_insert_with(HashMap::new);
        entry.insert(
            variant.to_string(),
            ScoreEntry {
                passed: summary.passed,
                total: summary.total,
                failed: summary.failed,
                avg_latency_ms: summary.avg_latency_ms,
                avg_ctx_tokens: summary.avg_ctx_tokens,
                avg_ctx_items: summary.avg_ctx_items,
                time: now_iso(),
            },
        );
        self.persist().await;
    }

    async fn set_winner(&self, info: WinnerInfo) {
        self.winners.write().await.insert(info.exp_id.clone(), info);
        self.persist().await;
    }

    async fn load_persisted(&self) {
        let path = &self.state_path;
        if let Ok(bytes) = tokio::fs::read(path).await {
            if let Ok(state) = serde_json::from_slice::<PersistedState>(&bytes) {
                {
                    let mut winners = self.winners.write().await;
                    winners.clear();
                    for w in state.winners.iter() {
                        winners.insert(w.exp_id.clone(), w.clone());
                    }
                }
                {
                    let mut score = self.scoreboard.write().await;
                    score.clear();
                    for row in state.scoreboard.iter() {
                        let entry = score.entry(row.exp_id.clone()).or_default();
                        entry.insert(row.variant.clone(), row.score.clone());
                    }
                }
            }
        }
    }

    async fn persist(&self) {
        let winners: Vec<WinnerInfo> = self.winners.read().await.values().cloned().collect();
        let mut scoreboard = Vec::new();
        {
            let map = self.scoreboard.read().await;
            for (exp_id, variants) in map.iter() {
                for (variant, score) in variants.iter() {
                    scoreboard.push(ScoreRow {
                        exp_id: exp_id.clone(),
                        proj: String::new(),
                        variant: variant.clone(),
                        score: score.clone(),
                    });
                }
            }
        }
        let payload = PersistedState {
            winners,
            scoreboard,
        };
        let _ = save_bytes_atomic(
            &self.state_path,
            &serde_json::to_vec_pretty(&payload).unwrap_or_else(|_| b"{}".to_vec()),
        )
        .await;
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
