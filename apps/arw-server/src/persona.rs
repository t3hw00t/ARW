use std::collections::BTreeMap;
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
        self.metrics.snapshot(&persona_id).await
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
