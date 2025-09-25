use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine as _;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::{
    sync::Mutex,
    time::{interval, Duration, MissedTickBehavior},
};

use arw_core::gating::CapsuleLeaseState;
use arw_protocol::GatingCapsule;

use crate::{read_models, tasks::TaskHandle, AppState};
use arw_topics::{
    TOPIC_POLICY_CAPSULE_APPLIED, TOPIC_POLICY_CAPSULE_EXPIRED, TOPIC_POLICY_CAPSULE_FAILED,
    TOPIC_POLICY_DECISION,
};

const EVENT_THROTTLE_MS: u64 = 2_000;
const LEGACY_HEADER_DETAIL: &str =
    "Legacy X-ARW-Gate header is no longer supported; send X-ARW-Capsule instead";
static CURRENT_HEADER_NAME: HeaderName = HeaderName::from_static("x-arw-capsule");
static LEGACY_HEADER_NAME: HeaderName = HeaderName::from_static("x-arw-gate");
const DEFAULT_REFRESH_SECS: u64 = 5;
const MIN_REFRESH_SECS: u64 = 1;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
enum CapsuleHeader<'a> {
    None,
    Current(&'a str),
    Legacy(&'a str),
}

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
    capsule: GatingCapsule,
    remaining_hops: Option<u32>,
    lease_until_ms: Option<u64>,
    renew_within_ms: Option<u64>,
}

impl CapsuleSnapshot {
    fn from_capsule(
        capsule: &GatingCapsule,
        now_ms: u64,
        lease: &CapsuleLeaseState,
        remaining_hops: Option<u32>,
    ) -> Self {
        Self {
            id: capsule.id.clone(),
            version: capsule.version.clone(),
            issuer: capsule.issuer.clone(),
            applied_ms: now_ms,
            hop_ttl: capsule.hop_ttl,
            denies: capsule.denies.len(),
            contracts: capsule.contracts.len(),
            remaining_hops,
            lease_until_ms: lease.lease_until_ms,
            renew_within_ms: lease.renew_within_ms,
        }
    }

    fn refresh_from_capsule(
        &mut self,
        capsule: &GatingCapsule,
        now_ms: u64,
        lease: &CapsuleLeaseState,
        remaining_hops: Option<u32>,
    ) {
        self.version.clone_from(&capsule.version);
        self.issuer.clone_from(&capsule.issuer);
        self.hop_ttl = capsule.hop_ttl;
        self.denies = capsule.denies.len();
        self.contracts = capsule.contracts.len();
        self.applied_ms = now_ms;
        self.remaining_hops = remaining_hops;
        self.lease_until_ms = lease.lease_until_ms;
        self.renew_within_ms = lease.renew_within_ms;
    }
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
    pub changed: bool,
    pub reapplied: Vec<CapsuleSnapshot>,
}

impl CapsuleStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn adopt(&self, capsule: &GatingCapsule, now_ms: u64) -> AdoptOutcome {
        let lease = arw_core::gating::adopt_capsule(capsule);
        let fingerprint = fingerprint_capsule(capsule);
        let remaining_hops = remaining_hops_after_adopt(capsule);
        let mut guard = self.inner.lock().await;
        match guard.entry(capsule.id.clone()) {
            Entry::Occupied(mut occ) => {
                let entry = occ.get_mut();
                let changed = entry.fingerprint != fingerprint
                    || entry.snapshot.version != capsule.version
                    || entry.snapshot.issuer != capsule.issuer;
                entry
                    .snapshot
                    .refresh_from_capsule(capsule, now_ms, &lease, remaining_hops);
                entry.fingerprint = fingerprint;
                entry.capsule.clone_from(capsule);
                entry.remaining_hops = remaining_hops;
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
                let snapshot =
                    CapsuleSnapshot::from_capsule(capsule, now_ms, &lease, remaining_hops);
                vac.insert(CapsuleEntry {
                    snapshot: snapshot.clone(),
                    fingerprint,
                    last_event_ms: now_ms,
                    capsule: capsule.clone(),
                    remaining_hops,
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
        let mut apply: Vec<(String, GatingCapsule)> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut expired: Vec<CapsuleSnapshot> = Vec::new();
        let mut changed = false;
        for (id, entry) in guard.iter_mut() {
            if let Some(expire) = entry.lease_until_ms {
                if now >= expire {
                    to_remove.push(id.clone());
                    expired.push(entry.snapshot.clone());
                    changed = true;
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
                    changed = true;
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
                changed = true;
            }
        }
        for id in to_remove {
            guard.remove(&id);
        }
        drop(guard);

        if apply.is_empty() {
            return ReplayOutcome {
                expired,
                changed,
                reapplied: Vec::new(),
            };
        }

        let mut leases: HashMap<String, CapsuleLeaseState> = HashMap::new();
        for (id, capsule) in &apply {
            let lease = arw_core::gating::adopt_capsule(capsule);
            leases.insert(id.clone(), lease);
        }

        let mut guard = self.inner.lock().await;
        let mut reapplied: Vec<CapsuleSnapshot> = Vec::with_capacity(apply.len());
        for (id, _) in apply.into_iter() {
            if let Some(entry) = guard.get_mut(&id) {
                if let Some(lease) = leases.get(&id) {
                    entry.snapshot.applied_ms = now;
                    entry.snapshot.lease_until_ms = lease.lease_until_ms;
                    entry.snapshot.renew_within_ms = lease.renew_within_ms;
                    entry.lease_until_ms = lease.lease_until_ms;
                    entry.renew_within_ms = lease.renew_within_ms;
                    entry.last_event_ms = now;
                    reapplied.push(entry.snapshot.clone());
                }
            }
        }
        ReplayOutcome {
            expired,
            changed,
            reapplied,
        }
    }
}

pub async fn refresh_capsules(state: &AppState) -> ReplayOutcome {
    let replay = state.capsules().replay_all().await;
    if !replay.expired.is_empty() {
        let expired_ms = now_ms();
        for snapshot in &replay.expired {
            state.bus().publish(
                TOPIC_POLICY_CAPSULE_EXPIRED,
                &json!({
                    "id": snapshot.id,
                    "version": snapshot.version,
                    "issuer": snapshot.issuer,
                    "expired_ms": expired_ms,
                    "applied_ms": snapshot.applied_ms,
                    "lease_until_ms": snapshot.lease_until_ms,
                }),
            );
        }
    }
    if replay.changed {
        let snapshot = state.capsules().snapshot().await;
        read_models::publish_read_model_patch(&state.bus(), "policy_capsules", &snapshot);
    }
    if !replay.reapplied.is_empty() {
        for snapshot in &replay.reapplied {
            state.bus().publish(
                TOPIC_POLICY_CAPSULE_APPLIED,
                &json!({
                    "id": snapshot.id,
                    "version": snapshot.version,
                    "issuer": snapshot.issuer,
                    "applied_ms": snapshot.applied_ms,
                    "hop_ttl": snapshot.hop_ttl,
                    "denies": snapshot.denies,
                    "contracts": snapshot.contracts,
                    "lease_until_ms": snapshot.lease_until_ms,
                    "renew_within_ms": snapshot.renew_within_ms,
                    "renewal": true,
                }),
            );
        }
    }
    replay
}

fn refresh_interval_secs() -> u64 {
    std::env::var("ARW_CAPSULE_REFRESH_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(|secs| secs.max(MIN_REFRESH_SECS))
        .unwrap_or(DEFAULT_REFRESH_SECS)
}

fn refresh_interval() -> Duration {
    Duration::from_secs(refresh_interval_secs())
}

pub fn start_refresh_task(state: AppState) -> TaskHandle {
    let mut ticker = interval(refresh_interval());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    TaskHandle::new(
        "capsules.refresh",
        tokio::spawn(async move {
            let initial = refresh_capsules(&state).await;
            if initial.changed || !initial.expired.is_empty() {
                tracing::debug!(
                    target: "arw::policy",
                    expired = initial.expired.len(),
                    changed = initial.changed,
                    "capsule refresh sweep applied",
                );
            }

            loop {
                ticker.tick().await;
                let outcome = refresh_capsules(&state).await;
                if outcome.changed || !outcome.expired.is_empty() {
                    tracing::debug!(
                        target: "arw::policy",
                        expired = outcome.expired.len(),
                        changed = outcome.changed,
                        "capsule refresh sweep applied",
                    );
                }
            }
        }),
    )
}

pub async fn capsule_mw(state: AppState, req: Request<Body>, next: Next) -> Response {
    match apply_capsule(&state, req.headers()).await {
        Ok(_) => next.run(req).await,
        Err(resp) => resp,
    }
}

async fn apply_capsule(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let raw = match extract_header(headers) {
        CapsuleHeader::None => return Ok(()),
        CapsuleHeader::Current(v) => v,
        CapsuleHeader::Legacy(raw) => {
            state.metrics().record_legacy_capsule_header();
            let capsule_id = parse_capsule(raw).ok().map(|cap| cap.id);
            if let Some(id) = capsule_id.as_deref() {
                tracing::warn!(target: "arw::policy", capsule_id = id, "legacy capsule header rejected");
            } else {
                tracing::warn!(target: "arw::policy", "legacy capsule header rejected (unparseable id)");
            }
            publish_failure(state, capsule_id.as_deref(), LEGACY_HEADER_DETAIL).await;
            return Err(error_response(
                StatusCode::GONE,
                "capsule_header_legacy",
                LEGACY_HEADER_DETAIL,
            ));
        }
    };
    let capsule = match parse_capsule(raw) {
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
    if !arw_core::rpu::verify_capsule(&capsule) {
        publish_failure(state, Some(&capsule.id), "verification failed").await;
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "capsule_verification_failed",
            "Capsule verification failed",
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
                "lease_until_ms": outcome.snapshot.lease_until_ms,
                "renew_within_ms": outcome.snapshot.renew_within_ms,
            }),
        );
        let snapshot = state.capsules().snapshot().await;
        read_models::publish_read_model_patch(&state.bus(), "policy_capsules", &snapshot);
    }
    Ok(())
}

fn extract_header(headers: &HeaderMap) -> CapsuleHeader<'_> {
    if let Some(value) = header_as_str(headers, &CURRENT_HEADER_NAME) {
        return CapsuleHeader::Current(value);
    }
    if let Some(value) = header_as_str(headers, &LEGACY_HEADER_NAME) {
        return CapsuleHeader::Legacy(value);
    }
    CapsuleHeader::None
}

fn header_as_str<'a>(headers: &'a HeaderMap, name: &'static HeaderName) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn remaining_hops_after_adopt(capsule: &GatingCapsule) -> Option<u32> {
    capsule.hop_ttl.and_then(|ttl| ttl.checked_sub(1))
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
    // Ignore signature bytes when fingerprinting so re-signed capsules with
    // identical policy payloads do not trigger redundant updates.
    let mut clean = cap.clone();
    clean.signature = None;
    let bytes = serde_json::to_vec(&clean).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arw_policy::PolicyEngine;
    use arw_topics::{TOPIC_POLICY_CAPSULE_FAILED, TOPIC_POLICY_DECISION, TOPIC_READMODEL_PATCH};
    use axum::http::{HeaderMap, HeaderValue};
    use once_cell::sync::Lazy;
    use std::{
        path::Path,
        sync::{Arc, Mutex as StdMutex},
    };
    use tempfile::tempdir;
    use tokio::time::{timeout, Duration};

    static ENV_LOCK: Lazy<StdMutex<()>> = Lazy::new(|| StdMutex::new(()));

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

    #[test]
    fn extract_current_header_trims_value() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CURRENT_HEADER_NAME.clone(),
            HeaderValue::from_static("  {\"id\":\"abc\"}  "),
        );
        match extract_header(&headers) {
            CapsuleHeader::Current(raw) => assert_eq!(raw, "{\"id\":\"abc\"}"),
            other => panic!("unexpected match: {:?}", other),
        }
    }

    #[test]
    fn extract_legacy_header_detects() {
        let mut headers = HeaderMap::new();
        headers.insert(LEGACY_HEADER_NAME.clone(), HeaderValue::from_static("{}"));
        match extract_header(&headers) {
            CapsuleHeader::Legacy(raw) => assert_eq!(raw, "{}"),
            other => panic!("unexpected match: {:?}", other),
        }
    }

    #[test]
    fn extract_prefers_current_over_legacy() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CURRENT_HEADER_NAME.clone(),
            HeaderValue::from_static("current"),
        );
        headers.insert(
            LEGACY_HEADER_NAME.clone(),
            HeaderValue::from_static("legacy"),
        );
        match extract_header(&headers) {
            CapsuleHeader::Current(raw) => assert_eq!(raw, "current"),
            other => panic!("unexpected match: {:?}", other),
        }
    }

    async fn build_state(dir: &Path) -> AppState {
        std::env::set_var("ARW_DEBUG", "1");
        std::env::set_var("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_arc = Arc::new(Mutex::new(policy));
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_arc, host, true)
            .with_config_state(Arc::new(Mutex::new(serde_json::json!({"mode": "test"}))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    fn sample_capsule(id: &str) -> GatingCapsule {
        GatingCapsule {
            id: id.to_string(),
            version: "1".into(),
            issued_at_ms: 0,
            issuer: Some("issuer".into()),
            hop_ttl: Some(1),
            propagate: None,
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: None,
            renew_within_ms: None,
            signature: Some("sig".into()),
        }
    }

    fn capsule_with_hops(id: &str, ttl: u32) -> GatingCapsule {
        GatingCapsule {
            hop_ttl: Some(ttl),
            lease_duration_ms: Some(60_000),
            renew_within_ms: Some(10_000),
            signature: None,
            ..sample_capsule(id)
        }
    }

    #[tokio::test]
    async fn replay_all_marks_changed_on_hop_decrement() {
        let store = CapsuleStore::new();
        let now = now_ms();
        let capsule = capsule_with_hops("hop-test", 3);

        store.adopt(&capsule, now).await;

        let replay = store.replay_all().await;
        assert!(replay.changed);
        assert!(replay.expired.is_empty());
        assert_eq!(replay.reapplied.len(), 1);

        let snapshot = store.snapshot().await;
        let items = snapshot["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["remaining_hops"].as_u64(), Some(1));
    }

    #[tokio::test]
    async fn refresh_capsules_publishes_patch_on_state_change() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;

        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                TOPIC_POLICY_CAPSULE_APPLIED.to_string(),
                TOPIC_READMODEL_PATCH.to_string(),
            ],
            Some(8),
        );

        let capsule = capsule_with_hops("refresh-test", 3);
        state.capsules().adopt(&capsule, now_ms()).await;

        let replay = refresh_capsules(&state).await;
        assert!(replay.changed);
        assert!(replay.expired.is_empty());
        assert_eq!(replay.reapplied.len(), 1);

        let applied = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("applied event available")
            .expect("bus open");
        assert_eq!(applied.kind, TOPIC_POLICY_CAPSULE_APPLIED);
        assert_eq!(applied.payload["id"].as_str(), Some("refresh-test"));
        assert_eq!(applied.payload["renewal"].as_bool(), Some(true));

        let event = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("patch event available")
            .expect("bus open");
        assert_eq!(event.kind, TOPIC_READMODEL_PATCH);
        assert_eq!(event.payload["id"].as_str(), Some("policy_capsules"));
    }

    #[test]
    fn refresh_interval_respects_env_and_floor() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");

        std::env::remove_var("ARW_CAPSULE_REFRESH_SECS");
        assert_eq!(refresh_interval_secs(), DEFAULT_REFRESH_SECS);

        std::env::set_var("ARW_CAPSULE_REFRESH_SECS", "0");
        assert_eq!(refresh_interval_secs(), MIN_REFRESH_SECS);

        std::env::set_var("ARW_CAPSULE_REFRESH_SECS", "7");
        assert_eq!(refresh_interval_secs(), 7);

        std::env::set_var("ARW_CAPSULE_REFRESH_SECS", "not-a-number");
        assert_eq!(refresh_interval_secs(), DEFAULT_REFRESH_SECS);

        std::env::remove_var("ARW_CAPSULE_REFRESH_SECS");
    }

    #[tokio::test]
    async fn legacy_header_returns_gone_and_emits_failure_events() {
        let temp = tempdir().expect("tempdir");
        let state = build_state(temp.path()).await;
        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                TOPIC_POLICY_CAPSULE_FAILED.to_string(),
                TOPIC_POLICY_DECISION.to_string(),
            ],
            Some(8),
        );

        let mut headers = HeaderMap::new();
        let cap = sample_capsule("legacy-test");
        let raw = serde_json::to_string(&cap).unwrap();
        headers.insert(
            LEGACY_HEADER_NAME.clone(),
            HeaderValue::from_str(&raw).unwrap(),
        );

        let result = apply_capsule(&state, &headers).await;
        let response = result.expect_err("legacy header should be rejected");
        assert_eq!(response.status(), StatusCode::GONE);

        let first = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("event available")
            .expect("bus not closed");
        assert_eq!(first.kind, TOPIC_POLICY_CAPSULE_FAILED);
        assert_eq!(first.payload["detail"].as_str(), Some(LEGACY_HEADER_DETAIL));
        assert_eq!(first.payload["id"].as_str(), Some("legacy-test"));

        let second = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("second event available")
            .expect("bus not closed");
        assert_eq!(second.kind, TOPIC_POLICY_DECISION);
        assert_eq!(second.payload["allow"].as_bool(), Some(false));
        assert_eq!(
            second.payload["explain"]["detail"].as_str(),
            Some(LEGACY_HEADER_DETAIL)
        );
    }
}
