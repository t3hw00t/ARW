use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use arw_events::Bus;
use arw_topics as topics;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::warn;
use utoipa::ToSchema;

use crate::responses;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema, Default)]
#[serde(default)]
pub struct EconomyStakeholderShare {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub share: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema, Default)]
#[serde(default)]
pub struct EconomyLedgerEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stakeholders: Vec<EconomyStakeholderShare>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gross_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net_amount: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema, Default)]
#[serde(default)]
pub struct EconomyLedgerTotal {
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default, ToSchema)]
#[serde(default)]
pub struct EconomyUsageCounters {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_requests: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, ToSchema)]
pub struct EconomyLedgerSnapshot {
    pub version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<EconomyLedgerEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub totals: Vec<EconomyLedgerTotal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attention: Vec<String>,
    #[serde(default)]
    pub usage: EconomyUsageCounters,
}

impl Default for EconomyLedgerSnapshot {
    fn default() -> Self {
        Self {
            version: 0,
            generated: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
            entries: Vec::new(),
            totals: Vec::new(),
            attention: Vec::new(),
            usage: EconomyUsageCounters::default(),
        }
    }
}

#[derive(Default)]
struct EconomyLedgerState {
    version: u64,
    snapshot: EconomyLedgerSnapshot,
}

pub struct EconomyLedger {
    store: RwLock<EconomyLedgerState>,
    bus: Bus,
    path: Option<PathBuf>,
}

impl EconomyLedger {
    #[allow(dead_code)]
    pub fn new(bus: Bus) -> Arc<Self> {
        Arc::new(Self {
            store: RwLock::new(EconomyLedgerState::default()),
            bus,
            path: None,
        })
    }

    pub async fn with_state_path(bus: Bus, path: PathBuf) -> Arc<Self> {
        let ledger = Arc::new(Self {
            store: RwLock::new(EconomyLedgerState::default()),
            bus,
            path: Some(path),
        });
        ledger.load_from_disk().await;
        ledger
    }

    pub async fn snapshot(&self) -> EconomyLedgerSnapshot {
        let guard = self.store.read().await;
        guard.snapshot.clone()
    }

    pub async fn replace(
        &self,
        entries: Vec<EconomyLedgerEntry>,
        totals: Vec<EconomyLedgerTotal>,
        attention: Vec<String>,
        usage: EconomyUsageCounters,
    ) {
        let mut guard = self.store.write().await;
        let next_version = guard.version.saturating_add(1);
        guard.version = next_version;
        guard.snapshot.version = next_version;
        guard.snapshot.entries = entries;
        guard.snapshot.totals = totals;
        guard.snapshot.attention = attention;
        guard.snapshot.usage = usage;
        guard.snapshot.generated = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        let snapshot = guard.snapshot.clone();
        drop(guard);
        self.persist(&snapshot).await;
        self.publish(&snapshot);
    }

    #[allow(dead_code)]
    pub async fn push_entry(&self, entry: EconomyLedgerEntry) {
        let mut guard = self.store.write().await;
        let next_version = guard.version.saturating_add(1);
        guard.version = next_version;
        guard.snapshot.version = next_version;
        guard.snapshot.generated = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        guard.snapshot.entries.push(entry);
        let snapshot = guard.snapshot.clone();
        drop(guard);
        self.persist(&snapshot).await;
        self.publish(&snapshot);
    }

    #[allow(dead_code)]
    pub async fn clear(&self) {
        self.replace(
            Vec::new(),
            Vec::new(),
            Vec::new(),
            EconomyUsageCounters::default(),
        )
        .await;
    }

    fn publish(&self, snapshot: &EconomyLedgerSnapshot) {
        if let Ok(mut payload) = serde_json::to_value(snapshot) {
            responses::attach_corr(&mut payload);
            self.bus
                .publish(topics::TOPIC_ECONOMY_LEDGER_UPDATED, &payload);
        } else {
            let mut payload = json!({"version": snapshot.version});
            responses::attach_corr(&mut payload);
            self.bus
                .publish(topics::TOPIC_ECONOMY_LEDGER_UPDATED, &payload);
        }
    }

    async fn persist(&self, snapshot: &EconomyLedgerSnapshot) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                warn!(error = %err, "failed to create economy ledger directory");
                return;
            }
        }
        match serde_json::to_vec_pretty(snapshot) {
            Ok(bytes) => {
                if let Err(err) = tokio::fs::write(path, bytes).await {
                    warn!(error = %err, path = %path.display(), "failed to persist economy ledger");
                }
            }
            Err(err) => warn!(error = %err, "failed to serialize economy ledger snapshot"),
        }
    }

    async fn load_from_disk(self: &Arc<Self>) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        match Self::read_snapshot_from_path(path).await {
            Ok(snapshot) => {
                let mut guard = self.store.write().await;
                guard.version = snapshot.version;
                guard.snapshot = snapshot;
            }
            Err(err) => warn!(
                error = %err,
                path = %path.display(),
                "failed to load economy ledger snapshot; using defaults"
            ),
        }
    }

    async fn read_snapshot_from_path(path: &Path) -> Result<EconomyLedgerSnapshot> {
        if !path.exists() {
            return Ok(EconomyLedgerSnapshot::default());
        }
        let bytes = tokio::fs::read(path).await?;
        if bytes.is_empty() {
            return Ok(EconomyLedgerSnapshot::default());
        }
        let mut snapshot: EconomyLedgerSnapshot = serde_json::from_slice(&bytes)?;
        if snapshot.generated.is_none() {
            snapshot.generated = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        }
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn snapshot_defaults_to_empty() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let temp = tempdir().expect("tempdir");
        let ledger =
            EconomyLedger::with_state_path(bus, temp.path().join("economy").join("ledger.json"))
                .await;
        let snap = ledger.snapshot().await;
        assert_eq!(snap.version, 0);
        assert!(snap.entries.is_empty());
        assert!(snap.totals.is_empty());
        assert!(snap.attention.is_empty());
        assert!(snap.usage.runtime_requests.is_empty());
    }

    #[tokio::test]
    async fn replace_updates_version_and_publishes() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let mut subscriber = bus.subscribe();
        let temp = tempdir().expect("tempdir");
        let ledger =
            EconomyLedger::with_state_path(bus.clone(), temp.path().join("ledger.json")).await;

        ledger
            .replace(
                vec![EconomyLedgerEntry {
                    id: "entry-1".into(),
                    currency: Some("USD".into()),
                    gross_amount: Some(10.0),
                    ..EconomyLedgerEntry::default()
                }],
                vec![EconomyLedgerTotal {
                    currency: "USD".into(),
                    pending: Some(10.0),
                    settled: Some(0.0),
                }],
                vec!["pending-settlement".into()],
                EconomyUsageCounters::default(),
            )
            .await;

        let snap = ledger.snapshot().await;
        assert_eq!(snap.version, 1);
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.totals.len(), 1);
        assert!(snap.usage.runtime_requests.is_empty());

        let event = subscriber.recv().await.expect("ledger event");
        assert_eq!(event.kind, topics::TOPIC_ECONOMY_LEDGER_UPDATED);
    }

    #[tokio::test]
    async fn writes_snapshot_to_disk() {
        let bus = arw_events::Bus::new_with_replay(4, 4);
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("economy").join("ledger.json");
        let ledger = EconomyLedger::with_state_path(bus, path.clone()).await;
        ledger
            .push_entry(EconomyLedgerEntry {
                id: "entry-42".into(),
                gross_amount: Some(42.5),
                ..EconomyLedgerEntry::default()
            })
            .await;
        let contents = tokio::fs::read_to_string(&path)
            .await
            .expect("ledger persisted");
        assert!(
            contents.contains("entry-42"),
            "ledger file should contain entry id"
        );
    }

    #[tokio::test]
    async fn loads_snapshot_from_disk() {
        let bus = arw_events::Bus::new_with_replay(4, 4);
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("ledger.json");
        let snapshot = EconomyLedgerSnapshot {
            version: 7,
            generated: Some("2025-10-25T00:00:00Z".into()),
            entries: vec![EconomyLedgerEntry {
                id: "existing-entry".into(),
                status: Some("settled".into()),
                ..EconomyLedgerEntry::default()
            }],
            totals: vec![EconomyLedgerTotal {
                currency: "USD".into(),
                pending: Some(0.0),
                settled: Some(100.0),
            }],
            attention: vec![],
            usage: EconomyUsageCounters::default(),
        };
        tokio::fs::write(
            &path,
            serde_json::to_vec_pretty(&snapshot).expect("serialize snapshot"),
        )
        .await
        .expect("write snapshot");
        let ledger = EconomyLedger::with_state_path(bus, path).await;
        let loaded = ledger.snapshot().await;
        assert_eq!(loaded.version, 7);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].id, "existing-entry");
        assert_eq!(loaded.totals.len(), 1);
    }
}




