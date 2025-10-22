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

pub async fn load_world_beliefs(service: &PersonaService, persona_id: &str) -> Result<Vec<Value>> {
    match service.get_entry(persona_id.to_string()).await? {
        Some(entry) => Ok(entry_world_beliefs(&entry)),
        None => Ok(Vec::new()),
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
