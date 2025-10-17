use std::sync::Arc;

use anyhow::Result;
use arw_kernel::{
    Kernel, PersonaEntry, PersonaEntryUpsert, PersonaHistoryAppend, PersonaHistoryEntry,
    PersonaProposal, PersonaProposalCreate, PersonaProposalStatusUpdate,
};

#[derive(Clone)]
pub struct PersonaService {
    kernel: Kernel,
}

impl PersonaService {
    pub fn new(kernel: Kernel) -> Arc<Self> {
        Arc::new(Self { kernel })
    }

    #[allow(dead_code)]
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
}
