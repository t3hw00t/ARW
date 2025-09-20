use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine as _;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use arw_protocol::GatingCapsule;

use crate::{read_models, AppState};
use arw_topics::{
    TOPIC_POLICY_CAPSULE_APPLIED, TOPIC_POLICY_CAPSULE_FAILED, TOPIC_POLICY_DECISION,
};

const EVENT_THROTTLE_MS: u64 = 2_000;
const HEADER_NAMES: [&str; 2] = ["X-ARW-Capsule", "X-ARW-Gate"];

#[derive(Clone, Serialize)]
pub struct CapsuleSnapshot {
    pub id: String,
    pub version: String,
    pub issuer: Option<String>,
    pub applied_ms: u64,
    pub hop_ttl: Option<u32>,
    pub denies: usize,
    pub contracts: usize,
    pub remaining_hops: Option<u32>,
    pub lease_until_ms: Option<u64>,
    pub renew_within_ms: Option<u64>,
}

struct CapsuleEntry {
    snapshot: CapsuleSnapshot,
    fingerprint: String,
    last_event_ms: u64,
    capsule: arw_protocol::GatingCapsule,
    remaining_hops: Option<u32>,
    lease_until_ms: Option<u64>,
    renew_within_ms: Option<u64>,
}

#[derive(Clone)]
pub struct CapsuleStore {
    inner: Arc<Mutex<HashMap<String, CapsuleEntry>>>,
}

pub struct AdoptOutcome {
    pub snapshot: CapsuleSnapshot,
    pub notify: bool,
}

pub struct ReplayOutcome {
    pub expired: Vec<CapsuleSnapshot>,
}

impl CapsuleStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn adopt(
        &self,
        capsule: &GatingCapsule,
        now_ms: u64,
        lease: arw_core::gating::CapsuleLeaseState,
    ) -> AdoptOutcome {
        let fingerprint = fingerprint_capsule(capsule);
        let mut guard = self.inner.lock().await;
        match guard.entry(capsule.id.clone()) {
            Entry::Occupied(mut occ) => {
                let entry = occ.get_mut();
                let changed = entry.fingerprint != fingerprint
                    || entry.snapshot.version != capsule.version
                    || entry.snapshot.issuer != capsule.issuer;
                entry.snapshot.version = capsule.version.clone();
                entry.snapshot.issuer = capsule.issuer.clone();
                entry.snapshot.hop_ttl = capsule.hop_ttl;
                entry.snapshot.denies = capsule.denies.len();
                entry.snapshot.contracts = capsule.contracts.len();
                entry.snapshot.applied_ms = now_ms;
                entry.snapshot.remaining_hops = capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1));
                entry.snapshot.lease_until_ms = lease.lease_until_ms;
                entry.snapshot.renew_within_ms = lease.renew_within_ms;
                entry.fingerprint = fingerprint;
                entry.capsule = capsule.clone();
                entry.remaining_hops = capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1));
                entry.lease_until_ms = lease.lease_until_ms;
                entry.renew_within_ms = lease.renew_within_ms;
                let should_notify =
                    changed || now_ms.saturating_sub(entry.last_event_ms) >= EVENT_THROTTLE_MS;
                if should_notify {
                    entry.last_event_ms = now_ms;
                }
                AdoptOutcome {
                    snapshot: entry.snapshot.clone(),
                    notify: should_notify,
                }
            }
            Entry::Vacant(vac) => {
                let snapshot = CapsuleSnapshot {
                    id: capsule.id.clone(),
                    version: capsule.version.clone(),
                    issuer: capsule.issuer.clone(),
                    applied_ms: now_ms,
                    hop_ttl: capsule.hop_ttl,
                    denies: capsule.denies.len(),
                    contracts: capsule.contracts.len(),
                    remaining_hops: capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1)),
                    lease_until_ms: lease.lease_until_ms,
                    renew_within_ms: lease.renew_within_ms,
                };
                vac.insert(CapsuleEntry {
                    snapshot: snapshot.clone(),
                    fingerprint,
                    last_event_ms: now_ms,
                    capsule: capsule.clone(),
                    remaining_hops: capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1)),
                    lease_until_ms: lease.lease_until_ms,
                    renew_within_ms: lease.renew_within_ms,
                });
                AdoptOutcome {
                    snapshot,
                    notify: true,
                }
            }
        }
    }

    pub async fn snapshot(&self) -> serde_json::Value {
        let guard = self.inner.lock().await;
        let mut items: Vec<CapsuleSnapshot> = guard.values().map(|e| e.snapshot.clone()).collect();
        items.sort_by(|a, b| b.applied_ms.cmp(&a.applied_ms));
        json!({
            "items": items,
            "count": items.len(),
        })
    }

    pub async fn replay_all(&self) -> ReplayOutcome {
        let now = now_ms();
        let mut guard = self.inner.lock().await;
        let mut apply: Vec<(String, arw_protocol::GatingCapsule)> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut expired: Vec<CapsuleSnapshot> = Vec::new();
        for (id, entry) in guard.iter_mut() {
            if let Some(expire) = entry.lease_until_ms {
                if now >= expire {
                    to_remove.push(id.clone());
                    expired.push(entry.snapshot.clone());
                    continue;
                }
            }

            let mut should_apply = false;
            match entry.remaining_hops {
                Some(0) => {}
                Some(ref mut hops) => {
                    should_apply = true;
                    *hops = hops.saturating_sub(1);
                    entry.snapshot.remaining_hops = Some(*hops);
                }
                None => {
                    if let Some(expire) = entry.lease_until_ms {
                        if let Some(window) = entry.renew_within_ms {
                            if expire.saturating_sub(now) <= window {
                                should_apply = true;
                            }
                        }
                    }
                }
            }
            if should_apply {
                apply.push((id.clone(), entry.capsule.clone()));
            }
        }
        for id in to_remove {
            guard.remove(&id);
        }
        drop(guard);

        if apply.is_empty() {
            return ReplayOutcome { expired };
        }

        let mut leases: HashMap<String, arw_core::gating::CapsuleLeaseState> = HashMap::new();
        for (id, capsule) in &apply {
            let lease = arw_core::gating::adopt_capsule(capsule);
            leases.insert(id.clone(), lease);
        }

        let mut guard = self.inner.lock().await;
        for (id, _) in apply.into_iter() {
            if let Some(entry) = guard.get_mut(&id) {
                if let Some(lease) = leases.get(&id) {
                    entry.snapshot.applied_ms = now;
                    entry.snapshot.lease_until_ms = lease.lease_until_ms;
                    entry.snapshot.renew_within_ms = lease.renew_within_ms;
                    entry.lease_until_ms = lease.lease_until_ms;
                    entry.renew_within_ms = lease.renew_within_ms;
                    entry.last_event_ms = now;
                }
            }
        }
        ReplayOutcome { expired }
    }
}

pub async fn capsule_mw(state: AppState, req: Request<Body>, next: Next) -> Response {
    match apply_capsule(&state, req.headers()).await {
        Ok(_) => next.run(req).await,
        Err(resp) => resp,
    }
}

async fn apply_capsule(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let raw = match extract_header(headers) {
        Some(v) => v,
        None => return Ok(()),
    };
    let capsule = match parse_capsule(&raw) {
        Ok(cap) => cap,
        Err(kind) => {
            let resp = error_response(
                StatusCode::BAD_REQUEST,
                "invalid_capsule",
                kind.as_message(),
            );
            publish_failure(state, None, kind.as_message()).await;
            return Err(resp);
        }
    };
    let lease = match arw_core::rpu::verify_and_adopt(&capsule) {
        Some(lease) => lease,
        None => {
            publish_failure(state, Some(&capsule.id), "verification failed").await;
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "capsule_verification_failed",
                "Capsule verification failed",
            ));
        }
    };
    let now = now_ms();
    let outcome = state.capsules().adopt(&capsule, now, lease).await;
    if outcome.notify {
        state.bus().publish(
            TOPIC_POLICY_CAPSULE_APPLIED,
            &json!({
                "id": outcome.snapshot.id,
                "version": outcome.snapshot.version,
                "issuer": outcome.snapshot.issuer,
                "applied_ms": outcome.snapshot.applied_ms,
                "hop_ttl": outcome.snapshot.hop_ttl,
                "denies": outcome.snapshot.denies,
                "contracts": outcome.snapshot.contracts,
                "lease_until_ms": outcome.snapshot.lease_until_ms,
                "renew_within_ms": outcome.snapshot.renew_within_ms,
            }),
        );
        let snapshot = state.capsules().snapshot().await;
        read_models::publish_read_model_patch(&state.bus(), "policy_capsules", &snapshot);
    }
    Ok(())
}

fn extract_header(headers: &HeaderMap) -> Option<String> {
    HEADER_NAMES.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    })
}

fn parse_capsule(raw: &str) -> Result<GatingCapsule, CapsuleParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CapsuleParseError::Empty);
    }
    if let Ok(cap) = serde_json::from_str::<GatingCapsule>(trimmed) {
        return Ok(cap);
    }
    if let Ok(decoded) = BASE64_STD.decode(trimmed) {
        if let Ok(text) = std::str::from_utf8(&decoded) {
            if let Ok(cap) = serde_json::from_str::<GatingCapsule>(text) {
                return Ok(cap);
            }
        }
    }
    Err(CapsuleParseError::Parse)
}

#[derive(Debug, Clone, Copy)]
enum CapsuleParseError {
    Empty,
    Parse,
}

impl CapsuleParseError {
    fn as_message(self) -> &'static str {
        match self {
            CapsuleParseError::Empty => "Capsule header was empty",
            CapsuleParseError::Parse => "Capsule header could not be decoded",
        }
    }
}

fn error_response(status: StatusCode, code: &str, detail: &str) -> Response {
    (
        status,
        Json(json!({
            "type": "about:blank",
            "title": "Capsule rejected",
            "status": status.as_u16(),
            "code": code,
            "detail": detail,
        })),
    )
        .into_response()
}

async fn publish_failure(state: &AppState, capsule_id: Option<&str>, detail: &str) {
    state.bus().publish(
        TOPIC_POLICY_CAPSULE_FAILED,
        &json!({
            "id": capsule_id,
            "detail": detail,
        }),
    );
    state.bus().publish(
        TOPIC_POLICY_DECISION,
        &json!({
            "action": "policy.capsule",
            "allow": false,
            "explain": {"detail": detail, "capsule_id": capsule_id},
        }),
    );
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn fingerprint_capsule(cap: &GatingCapsule) -> String {
    let bytes = serde_json::to_vec(cap).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_header() {
        let cap = GatingCapsule {
            id: "test".into(),
            version: "1".into(),
            issued_at_ms: 42,
            issuer: Some("issuer".into()),
            hop_ttl: None,
            propagate: None,
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: None,
            renew_within_ms: None,
            signature: None,
        };
        let raw = serde_json::to_string(&cap).unwrap();
        let parsed = parse_capsule(&raw).unwrap();
        assert_eq!(parsed.id, "test");
    }

    #[test]
    fn parse_base64_header() {
        let cap = GatingCapsule {
            id: "test".into(),
            version: "1".into(),
            issued_at_ms: 42,
            issuer: Some("issuer".into()),
            hop_ttl: None,
            propagate: None,
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: None,
            renew_within_ms: None,
            signature: None,
        };
        let raw = serde_json::to_vec(&cap).unwrap();
        let encoded = BASE64_STD.encode(raw);
        let parsed = parse_capsule(&encoded).unwrap();
        assert_eq!(parsed.id, "test");
    }
}
