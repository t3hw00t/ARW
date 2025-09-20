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
}

struct CapsuleEntry {
    snapshot: CapsuleSnapshot,
    fingerprint: String,
    last_event_ms: u64,
    capsule: arw_protocol::GatingCapsule,
    remaining_hops: Option<u32>,
}

#[derive(Clone)]
pub struct CapsuleStore {
    inner: Arc<Mutex<HashMap<String, CapsuleEntry>>>,
}

pub struct AdoptOutcome {
    pub snapshot: CapsuleSnapshot,
    pub notify: bool,
}

impl CapsuleStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn adopt(&self, capsule: &GatingCapsule, now_ms: u64) -> AdoptOutcome {
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
                entry.fingerprint = fingerprint;
                entry.capsule = capsule.clone();
                entry.remaining_hops = capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1));
                let should_notify = if changed {
                    entry.last_event_ms = now_ms;
                    true
                } else if now_ms.saturating_sub(entry.last_event_ms) >= EVENT_THROTTLE_MS {
                    entry.last_event_ms = now_ms;
                    true
                } else {
                    false
                };
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
                };
                vac.insert(CapsuleEntry {
                    snapshot: snapshot.clone(),
                    fingerprint,
                    last_event_ms: now_ms,
                    capsule: capsule.clone(),
                    remaining_hops: capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1)),
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

    pub async fn replay_all(&self) -> bool {
        let mut guard = self.inner.lock().await;
        let mut capsules: Vec<arw_protocol::GatingCapsule> = Vec::new();
        for entry in guard.values_mut() {
            match entry.remaining_hops {
                Some(0) => continue,
                Some(ref mut hops) => {
                    capsules.push(entry.capsule.clone());
                    *hops = hops.saturating_sub(1);
                    entry.snapshot.remaining_hops = Some(*hops);
                }
                None => continue,
            }
        }
        drop(guard);
        for cap in &capsules {
            arw_core::gating::adopt_capsule(cap);
        }
        !capsules.is_empty()
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
    let adopted = match arw_core::rpu::verify_and_adopt(&capsule) {
        true => true,
        false => {
            publish_failure(state, Some(&capsule.id), "verification failed").await;
            return Err(error_response(
                StatusCode::FORBIDDEN,
                "capsule_verification_failed",
                "Capsule verification failed",
            ));
        }
    };
    if !adopted {
        publish_failure(state, Some(&capsule.id), "verification rejected").await;
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "capsule_rejected",
            "Capsule rejected by guardrails",
        ));
    }
    let now = now_ms();
    let outcome = state.capsules().adopt(&capsule, now).await;
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
            signature: None,
        };
        let raw = serde_json::to_vec(&cap).unwrap();
        let encoded = BASE64_STD.encode(raw);
        let parsed = parse_capsule(&encoded).unwrap();
        assert_eq!(parsed.id, "test");
    }
}
