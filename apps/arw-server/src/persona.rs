use std::collections::{BTreeMap, HashSet};
use std::env;
use std::sync::Arc;

use anyhow::Result;
use arw_kernel::{
    Kernel, PersonaEntry, PersonaEntryUpsert, PersonaHistoryAppend, PersonaHistoryEntry,
    PersonaProposal, PersonaProposalCreate, PersonaProposalStatusUpdate, PersonaVibeSample,
    PersonaVibeSampleCreate,
};
use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct PersonaService {
    kernel: Kernel,
    metrics: Arc<VibeMetricsStore>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{SecondsFormat, Utc};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn make_entry() -> PersonaEntry {
        PersonaEntry {
            id: "persona.test".into(),
            owner_kind: "workspace".into(),
            owner_ref: "local".into(),
            name: Some("Test".into()),
            archetype: None,
            traits: json!({}),
            preferences: json!({
                "context": {
                    "lane_weights": { "semantic": 0.2 },
                    "slot_budgets": { "evidence": 3 },
                    "min_score_bias": 0.1
                }
            }),
            worldview: json!({ "values": ["care"] }),
            vibe_profile: json!({}),
            calibration: json!({}),
            updated: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            version: 1,
        }
    }

    #[test]
    fn bias_respects_preferences_and_signals() {
        let entry = make_entry();
        let mut metrics = PersonaVibeMetricsSnapshot {
            persona_id: entry.id.clone(),
            total_feedback: 1,
            signal_counts: BTreeMap::new(),
            average_strength: Some(0.7),
            last_signal: Some("lane+:episodic".into()),
            last_strength: Some(0.7),
            last_updated: None,
            retain_max: 50,
        };
        metrics.signal_counts.insert("lane+:episodic".into(), 1);
        let bias = compute_context_bias(&entry, &metrics);
        assert_eq!(bias.slot_overrides.get("evidence"), Some(&3));
        assert!(bias.lane_priorities.get("semantic").unwrap() >= &0.2);
        assert!(bias.lane_priorities.contains_key("episodic"));
        assert!(bias.min_score_delta > 0.09);
    }

    #[test]
    fn bias_handles_generic_warmer_signal() {
        let entry = make_entry();
        let metrics = PersonaVibeMetricsSnapshot {
            persona_id: entry.id.clone(),
            total_feedback: 1,
            signal_counts: BTreeMap::new(),
            average_strength: Some(0.4),
            last_signal: Some("warmer".into()),
            last_strength: Some(0.4),
            last_updated: None,
            retain_max: 50,
        };
        let bias = compute_context_bias(&entry, &metrics);
        assert!(bias.lane_priorities.get("*").unwrap() > &0.05);
    }
}

impl PersonaService {
    pub fn new(kernel: Kernel) -> Arc<Self> {
        Arc::new(Self {
            kernel,
            metrics: Arc::new(VibeMetricsStore::default()),
        })
    }

    pub async fn upsert_entry(&self, upsert: PersonaEntryUpsert) -> Result<PersonaEntry> {
        self.kernel.upsert_persona_entry_async(upsert).await
    }

    pub async fn get_entry(&self, id: String) -> Result<Option<PersonaEntry>> {
        self.kernel.get_persona_entry_async(id).await
    }

    pub async fn list_entries(
        &self,
        owner_kind: Option<String>,
        owner_ref: Option<String>,
        limit: i64,
    ) -> Result<Vec<PersonaEntry>> {
        self.kernel
            .list_persona_entries_async(owner_kind, owner_ref, limit)
            .await
    }

    pub async fn create_proposal(&self, create: PersonaProposalCreate) -> Result<String> {
        self.kernel.insert_persona_proposal_async(create).await
    }

    pub async fn update_proposal_status(
        &self,
        proposal_id: String,
        status: PersonaProposalStatusUpdate,
    ) -> Result<bool> {
        self.kernel
            .update_persona_proposal_status_async(proposal_id, status)
            .await
    }

    #[allow(dead_code)]
    pub async fn list_proposals(
        &self,
        persona_id: Option<String>,
        status: Option<String>,
        limit: i64,
    ) -> Result<Vec<PersonaProposal>> {
        self.kernel
            .list_persona_proposals_async(persona_id, status, limit)
            .await
    }

    pub async fn get_proposal(&self, proposal_id: String) -> Result<Option<PersonaProposal>> {
        self.kernel.get_persona_proposal_async(proposal_id).await
    }

    pub async fn apply_diff(
        &self,
        persona_id: String,
        diff: serde_json::Value,
    ) -> Result<PersonaEntry> {
        self.kernel.apply_persona_diff_async(persona_id, diff).await
    }

    pub async fn append_history(&self, entry: PersonaHistoryAppend) -> Result<i64> {
        self.kernel.append_persona_history_async(entry).await
    }

    pub async fn list_history(
        &self,
        persona_id: String,
        limit: i64,
    ) -> Result<Vec<PersonaHistoryEntry>> {
        self.kernel
            .list_persona_history_async(persona_id, limit)
            .await
    }

    pub async fn publish_feedback(
        &self,
        bus: arw_events::Bus,
        persona_id: String,
        payload: serde_json::Value,
    ) -> Result<()> {
        let mut enriched = payload;
        if let serde_json::Value::Object(ref mut map) = enriched {
            map.entry("persona_id")
                .or_insert_with(|| serde_json::Value::String(persona_id));
        }
        bus.publish(arw_topics::TOPIC_PERSONA_FEEDBACK, &enriched);
        Ok(())
    }

    pub async fn record_vibe_feedback(
        &self,
        kind: Option<String>,
        persona_id: String,
        signal: Option<String>,
        strength: Option<f32>,
        note: Option<String>,
        metadata: serde_json::Value,
    ) -> Result<()> {
        let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        self.metrics
            .record(
                persona_id.clone(),
                signal.clone(),
                strength,
                timestamp.clone(),
            )
            .await;
        self.kernel
            .insert_persona_vibe_sample_async(
                PersonaVibeSampleCreate {
                    persona_id,
                    kind,
                    signal,
                    strength,
                    note,
                    metadata,
                    recorded_at: timestamp,
                },
                vibe_sample_retain(),
            )
            .await?;
        Ok(())
    }

    pub async fn vibe_metrics_snapshot(&self, persona_id: String) -> PersonaVibeMetricsSnapshot {
        let snapshot = self.metrics.snapshot(&persona_id).await;
        if snapshot.total_feedback > 0 {
            return snapshot;
        }

        match self
            .kernel
            .list_persona_vibe_samples_async(persona_id.clone(), vibe_sample_retain())
            .await
        {
            Ok(samples) if !samples.is_empty() => {
                self.metrics
                    .rebuild_from_samples(&persona_id, &samples)
                    .await;
                self.metrics.snapshot(&persona_id).await
            }
            _ => snapshot,
        }
    }

    pub async fn list_vibe_history(
        &self,
        persona_id: String,
        limit: i64,
    ) -> Result<Vec<PersonaVibeSample>> {
        self.kernel
            .list_persona_vibe_samples_async(persona_id, limit)
            .await
    }
}

pub(crate) fn vibe_sample_retain() -> i64 {
    env::var("ARW_PERSONA_VIBE_HISTORY_RETAIN")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|v| v.clamp(1, 500))
        .unwrap_or(50)
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonaVibeMetricsSnapshot {
    pub persona_id: String,
    pub total_feedback: u64,
    pub signal_counts: BTreeMap<String, u64>,
    pub average_strength: Option<f32>,
    pub last_signal: Option<String>,
    pub last_strength: Option<f32>,
    pub last_updated: Option<String>,
    pub retain_max: i64,
}

impl PersonaVibeMetricsSnapshot {
    fn empty(persona_id: String) -> Self {
        Self {
            persona_id,
            total_feedback: 0,
            signal_counts: BTreeMap::new(),
            average_strength: None,
            last_signal: None,
            last_strength: None,
            last_updated: None,
            retain_max: vibe_sample_retain(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PersonaContextBias {
    pub lane_priorities: BTreeMap<String, f32>,
    pub slot_overrides: BTreeMap<String, usize>,
    pub min_score_delta: f32,
}

impl PersonaContextBias {
    pub fn is_empty(&self) -> bool {
        self.lane_priorities.is_empty()
            && self.slot_overrides.is_empty()
            && self.min_score_delta.abs() < f32::EPSILON
    }
}

pub fn compute_context_bias(
    entry: &PersonaEntry,
    metrics: &PersonaVibeMetricsSnapshot,
) -> PersonaContextBias {
    let mut bias = PersonaContextBias::default();

    if let Some(prefs) = entry.preferences.as_object() {
        if let Some(context) = prefs.get("context").and_then(|v| v.as_object()) {
            if let Some(lanes) = context.get("lane_weights").and_then(|v| v.as_object()) {
                for (lane, value) in lanes {
                    if let Some(weight) = lane_weight_from_value(value) {
                        let key = lane.trim().to_ascii_lowercase();
                        if key.is_empty() {
                            continue;
                        }
                        bias.lane_priorities.insert(key, weight.clamp(-1.0, 1.0));
                    }
                }
            }
            if let Some(slots) = context.get("slot_budgets").and_then(|v| v.as_object()) {
                for (slot, value) in slots {
                    if let Some(limit) = slot_limit_from_value(value) {
                        let key = slot.trim().to_ascii_lowercase();
                        if key.is_empty() || limit == 0 {
                            continue;
                        }
                        bias.slot_overrides.insert(key, limit);
                    }
                }
            }
            if let Some(delta) = context.get("min_score_bias") {
                if let Some(value) = numeric_from_value(delta) {
                    let clamped = (value as f32).clamp(-0.5, 0.5);
                    if clamped.abs() >= f32::EPSILON {
                        bias.min_score_delta = clamped;
                    }
                }
            }
        }
    }

    if let Some(signal) = metrics.last_signal.as_deref() {
        apply_vibe_signal(&mut bias, signal, metrics.last_strength);
    }

    bias
}

fn lane_weight_from_value(value: &Value) -> Option<f32> {
    match value {
        Value::Number(num) => num.as_f64().map(|v| v as f32),
        Value::String(s) => s.trim().parse::<f32>().ok(),
        Value::Bool(true) => Some(0.25),
        Value::Bool(false) => Some(0.0),
        _ => None,
    }
}

fn slot_limit_from_value(value: &Value) -> Option<usize> {
    match value {
        Value::Number(num) => {
            if let Some(v) = num.as_u64() {
                Some(v as usize)
            } else {
                num.as_f64()
                    .filter(|v| *v >= 0.0)
                    .map(|v| v.round() as usize)
            }
        }
        Value::String(s) => s.trim().parse::<usize>().ok(),
        Value::Bool(true) => Some(1usize),
        Value::Bool(false) => Some(0usize),
        _ => None,
    }
    .filter(|v| *v > 0)
}

fn numeric_from_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(num) => num.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn apply_vibe_signal(bias: &mut PersonaContextBias, signal: &str, strength: Option<f32>) {
    let trimmed = signal.trim();
    if trimmed.is_empty() {
        return;
    }
    let normalized = trimmed.to_ascii_lowercase();
    let strength = strength.unwrap_or(0.6).clamp(0.0, 1.0);
    let lane_delta = |positive: bool| -> f32 {
        let base = 0.05 + (strength * 0.25);
        if positive {
            base
        } else {
            -base
        }
    };
    let slot_min = |s: f32| -> usize {
        let value = (1.0 + s * 3.0).round() as i32;
        value.max(1) as usize
    };

    let mut apply_lane = |lane: &str, delta: f32| {
        if delta.abs() < f32::EPSILON {
            return;
        }
        let key = lane.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let entry = bias.lane_priorities.entry(key).or_insert(0.0);
        *entry = (*entry + delta).clamp(-1.0, 1.0);
    };

    let mut apply_slot = |slot: &str, min_count: usize| {
        if min_count == 0 {
            return;
        }
        let key = slot.trim().to_ascii_lowercase();
        if key.is_empty() {
            return;
        }
        let entry = bias.slot_overrides.entry(key).or_insert(0);
        if min_count > *entry {
            *entry = min_count;
        }
    };

    if let Some(rest) = normalized.strip_prefix("lane+:") {
        apply_lane(rest, lane_delta(true));
        return;
    }
    if let Some(rest) = normalized.strip_prefix("lane-:") {
        apply_lane(rest, lane_delta(false));
        return;
    }
    if let Some(rest) = normalized.strip_prefix("lane:") {
        apply_lane(rest, lane_delta(true));
        return;
    }
    if let Some(rest) = normalized.strip_prefix("slot+:") {
        apply_slot(rest, slot_min(strength));
        return;
    }
    if let Some(rest) = normalized.strip_prefix("slot:") {
        apply_slot(rest, slot_min(strength));
        return;
    }
    if let Some(rest) = normalized.strip_prefix("minscore+:") {
        let delta = lane_delta(true);
        bias.min_score_delta = (bias.min_score_delta + delta).clamp(-0.5, 0.5);
        if !rest.trim().is_empty() {
            apply_lane(rest, lane_delta(true) * 0.5);
        }
        return;
    }
    if let Some(rest) = normalized.strip_prefix("minscore-:") {
        let delta = lane_delta(false);
        bias.min_score_delta = (bias.min_score_delta + delta).clamp(-0.5, 0.5);
        if !rest.trim().is_empty() {
            apply_lane(rest, lane_delta(false) * 0.5);
        }
        return;
    }

    match normalized.as_str() {
        "warmer" => apply_lane("*", lane_delta(true)),
        "cooler" => apply_lane("*", lane_delta(false)),
        "broader" => apply_slot("*", slot_min(strength + 0.1)),
        "narrower" => {
            let adjustment = lane_delta(false) * 0.5;
            bias.min_score_delta = (bias.min_score_delta + adjustment).clamp(-0.5, 0.5);
        }
        _ => {}
    }
}

#[derive(Default)]
struct VibeMetricsStore {
    inner: RwLock<BTreeMap<String, PersonaVibeMetricsState>>,
}

impl VibeMetricsStore {
    async fn record(
        &self,
        persona_id: String,
        signal: Option<String>,
        strength: Option<f32>,
        timestamp: String,
    ) {
        let mut guard = self.inner.write().await;
        let entry = guard
            .entry(persona_id)
            .or_insert_with(PersonaVibeMetricsState::default);
        entry.record(signal, strength, timestamp);
    }

    async fn snapshot(&self, persona_id: &str) -> PersonaVibeMetricsSnapshot {
        let guard = self.inner.read().await;
        guard
            .get(persona_id)
            .map(|state| state.snapshot(persona_id.to_string()))
            .unwrap_or_else(|| PersonaVibeMetricsSnapshot::empty(persona_id.to_string()))
    }

    async fn rebuild_from_samples(&self, persona_id: &str, samples: &[PersonaVibeSample]) {
        let mut state = PersonaVibeMetricsState::default();
        for sample in samples.iter().rev() {
            state.record(
                sample.signal.clone(),
                sample.strength,
                sample.recorded_at.clone(),
            );
        }

        let mut guard = self.inner.write().await;
        guard.insert(persona_id.to_string(), state);
    }
}

const PERSONA_BELIEF_SUMMARY_MAX: usize = 160;
const PERSONA_BELIEF_LIMIT: usize = 16;

#[derive(Clone, Copy)]
struct BeliefParams<'a> {
    persona_id: &'a str,
    kind: &'a str,
    slot: &'a str,
    updated: &'a str,
    confidence: f32,
    max_entries: usize,
}

impl<'a> BeliefParams<'a> {
    fn new(
        persona_id: &'a str,
        kind: &'a str,
        slot: &'a str,
        updated: &'a str,
        confidence: f32,
        max_entries: usize,
    ) -> Self {
        Self {
            persona_id,
            kind,
            slot,
            updated,
            confidence,
            max_entries,
        }
    }
}

struct BeliefBuilder<'a, 'b> {
    params: BeliefParams<'a>,
    out: &'b mut Vec<Value>,
    seen: &'b mut HashSet<String>,
    added: usize,
}

impl<'a, 'b> BeliefBuilder<'a, 'b> {
    fn new(
        params: BeliefParams<'a>,
        out: &'b mut Vec<Value>,
        seen: &'b mut HashSet<String>,
    ) -> Self {
        Self {
            params,
            out,
            seen,
            added: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.added >= self.params.max_entries
    }

    fn push(&mut self, label: &str, raw_value: &Value) -> bool {
        if self.is_full() {
            return false;
        }
        let trimmed_label = label.trim();
        if trimmed_label.is_empty() {
            return false;
        }
        let slug = match slugify_label(trimmed_label) {
            Some(slug) => slug,
            None => return false,
        };
        let belief_id = format!(
            "persona::{}::{}::{}",
            self.params.persona_id, self.params.slot, slug
        );
        if !self.seen.insert(belief_id.clone()) {
            return false;
        }
        let summary = truncate_summary(&value_summary(raw_value), PERSONA_BELIEF_SUMMARY_MAX);
        let belief = json!({
            "id": belief_id,
            "persona_id": self.params.persona_id,
            "lane": "worldview",
            "slot": self.params.slot,
            "kind": self.params.kind,
            "label": trimmed_label,
            "summary": summary,
            "value": raw_value.clone(),
            "confidence": self.params.confidence,
            "weight": self.params.confidence,
            "updated": self.params.updated,
            "source": "persona",
            "rationale": {
                "type": self.params.kind,
                "label": trimmed_label,
            }
        });
        self.out.push(belief);
        self.added += 1;
        true
    }
}

pub fn entry_world_beliefs(entry: &PersonaEntry) -> Vec<Value> {
    let mut beliefs = Vec::new();
    let updated = entry.updated.clone();
    let base_confidence = entry
        .calibration
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.62) as f32;

    beliefs.push(json!({
        "id": format!("persona::{}::profile", entry.id),
        "persona_id": entry.id,
        "lane": "worldview",
        "slot": "persona_profile",
        "kind": "persona_profile",
        "name": entry.name,
        "archetype": entry.archetype,
        "traits": entry.traits,
        "preferences": entry.preferences,
        "worldview": entry.worldview,
        "calibration": entry.calibration,
        "confidence": base_confidence,
        "weight": base_confidence,
        "updated": updated,
        "source": "persona",
        "rationale": {
            "type": "persona_profile",
            "version": entry.version,
        }
    }));

    let mut seen_ids: HashSet<String> =
        HashSet::from_iter([format!("persona::{}::profile", entry.id)]);
    let updated_ref = updated.as_str();
    extract_beliefs_from_value(
        &entry.traits,
        BeliefParams::new(
            entry.id.as_str(),
            "persona_trait",
            "persona_trait",
            updated_ref,
            (base_confidence * 0.9).clamp(0.05, 1.0),
            PERSONA_BELIEF_LIMIT,
        ),
        &mut beliefs,
        &mut seen_ids,
    );
    extract_beliefs_from_value(
        &entry.preferences,
        BeliefParams::new(
            entry.id.as_str(),
            "persona_preference",
            "persona_preference",
            updated_ref,
            (base_confidence * 0.85).clamp(0.05, 1.0),
            PERSONA_BELIEF_LIMIT,
        ),
        &mut beliefs,
        &mut seen_ids,
    );
    extract_beliefs_from_value(
        &entry.worldview,
        BeliefParams::new(
            entry.id.as_str(),
            "persona_worldview",
            "persona_worldview",
            updated_ref,
            base_confidence,
            PERSONA_BELIEF_LIMIT,
        ),
        &mut beliefs,
        &mut seen_ids,
    );
    beliefs
}

pub async fn load_context_bundle(
    service: &PersonaService,
    persona_id: &str,
) -> Result<(Vec<Value>, Option<PersonaContextBias>)> {
    match service.get_entry(persona_id.to_string()).await? {
        Some(entry) => {
            let beliefs = entry_world_beliefs(&entry);
            let metrics = service.vibe_metrics_snapshot(persona_id.to_string()).await;
            let bias = compute_context_bias(&entry, &metrics);
            Ok((beliefs, Some(bias)))
        }
        None => Ok((Vec::new(), None)),
    }
}

pub fn merge_world_beliefs(base: &Arc<[Value]>, extras: Vec<Value>) -> Arc<[Value]> {
    if extras.is_empty() {
        return base.clone();
    }
    let mut merged: Vec<Value> = Vec::with_capacity(base.len() + extras.len());
    let mut seen: HashSet<String> = HashSet::new();
    for belief in base.iter() {
        if let Some(id) = belief.get("id").and_then(|v| v.as_str()) {
            seen.insert(id.to_string());
        }
        merged.push(belief.clone());
    }
    for belief in extras {
        if let Some(id) = belief.get("id").and_then(|v| v.as_str()) {
            if !seen.insert(id.to_string()) {
                continue;
            }
        }
        merged.push(belief);
    }
    Arc::from(merged)
}

fn extract_beliefs_from_value(
    value: &Value,
    params: BeliefParams<'_>,
    out: &mut Vec<Value>,
    seen: &mut HashSet<String>,
) {
    let mut builder = BeliefBuilder::new(params, out, seen);
    match value {
        Value::Object(map) => {
            for (label, val) in map {
                if builder.is_full() {
                    break;
                }
                builder.push(label, val);
            }
        }
        Value::Array(items) => {
            for (idx, val) in items.iter().enumerate() {
                if builder.is_full() {
                    break;
                }
                let label = val
                    .get("label")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("name").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("{} {}", params.slot.replace('_', " "), idx + 1));
                builder.push(label.as_str(), val);
            }
        }
        other => {
            if !builder.is_full() {
                let label = params.kind.replace('_', " ");
                builder.push(&label, other);
            }
        }
    }
}

fn value_summary(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(items) => {
            if items.is_empty() {
                "[]".to_string()
            } else {
                let mut parts: Vec<String> = Vec::new();
                for item in items.iter().take(4) {
                    parts.push(value_summary(item));
                }
                if items.len() > 4 {
                    parts.push("…".into());
                }
                parts.join(", ")
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                "{}".to_string()
            } else {
                let mut parts: Vec<String> = Vec::new();
                for (idx, (key, val)) in map.iter().enumerate() {
                    if idx >= 4 {
                        parts.push("…".into());
                        break;
                    }
                    parts.push(format!("{key}: {}", value_summary(val)));
                }
                parts.join(", ")
            }
        }
    }
}

fn truncate_summary(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    if !out.ends_with('…') {
        out.push('…');
    }
    out
}

fn slugify_label(label: &str) -> Option<String> {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if matches!(ch, '-' | '_' | ' ' | ':' | '/' | '.') {
            if !prev_dash && !slug.is_empty() {
                slug.push('-');
                prev_dash = true;
            }
        } else {
            prev_dash = false;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

#[derive(Default)]
struct PersonaVibeMetricsState {
    total_feedback: u64,
    signal_counts: BTreeMap<String, u64>,
    sum_strength: f64,
    strength_samples: u64,
    last_signal: Option<String>,
    last_strength: Option<f32>,
    last_updated: Option<String>,
}

impl PersonaVibeMetricsState {
    fn record(&mut self, signal: Option<String>, strength: Option<f32>, timestamp: String) {
        self.total_feedback = self.total_feedback.saturating_add(1);
        let key = signal.unwrap_or_else(|| "unspecified".to_string());
        *self.signal_counts.entry(key.clone()).or_default() += 1;
        self.last_signal = Some(key);
        if let Some(s) = strength {
            self.sum_strength += s as f64;
            self.strength_samples = self.strength_samples.saturating_add(1);
            self.last_strength = Some(s);
        }
        self.last_updated = Some(timestamp);
    }

    fn snapshot(&self, persona_id: String) -> PersonaVibeMetricsSnapshot {
        let average_strength = if self.strength_samples > 0 {
            Some((self.sum_strength / self.strength_samples as f64) as f32)
        } else {
            None
        };

        PersonaVibeMetricsSnapshot {
            persona_id,
            total_feedback: self.total_feedback,
            signal_counts: self.signal_counts.clone(),
            average_strength,
            last_signal: self.last_signal.clone(),
            last_strength: self.last_strength,
            last_updated: self.last_updated.clone(),
            retain_max: vibe_sample_retain(),
        }
    }
}
