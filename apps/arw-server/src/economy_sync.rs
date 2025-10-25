use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use arw_kernel::Kernel;
use serde_json::{json, Map, Value};
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::{
    cluster::ClusterRegistry,
    economy::{
        EconomyLedger, EconomyLedgerEntry, EconomyLedgerTotal, EconomyStakeholderShare,
        EconomyUsageCounters,
    },
    runtime::RuntimeRegistry,
    tasks::TaskHandle,
    AppState,
};
use arw_runtime::{RuntimeRecord, RuntimeSeverity, RuntimeState};
use chrono::SecondsFormat;

const SYNC_LIMIT: i64 = 500;
const SYNC_INTERVAL: Duration = Duration::from_secs(5);
const EPSILON: f64 = 1e-6;

pub fn start(state: AppState) -> TaskHandle {
    let kernel = state.kernel().clone();
    let ledger = state.economy();
    let runtime = state.runtime();
    let cluster = state.cluster();
    crate::tasks::spawn_supervised("economy.ledger.sync", move || {
        let kernel = kernel.clone();
        let ledger = ledger.clone();
        let runtime = runtime.clone();
        let cluster = cluster.clone();
        async move {
            loop {
                if let Err(err) =
                    sync_once(&kernel, ledger.clone(), runtime.clone(), cluster.clone()).await
                {
                    warn!(
                        target: "arw::economy",
                        error = %err,
                        "economy ledger sync failed"
                    );
                }
                sleep(SYNC_INTERVAL).await;
            }
        }
    })
}

async fn sync_once(
    kernel: &Kernel,
    ledger: Arc<EconomyLedger>,
    runtime: Arc<RuntimeRegistry>,
    cluster: Arc<ClusterRegistry>,
) -> Result<()> {
    let previous_snapshot = ledger.snapshot().await;
    let contributions = kernel
        .list_contributions_async(SYNC_LIMIT)
        .await
        .context("list contributions")?;
    let mut summary = build_summary(&contributions);
    summary.usage = previous_snapshot.usage.clone();
    let (usage_after_runtime, runtime_alerts) =
        augment_with_runtime(&mut summary, &previous_snapshot.usage, runtime).await;
    summary.usage = usage_after_runtime;
    let cluster_alerts = collect_cluster_alerts(cluster).await;
    if !runtime_alerts.is_empty() || !cluster_alerts.is_empty() {
        summary.attention.extend(runtime_alerts);
        summary.attention.extend(cluster_alerts);
        summary.attention.sort();
        summary.attention.dedup();
    }
    let unchanged = previous_snapshot.entries == summary.entries
        && previous_snapshot.totals == summary.totals
        && previous_snapshot.attention == summary.attention
        && previous_snapshot.usage == summary.usage;
    if unchanged {
        debug!(target: "arw::economy", "economy ledger unchanged");
        return Ok(());
    }
    let LedgerMaterial {
        entries,
        totals,
        attention,
        usage,
    } = summary;
    ledger.replace(entries, totals, attention, usage).await;
    Ok(())
}

struct LedgerMaterial {
    entries: Vec<EconomyLedgerEntry>,
    totals: Vec<EconomyLedgerTotal>,
    attention: Vec<String>,
    usage: EconomyUsageCounters,
}

fn build_summary(contributions: &[Value]) -> LedgerMaterial {
    let mut entries = Vec::new();
    for contribution in contributions.iter().rev() {
        if let Some(entry) = contribution_to_entry(contribution) {
            entries.push(entry);
        }
    }

    let mut totals_map: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let mut attention = Vec::new();

    for entry in &entries {
        let currency = entry
            .currency
            .clone()
            .unwrap_or_else(|| "unitless".to_string());
        let amount = entry.net_amount.or(entry.gross_amount).unwrap_or_default();
        let status = entry.status.as_deref().unwrap_or("pending");
        let bucket = totals_map.entry(currency.clone()).or_insert((0.0, 0.0));
        match status {
            "settled" => bucket.1 += amount,
            "cancelled" | "failed" => {
                bucket.0 += amount;
                attention.push(build_attention_message(entry, amount, "requires review"));
            }
            _ => {
                bucket.0 += amount;
                attention.push(build_attention_message(entry, amount, "pending"));
            }
        }
    }

    attention.sort();
    attention.dedup();

    let totals = totals_map
        .into_iter()
        .map(|(currency, (pending, settled))| EconomyLedgerTotal {
            currency,
            pending: if pending.abs() > EPSILON {
                Some((pending * 100.0).round() / 100.0)
            } else {
                None
            },
            settled: if settled.abs() > EPSILON {
                Some((settled * 100.0).round() / 100.0)
            } else {
                None
            },
        })
        .collect();

    LedgerMaterial {
        entries,
        totals,
        attention,
        usage: EconomyUsageCounters::default(),
    }
}

async fn collect_cluster_alerts(cluster: Arc<ClusterRegistry>) -> Vec<String> {
    let nodes = cluster.snapshot().await;
    let mut alerts = Vec::new();
    for node in nodes {
        if let Some(health) = node.health.as_deref() {
            if health != "ok" {
                alerts.push(format!("Cluster node {} {}", node.id, health));
            }
        }
        if let Some(last_seen_ms) = node.last_seen_ms {
            if last_seen_ms == 0 {
                alerts.push(format!(
                    "Cluster node {} has not reported heartbeat",
                    node.id
                ));
            }
        }
    }
    alerts
}

async fn augment_with_runtime(
    summary: &mut LedgerMaterial,
    previous_usage: &EconomyUsageCounters,
    runtime: Arc<RuntimeRegistry>,
) -> (EconomyUsageCounters, Vec<String>) {
    let snapshot = runtime.snapshot().await;
    let mut next_usage = previous_usage.clone();
    let mut alerts = Vec::new();

    for record in snapshot.runtimes {
        let runtime_id = record.descriptor.id.clone();
        let runtime_name = runtime_display_name(&record);
        let state = record.status.state;
        let severity = record.status.severity;
        let health = record.status.health.clone().unwrap_or_default();
        let total_hits = health.request_count.unwrap_or(0);
        let prev_hits = previous_usage
            .runtime_requests
            .get(&runtime_id)
            .copied()
            .unwrap_or(0);

        if matches!(state, RuntimeState::Error | RuntimeState::Offline) {
            alerts.push(format!("Runtime {runtime_name} {}", state.display_label()));
        } else if matches!(state, RuntimeState::Degraded) {
            alerts.push(format!("Runtime {runtime_name} degraded"));
        }
        if matches!(severity, RuntimeSeverity::Warn | RuntimeSeverity::Error)
            && !matches!(state, RuntimeState::Error | RuntimeState::Offline)
        {
            alerts.push(format!(
                "Runtime {runtime_name} severity {}",
                severity.display_label()
            ));
        }
        if let Some(inflight) = health.inflight_jobs {
            if inflight > 0 {
                alerts.push(format!(
                    "Runtime {runtime_name} {inflight} job{} in flight",
                    if inflight == 1 { "" } else { "s" }
                ));
            }
        }
        if let Some(error_rate) = health.error_rate {
            if error_rate > 0.0 {
                alerts.push(format!(
                    "Runtime {runtime_name} error rate {:.2}%",
                    error_rate * 100.0
                ));
            }
        }

        if total_hits > prev_hits {
            let delta = total_hits - prev_hits;
            let status_str = match state {
                RuntimeState::Ready => "settled",
                RuntimeState::Starting | RuntimeState::Unknown => "pending",
                RuntimeState::Degraded => "pending",
                RuntimeState::Error | RuntimeState::Offline => "requires_review",
            };
            let issued_at = record
                .status
                .updated_at
                .to_rfc3339_opts(SecondsFormat::Millis, true);
            let metadata = json!({
                "runtime_id": runtime_id,
                "runtime_name": runtime_name,
                "adapter": record.descriptor.adapter,
                "profile": record.descriptor.profile,
                "modalities": record.descriptor.modalities,
                "state": state.as_str(),
                "severity": severity.as_str(),
                "total_requests": total_hits,
                "delta_requests": delta,
                "latency_ms": health.latency_ms,
                "error_rate": health.error_rate,
                "inflight_jobs": health.inflight_jobs,
                "capacity": health.capacity,
                "prompt_cache_warm": health.prompt_cache_warm,
                "updated_at": issued_at,
            });
            summary.entries.push(EconomyLedgerEntry {
                id: format!("runtime.{}.{}", runtime_id, total_hits),
                job_id: Some(runtime_id.clone()),
                currency: Some("requests".into()),
                gross_amount: Some(delta as f64),
                net_amount: Some(delta as f64),
                status: Some(status_str.to_string()),
                issued_at: Some(issued_at),
                metadata: Some(metadata),
                ..EconomyLedgerEntry::default()
            });
            let settled = matches!(status_str, "settled");
            increment_totals(&mut summary.totals, "requests", delta as f64, settled);
        }

        next_usage.runtime_requests.insert(runtime_id, total_hits);
    }

    alerts.sort_by_key(|item| item.to_lowercase());
    alerts.dedup();

    (next_usage, alerts)
}

fn runtime_display_name(record: &RuntimeRecord) -> String {
    record
        .descriptor
        .name
        .clone()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| record.descriptor.id.clone())
}

fn increment_totals(
    totals: &mut Vec<EconomyLedgerTotal>,
    currency: &str,
    amount: f64,
    settled: bool,
) {
    if amount <= 0.0 {
        return;
    }
    if let Some(total) = totals.iter_mut().find(|t| t.currency == currency) {
        if settled {
            let current = total.settled.unwrap_or(0.0);
            total.settled = Some(current + amount);
        } else {
            let current = total.pending.unwrap_or(0.0);
            total.pending = Some(current + amount);
        }
    } else {
        totals.push(EconomyLedgerTotal {
            currency: currency.to_string(),
            pending: if settled { None } else { Some(amount) },
            settled: if settled { Some(amount) } else { None },
        });
    }
}

fn build_attention_message(entry: &EconomyLedgerEntry, amount: f64, status: &str) -> String {
    let currency = entry
        .currency
        .clone()
        .unwrap_or_else(|| "unitless".to_string());
    let mut parts = vec![
        capitalise(status),
        format!("{currency} {:.2}", (amount * 100.0).round() / 100.0),
    ];
    if let Some(job) = &entry.job_id {
        parts.push(format!("job {}", job));
    }
    parts.join(" • ")
}

fn capitalise(status: &str) -> String {
    let mut chars = status.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

fn contribution_to_entry(value: &Value) -> Option<EconomyLedgerEntry> {
    let id = value.get("id")?;
    let id_string = match id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };

    let meta_value = value.get("meta").cloned().unwrap_or_else(|| json!({}));
    let meta_obj = meta_value.as_object();

    let mut entry = EconomyLedgerEntry {
        id: id_string,
        issued_at: as_str(value, "time"),
        job_id: meta_obj
            .and_then(|m| m.get("job_id"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| as_str(value, "corr_id")),
        persona_id: meta_obj
            .and_then(|m| m.get("persona_id"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| as_str(value, "subject")),
        contract_id: meta_obj
            .and_then(|m| m.get("contract_id").or_else(|| m.get("contract")))
            .and_then(|v| v.as_str())
            .map(String::from),
        settled_at: meta_obj
            .and_then(|m| m.get("settled_at"))
            .and_then(|v| v.as_str())
            .map(String::from),
        status: meta_obj
            .and_then(|m| {
                m.get("status")
                    .or_else(|| m.get("state"))
                    .and_then(|v| v.as_str())
            })
            .map(String::from)
            .or_else(|| infer_status(value)),
        ..EconomyLedgerEntry::default()
    };

    entry.currency = meta_obj
        .and_then(|m| {
            m.get("currency")
                .or_else(|| m.get("unit"))
                .and_then(|v| v.as_str())
        })
        .map(String::from)
        .or_else(|| as_str(value, "unit"))
        .or_else(|| Some("unitless".to_string()));

    let qty_amount = value
        .get("qty")
        .and_then(|v| v.as_f64())
        .or_else(|| value.get("qty").and_then(|v| v.as_i64()).map(|v| v as f64));

    entry.gross_amount = meta_obj
        .and_then(|m| {
            m.get("gross_amount")
                .or_else(|| m.get("amount"))
                .and_then(|v| v.as_f64())
        })
        .or(qty_amount);
    entry.net_amount = meta_obj
        .and_then(|m| m.get("net_amount").and_then(|v| v.as_f64()))
        .or(entry.gross_amount);

    entry.stakeholders = parse_stakeholders(meta_obj);

    let mut metadata = Map::new();
    if let Some(obj) = meta_obj {
        for (k, v) in obj {
            metadata.insert(k.clone(), v.clone());
        }
    }
    for (key, maybe_value) in [
        ("kind", value.get("kind")),
        ("subject", value.get("subject")),
        ("unit", value.get("unit")),
        ("qty", value.get("qty")),
        ("corr_id", value.get("corr_id")),
        ("project", value.get("proj")),
    ] {
        if let Some(val) = maybe_value {
            if !val.is_null() {
                metadata.entry(key.to_string()).or_insert(val.clone());
            }
        }
    }
    if !metadata.is_empty() {
        entry.metadata = Some(Value::Object(metadata));
    }

    Some(entry)
}

fn parse_stakeholders(meta_obj: Option<&Map<String, Value>>) -> Vec<EconomyStakeholderShare> {
    let mut out = Vec::new();
    let Some(obj) = meta_obj else {
        return out;
    };
    let Some(array) = obj.get("stakeholders").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in array {
        let mut share = EconomyStakeholderShare::default();
        match item {
            Value::String(id) => {
                share.id = id.clone();
            }
            Value::Object(map) => {
                if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                    share.id = id.to_string();
                } else {
                    continue;
                }
                if let Some(role) = map.get("role").and_then(|v| v.as_str()) {
                    share.role = Some(role.to_string());
                }
                if let Some(value) = map
                    .get("share")
                    .and_then(|v| v.as_f64())
                    .or_else(|| map.get("share").and_then(|v| v.as_i64()).map(|v| v as f64))
                {
                    share.share = Some(value);
                }
                if let Some(value) = map
                    .get("amount")
                    .and_then(|v| v.as_f64())
                    .or_else(|| map.get("amount").and_then(|v| v.as_i64()).map(|v| v as f64))
                {
                    share.amount = Some(value);
                }
            }
            _ => continue,
        }
        out.push(share);
    }
    out
}

fn infer_status(value: &Value) -> Option<String> {
    let kind = value
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if kind.ends_with(".complete") {
        Some("settled".into())
    } else if kind.ends_with(".submit") {
        Some("pending".into())
    } else {
        None
    }
}

fn as_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_runtime::{RuntimeHealth, RuntimeStatus};
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn contributions_map_to_entries_and_totals() {
        let contributions = vec![
            json!({
                "id": 2,
                "time": "2025-10-25T12:00:00.000Z",
                "subject": "local",
                "kind": "task.complete",
                "qty": 5,
                "unit": "credit",
                "meta": {
                    "currency": "USD",
                    "amount": 25.0,
                    "status": "settled",
                    "stakeholders": [
                        {"id": "persona.alpha", "share": 0.6},
                        {"id": "operator", "share": 0.4}
                    ],
                    "contract_id": "contract-1",
                    "job_id": "job-123"
                }
            }),
            json!({
                "id": 1,
                "time": "2025-10-25T11:55:00.000Z",
                "subject": "local",
                "kind": "task.submit",
                "qty": 5,
                "unit": "credit",
                "meta": {
                    "currency": "USD",
                    "amount": 25.0,
                    "status": "pending",
                    "job_id": "job-123"
                }
            }),
        ];
        let summary = build_summary(&contributions);
        assert_eq!(summary.entries.len(), 2);
        assert_eq!(summary.totals.len(), 1);
        let total = &summary.totals[0];
        assert_eq!(total.currency, "USD");
        assert_eq!(total.pending, Some(25.0));
        assert_eq!(total.settled, Some(25.0));
        assert!(!summary.attention.is_empty());
        assert!(summary.attention[0].contains("Pending"));
    }

    #[test]
    fn contribution_without_amount_uses_qty() {
        let contributions = vec![json!({
            "id": 10,
            "time": "2025-10-25T12:05:00.000Z",
            "subject": "local",
            "kind": "task.complete",
            "qty": 3,
            "unit": "task",
            "meta": {}
        })];
        let summary = build_summary(&contributions);
        assert_eq!(summary.entries.len(), 1);
        let entry = &summary.entries[0];
        assert_eq!(entry.gross_amount, Some(3.0));
        assert_eq!(entry.currency.as_deref(), Some("task"));
        assert_eq!(summary.totals[0].settled, Some(3.0));
    }

    #[tokio::test]
    async fn augment_with_runtime_tracks_usage_delta() {
        let bus = arw_events::Bus::new_with_replay(8, 8);
        let runtime = Arc::new(RuntimeRegistry::new(bus));
        let mut status = RuntimeStatus::new("runtime-1", RuntimeState::Ready);
        status.health = Some(RuntimeHealth {
            request_count: Some(5),
            ..Default::default()
        });
        status.updated_at = Utc::now();
        runtime.apply_status(status).await;

        let mut summary = build_summary(&[]);
        let (usage, _alerts) = augment_with_runtime(
            &mut summary,
            &EconomyUsageCounters::default(),
            runtime.clone(),
        )
        .await;

        assert_eq!(usage.runtime_requests.get("runtime-1"), Some(&5));
        assert!(summary
            .entries
            .iter()
            .any(|entry| entry.currency.as_deref() == Some("requests")));
    }
}

