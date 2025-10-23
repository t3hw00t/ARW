use std::collections::BTreeMap;
use std::sync::Arc;

use metrics::counter;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc::{error::TrySendError, Sender};

use arw_events::Bus;

use super::SharedValue;

pub struct WorkingSet {
    pub items: Vec<SharedValue>,
    pub seeds: Vec<SharedValue>,
    pub expanded: Vec<SharedValue>,
    pub diagnostics: SharedValue,
    pub summary: WorkingSetSummary,
}

#[derive(Debug, Clone, Default)]
pub struct WorkingSetSummary {
    pub target_limit: usize,
    pub lanes_requested: usize,
    pub selected: usize,
    pub avg_cscore: f32,
    pub max_cscore: f32,
    pub min_cscore: f32,
    pub threshold_hits: usize,
    pub total_candidates: usize,
    pub lane_counts: BTreeMap<String, usize>,
    pub lane_priorities: BTreeMap<String, f32>,
    pub slot_counts: BTreeMap<String, usize>,
    pub slot_budgets: BTreeMap<String, usize>,
    pub min_score: f32,
    pub scorer: String,
}

impl WorkingSetSummary {
    pub fn to_json(&self) -> Value {
        let mut obj = Map::new();
        obj.insert("target_limit".into(), json!(self.target_limit));
        obj.insert("lanes_requested".into(), json!(self.lanes_requested));
        obj.insert("selected".into(), json!(self.selected));
        obj.insert("avg_cscore".into(), json!(self.avg_cscore));
        obj.insert("max_cscore".into(), json!(self.max_cscore));
        obj.insert("min_cscore".into(), json!(self.min_cscore));
        obj.insert("threshold_hits".into(), json!(self.threshold_hits));
        obj.insert("total_candidates".into(), json!(self.total_candidates));
        let mut lanes = Map::new();
        for (lane, count) in self.lane_counts.iter() {
            lanes.insert(lane.clone(), json!(count));
        }
        obj.insert("lane_counts".into(), Value::Object(lanes));
        if !self.lane_priorities.is_empty() {
            let mut prefs = Map::new();
            for (lane, weight) in self.lane_priorities.iter() {
                prefs.insert(lane.clone(), json!(weight));
            }
            obj.insert("lane_priorities".into(), Value::Object(prefs));
        }
        if !self.slot_counts.is_empty() || !self.slot_budgets.is_empty() {
            let mut slots = Map::new();
            if !self.slot_counts.is_empty() {
                let mut counts = Map::new();
                for (slot, count) in self.slot_counts.iter() {
                    counts.insert(slot.clone(), json!(count));
                }
                slots.insert("counts".into(), Value::Object(counts));
            }
            if !self.slot_budgets.is_empty() {
                let mut budgets = Map::new();
                for (slot, limit) in self.slot_budgets.iter() {
                    budgets.insert(slot.clone(), json!(limit));
                }
                slots.insert("budgets".into(), Value::Object(budgets));
            }
            obj.insert("slots".into(), Value::Object(slots));
        }
        obj.insert("min_score".into(), json!(self.min_score));
        obj.insert("scorer".into(), json!(self.scorer));
        Value::Object(obj)
    }
}

#[derive(Clone, Debug)]
pub struct WorkingSetStreamEvent {
    pub iteration: usize,
    pub kind: String,
    pub payload: SharedValue,
}

pub trait WorkingSetObserver {
    fn emit(&mut self, kind: &'static str, payload: SharedValue);
}

impl WorkingSetObserver for () {
    fn emit(&mut self, _kind: &'static str, _payload: SharedValue) {}
}

pub struct ChannelObserver {
    iteration: usize,
    tx: Sender<WorkingSetStreamEvent>,
}

impl ChannelObserver {
    pub fn new(iteration: usize, tx: Sender<WorkingSetStreamEvent>) -> Self {
        Self { iteration, tx }
    }
}

impl WorkingSetObserver for ChannelObserver {
    fn emit(&mut self, kind: &'static str, payload: SharedValue) {
        counter!(
            "arw_context_observer_emit_total",
            "observer" => "channel",
            "event" => kind
        )
        .increment(1);
        let evt = WorkingSetStreamEvent {
            iteration: self.iteration,
            kind: kind.to_string(),
            payload,
        };
        if let Err(err) = self.tx.try_send(evt) {
            match err {
                TrySendError::Full(evt) => {
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(evt).await;
                    });
                }
                TrySendError::Closed(_) => {}
            }
        }
    }
}

#[derive(Clone)]
pub struct BusObserver {
    bus: Bus,
    iteration: usize,
    corr_id: Option<String>,
    project: Option<String>,
    query: Option<String>,
}

impl BusObserver {
    pub fn new(
        bus: Bus,
        iteration: usize,
        corr_id: Option<String>,
        project: Option<String>,
        query: Option<String>,
    ) -> Self {
        Self {
            bus,
            iteration,
            corr_id,
            project,
            query,
        }
    }

    fn enrich_value(&self, payload: &Value) -> Value {
        let mut map: Map<String, Value> = match payload {
            Value::Object(map) => map.clone(),
            other => {
                let mut map = Map::new();
                map.insert("value".into(), other.clone());
                map
            }
        };
        map.insert("iteration".into(), json!(self.iteration));
        if let Some(corr) = &self.corr_id {
            map.insert("corr_id".into(), Value::String(corr.clone()));
        }
        if let Some(project) = &self.project {
            map.insert("project".into(), Value::String(project.clone()));
        }
        if let Some(query) = &self.query {
            map.insert("query".into(), Value::String(query.clone()));
        }
        Value::Object(map)
    }

    fn publish_enriched(&self, kind: &'static str, value: &Value) {
        self.bus.publish(kind, value);
    }
}

impl WorkingSetObserver for BusObserver {
    fn emit(&mut self, kind: &'static str, payload: SharedValue) {
        counter!(
            "arw_context_observer_emit_total",
            "observer" => "bus",
            "event" => kind
        )
        .increment(1);
        let enriched = self.enrich_value(payload.as_ref());
        self.publish_enriched(kind, &enriched);
    }
}

pub struct CompositeObserver<A> {
    first: A,
    second: BusObserver,
}

impl<A> CompositeObserver<A> {
    pub fn new(first: A, second: BusObserver) -> Self {
        Self { first, second }
    }
}

impl<A> WorkingSetObserver for CompositeObserver<A>
where
    A: WorkingSetObserver,
{
    fn emit(&mut self, kind: &'static str, payload: SharedValue) {
        counter!(
            "arw_context_observer_emit_total",
            "observer" => "composite",
            "event" => kind
        )
        .increment(1);
        self.first.emit(kind, Arc::clone(&payload));
        self.second
            .publish_enriched(kind, &self.second.enrich_value(payload.as_ref()));
    }
}
