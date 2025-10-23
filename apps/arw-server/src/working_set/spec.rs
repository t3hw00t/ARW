use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

#[derive(Clone, Debug)]
pub struct WorkingSetSpec {
    pub query: Option<String>,
    pub embed: Option<Vec<f32>>,
    pub lanes: Vec<String>,
    pub limit: usize,
    pub expand_per_seed: usize,
    pub diversity_lambda: f32,
    pub min_score: f32,
    pub project: Option<String>,
    pub persona_id: Option<String>,
    pub lane_bonus: f32,
    pub lane_priorities: BTreeMap<String, f32>,
    pub scorer: Option<String>,
    pub expand_query: bool,
    pub expand_query_top_k: usize,
    pub slot_budgets: BTreeMap<String, usize>,
}

impl WorkingSetSpec {
    pub fn normalize(&mut self) {
        self.lanes = self
            .lanes
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.lanes.sort();
        self.lanes.dedup();
        if self.lanes.is_empty() {
            self.lanes = default_lanes();
        }
        if self.limit == 0 {
            self.limit = default_limit();
        }
        self.limit = self.limit.clamp(1, 256);
        self.expand_per_seed = self.expand_per_seed.min(16);
        if !self.diversity_lambda.is_finite() {
            self.diversity_lambda = default_diversity_lambda();
        }
        self.diversity_lambda = self.diversity_lambda.clamp(0.0, 1.0);
        if !self.min_score.is_finite() {
            self.min_score = default_min_score();
        }
        self.min_score = self.min_score.clamp(0.0, 1.0);
        if let Some(id) = self.persona_id.as_mut() {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                self.persona_id = None;
            } else {
                *id = trimmed.to_string();
            }
        }
        if !self.lane_bonus.is_finite() {
            self.lane_bonus = default_lane_bonus();
        }
        self.lane_bonus = self.lane_bonus.clamp(0.0, 1.0);
        let scorer_name = self
            .scorer
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(default_scorer);
        self.scorer = Some(scorer_name);
        if self.expand_query_top_k == 0 {
            self.expand_query_top_k = default_expand_query_top_k();
        }
        self.expand_query_top_k = self.expand_query_top_k.clamp(1, 32);
        self.normalize_slot_budgets();
        self.normalize_lane_priorities();
    }

    pub fn scorer_label(&self) -> String {
        self.scorer.clone().unwrap_or_else(default_scorer)
    }

    pub fn snapshot(&self) -> Value {
        let mut snapshot = Map::new();
        snapshot.insert("query_provided".into(), json!(self.query.is_some()));
        snapshot.insert("lanes".into(), json!(self.lanes));
        snapshot.insert("limit".into(), json!(self.limit));
        snapshot.insert("expand_per_seed".into(), json!(self.expand_per_seed));
        snapshot.insert("diversity_lambda".into(), json!(self.diversity_lambda));
        snapshot.insert("min_score".into(), json!(self.min_score));
        snapshot.insert("project".into(), json!(self.project));
        snapshot.insert("persona".into(), json!(self.persona_id));
        snapshot.insert("lane_bonus".into(), json!(self.lane_bonus));
        if !self.lane_priorities.is_empty() {
            let mut lanes = Map::new();
            for (lane, weight) in self.lane_priorities.iter() {
                lanes.insert(lane.clone(), json!(weight));
            }
            snapshot.insert("lane_priorities".into(), Value::Object(lanes));
        }
        snapshot.insert("scorer".into(), json!(self.scorer));
        snapshot.insert("expand_query".into(), json!(self.expand_query));
        snapshot.insert("expand_query_top_k".into(), json!(self.expand_query_top_k));
        if !self.slot_budgets.is_empty() {
            let mut slots = Map::new();
            for (slot, limit) in self.slot_budgets.iter() {
                slots.insert(slot.clone(), json!(limit));
            }
            snapshot.insert("slot_budgets".into(), Value::Object(slots));
        }
        Value::Object(snapshot)
    }

    fn normalize_slot_budgets(&mut self) {
        if self.slot_budgets.is_empty() {
            self.slot_budgets = default_slot_budgets();
        }
        if self.slot_budgets.is_empty() {
            return;
        }
        let mut normalized = BTreeMap::new();
        let limit_cap = self.limit.max(1);
        for (slot, value) in std::mem::take(&mut self.slot_budgets) {
            let slot = slot.trim().to_ascii_lowercase();
            if slot.is_empty() {
                continue;
            }
            let capped = value.min(limit_cap);
            if capped == 0 {
                continue;
            }
            normalized.insert(slot, capped);
        }
        self.slot_budgets = normalized;
    }

    fn normalize_lane_priorities(&mut self) {
        if self.lane_priorities.is_empty() {
            self.lane_priorities = default_lane_priorities();
        }
        if self.lane_priorities.is_empty() {
            return;
        }
        let mut normalized = BTreeMap::new();
        for (lane, value) in std::mem::take(&mut self.lane_priorities) {
            let key = lane.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            if !value.is_finite() {
                continue;
            }
            let clamped = value.clamp(-1.0, 1.0);
            if clamped.abs() < f32::EPSILON {
                continue;
            }
            normalized.insert(key, clamped);
        }
        self.lane_priorities = normalized;
    }

    pub fn slot_limit(&self, slot: &str) -> Option<usize> {
        if self.slot_budgets.is_empty() {
            return None;
        }
        let key = slot.trim().to_ascii_lowercase();
        self.slot_budgets
            .get(&key)
            .copied()
            .or_else(|| self.slot_budgets.get("*").copied())
    }

    pub fn lane_priority(&self, lane: &str) -> f32 {
        if self.lane_priorities.is_empty() {
            return 0.0;
        }
        let key = lane.trim().to_ascii_lowercase();
        if key.is_empty() {
            return 0.0;
        }
        self.lane_priorities
            .get(&key)
            .copied()
            .or_else(|| self.lane_priorities.get("*").copied())
            .unwrap_or(0.0)
    }

    pub fn merge_lane_priorities(&mut self, adjustments: &BTreeMap<String, f32>) {
        if adjustments.is_empty() {
            return;
        }
        for (lane, delta) in adjustments {
            if !delta.is_finite() {
                continue;
            }
            let trimmed = lane.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                continue;
            }
            let value = (*delta).clamp(-1.0, 1.0);
            if value.abs() < f32::EPSILON {
                continue;
            }
            let entry = self.lane_priorities.entry(trimmed).or_insert(0.0);
            *entry = (*entry + value).clamp(-1.0, 1.0);
        }
    }

    pub fn merge_slot_budgets(&mut self, overrides: &BTreeMap<String, usize>) {
        if overrides.is_empty() {
            return;
        }
        for (slot, limit) in overrides {
            let key = slot.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            let existing = self.slot_budgets.entry(key).or_insert(0);
            if *limit > *existing {
                *existing = *limit;
            }
        }
    }

    pub fn adjust_min_score(&mut self, delta: f32) {
        if !delta.is_finite() || delta.abs() < f32::EPSILON {
            return;
        }
        self.min_score = (self.min_score + delta).clamp(0.0, 1.0);
    }
}

fn env_flag(key: &str) -> Option<bool> {
    std::env::var(key).ok().and_then(|v| {
        let v = v.trim().to_ascii_lowercase();
        match v.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

pub fn default_lanes() -> Vec<String> {
    std::env::var("ARW_CONTEXT_LANES_DEFAULT")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|lanes: &Vec<String>| !lanes.is_empty())
        .unwrap_or_else(|| {
            vec![
                "semantic".to_string(),
                "procedural".to_string(),
                "episodic".to_string(),
                "story_thread".to_string(),
            ]
        })
}

pub fn default_limit() -> usize {
    std::env::var("ARW_CONTEXT_K")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(18)
}

pub fn default_expand_per_seed() -> usize {
    std::env::var("ARW_CONTEXT_EXPAND_PER_SEED")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3)
        .min(16)
}

pub fn default_diversity_lambda() -> f32 {
    std::env::var("ARW_CONTEXT_DIVERSITY_LAMBDA")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.72)
}

pub fn default_min_score() -> f32 {
    std::env::var("ARW_CONTEXT_MIN_SCORE")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.1)
}

pub fn default_lane_bonus() -> f32 {
    std::env::var("ARW_CONTEXT_LANE_BONUS")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.05)
        .clamp(0.0, 1.0)
}

pub fn default_expand_query() -> bool {
    env_flag("ARW_CONTEXT_EXPAND_QUERY").unwrap_or(false)
}

pub fn default_expand_query_top_k() -> usize {
    std::env::var("ARW_CONTEXT_EXPAND_QUERY_TOP_K")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(4)
        .min(32)
}

pub fn default_scorer() -> String {
    std::env::var("ARW_CONTEXT_SCORER")
        .ok()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "mmrd".to_string())
}

pub fn default_max_iterations() -> usize {
    std::env::var("ARW_CONTEXT_COVERAGE_MAX_ITERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(2)
        .min(6)
}

pub fn default_streaming_enabled() -> bool {
    env_flag("ARW_CONTEXT_STREAM_DEFAULT").unwrap_or(false)
}

pub fn default_slot_budgets() -> BTreeMap<String, usize> {
    let mut budgets = BTreeMap::new();
    let raw = match std::env::var("ARW_CONTEXT_SLOT_BUDGETS") {
        Ok(raw) => raw,
        Err(_) => return budgets,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return budgets;
    }
    if trimmed.starts_with('{') {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
            for (slot, value) in map.into_iter() {
                if let Some(parsed) = parse_slot_budget_value(value) {
                    let key = normalize_slot_key(&slot);
                    if !key.is_empty() {
                        budgets.insert(key, parsed);
                    }
                }
            }
        }
        return budgets;
    }
    for part in trimmed.split(',') {
        let mut iter = part.splitn(2, '=');
        let key = iter.next().unwrap_or("").trim();
        let value = iter.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        if let Ok(parsed) = value.parse::<usize>() {
            let key = normalize_slot_key(key);
            if !key.is_empty() {
                budgets.insert(key, parsed);
            }
        }
    }
    budgets
}

fn parse_slot_budget_value(value: Value) -> Option<usize> {
    match value {
        Value::Number(num) => num.as_u64().map(|v| v as usize),
        Value::String(s) => s.trim().parse::<usize>().ok(),
        Value::Bool(b) => Some(if b { 1 } else { 0 }),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn normalize_slot_key(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

pub fn default_lane_priorities() -> BTreeMap<String, f32> {
    let mut weights = BTreeMap::new();
    let raw = match std::env::var("ARW_CONTEXT_LANE_PRIORITIES") {
        Ok(raw) => raw,
        Err(_) => return weights,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return weights;
    }
    if trimmed.starts_with('{') {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(trimmed) {
            for (lane, value) in map {
                if let Some(weight) = parse_lane_priority_value(value) {
                    let key = normalize_slot_key(&lane);
                    if !key.is_empty() {
                        weights.insert(key, weight);
                    }
                }
            }
        }
        return weights;
    }
    for part in trimmed.split(',') {
        let mut iter = part.splitn(2, '=');
        let key = iter.next().unwrap_or("").trim();
        let value = iter.next().unwrap_or("").trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        if let Ok(parsed) = value.parse::<f32>() {
            let key = normalize_slot_key(key);
            if !key.is_empty() && parsed.is_finite() {
                weights.insert(key, parsed.clamp(-1.0, 1.0));
            }
        }
    }
    weights
}

fn parse_lane_priority_value(value: Value) -> Option<f32> {
    match value {
        Value::Number(num) => num.as_f64().map(|v| v as f32),
        Value::String(s) => s.trim().parse::<f32>().ok(),
        Value::Bool(true) => Some(0.25),
        Value::Bool(false) => Some(0.0),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => None,
    }
    .map(|v| v.clamp(-1.0, 1.0))
}
