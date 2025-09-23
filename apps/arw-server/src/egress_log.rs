use serde_json::Value;

use crate::util;

#[derive(Clone, Debug)]
pub struct EgressRecord<'a> {
    pub decision: &'a str,
    pub reason: Option<&'a str>,
    pub dest_host: Option<&'a str>,
    pub dest_port: Option<i64>,
    pub protocol: Option<&'a str>,
    pub bytes_in: Option<i64>,
    pub bytes_out: Option<i64>,
    pub corr_id: Option<&'a str>,
    pub project: Option<&'a str>,
    pub meta: Option<&'a Value>,
}

impl<'a> EgressRecord<'a> {
    #[allow(dead_code)]
    pub fn new(decision: &'a str) -> Self {
        Self {
            decision,
            reason: None,
            dest_host: None,
            dest_port: None,
            protocol: None,
            bytes_in: None,
            bytes_out: None,
            corr_id: None,
            project: None,
            meta: None,
        }
    }
}

pub async fn record(
    kernel: Option<&arw_kernel::Kernel>,
    bus: &arw_events::Bus,
    posture: Option<&str>,
    record: &EgressRecord<'_>,
    force: bool,
    emit_event: bool,
) -> Option<i64> {
    let posture_owned = posture
        .map(|s| s.to_string())
        .unwrap_or_else(util::effective_posture);
    let entry = append(kernel, posture_owned.as_str(), record, force).await;
    if emit_event {
        publish(bus, posture_owned.as_str(), entry, record);
    }
    entry
}

fn ledger_enabled() -> bool {
    matches!(
        std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref(),
        Some("1")
    )
}

async fn append(
    kernel: Option<&arw_kernel::Kernel>,
    posture: &str,
    record: &EgressRecord<'_>,
    force: bool,
) -> Option<i64> {
    if !(force || ledger_enabled()) {
        return None;
    }
    let kernel = kernel?;
    match kernel
        .append_egress_async(
            record.decision.to_string(),
            record.reason.map(|s| s.to_string()),
            record.dest_host.map(|s| s.to_string()),
            record.dest_port,
            record.protocol.map(|s| s.to_string()),
            record.bytes_in,
            record.bytes_out,
            record.corr_id.map(|s| s.to_string()),
            record.project.map(|s| s.to_string()),
            Some(posture.to_string()),
            record.meta.cloned(),
        )
        .await
    {
        Ok(id) => Some(id),
        Err(err) => {
            tracing::warn!("egress_log: failed to append ledger entry: {err}");
            None
        }
    }
}

fn publish(
    bus: &arw_events::Bus,
    posture: &str,
    ledger_id: Option<i64>,
    record: &EgressRecord<'_>,
) {
    let mut payload = serde_json::Map::new();
    payload.insert("id".into(), serde_json::json!(ledger_id));
    payload.insert("decision".into(), serde_json::json!(record.decision));
    if let Some(reason) = record.reason {
        payload.insert("reason".into(), serde_json::json!(reason));
    }
    payload.insert("dest_host".into(), serde_json::json!(record.dest_host));
    payload.insert("dest_port".into(), serde_json::json!(record.dest_port));
    payload.insert("protocol".into(), serde_json::json!(record.protocol));
    payload.insert("bytes_in".into(), serde_json::json!(record.bytes_in));
    payload.insert("bytes_out".into(), serde_json::json!(record.bytes_out));
    payload.insert("corr_id".into(), serde_json::json!(record.corr_id));
    payload.insert("proj".into(), serde_json::json!(record.project));
    payload.insert(
        "meta".into(),
        record.meta.cloned().unwrap_or(serde_json::Value::Null),
    );
    payload.insert("posture".into(), serde_json::json!(posture));
    bus.publish(
        arw_topics::TOPIC_EGRESS_LEDGER_APPENDED,
        &serde_json::Value::Object(payload),
    );
}
