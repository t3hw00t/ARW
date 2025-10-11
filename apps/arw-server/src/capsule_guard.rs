use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine as _;
use chrono::{SecondsFormat, TimeZone, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::convert::TryFrom;
use tokio::{sync::Mutex, time::Duration};

use arw_core::gating::CapsuleLeaseState;
use arw_protocol::GatingCapsule;

use crate::{
    read_models, request_ctx, request_ctx::RequestCorrelation, tasks::TaskHandle, AppState,
};
use arw_topics::{
    TOPIC_POLICY_CAPSULE_APPLIED, TOPIC_POLICY_CAPSULE_EXPIRED, TOPIC_POLICY_CAPSULE_FAILED,
    TOPIC_POLICY_CAPSULE_TEARDOWN, TOPIC_POLICY_DECISION,
};

pub const CAPSULE_EXPIRING_SOON_WINDOW_MS: u64 = 60_000;
const EVENT_THROTTLE_MS: u64 = 2_000;
const LEGACY_HEADER_DETAIL: &str =
    "Legacy X-ARW-Gate header is no longer supported; send X-ARW-Capsule instead";
static CURRENT_HEADER_NAME: HeaderName = HeaderName::from_static("x-arw-capsule");
static LEGACY_HEADER_NAME: HeaderName = HeaderName::from_static("x-arw-gate");
const DEFAULT_REFRESH_SECS: u64 = 5;
const MIN_REFRESH_MS: u64 = 50;
const HOP_TICK_MS: u64 = 1_000;

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

struct CapsuleStatusInfo {
    status: &'static str,
    status_label: String,
    aria_hint: String,
    expires_in_ms: Option<u64>,
    renew_in_ms: Option<u64>,
    renew_window_start_ms: Option<u64>,
    renew_window_started: bool,
    expired_since_ms: Option<u64>,
}

pub struct CapsuleTeardownOutcome {
    pub removed: Vec<Value>,
    pub not_found: Vec<String>,
    pub remaining: usize,
    pub dry_run: bool,
    pub reason: Option<String>,
}

pub struct CapsuleTeardownSpec<'a> {
    pub selection: CapsuleTeardownSelection<'a>,
    pub reason: Option<&'a str>,
    pub dry_run: bool,
}

pub enum CapsuleTeardownSelection<'a> {
    All,
    Ids(&'a [String]),
}

pub(crate) struct CapsuleStoreTeardown {
    removed: Vec<CapsuleSnapshot>,
    not_found: Vec<String>,
    remaining: usize,
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

    fn to_json(&self, now_ms: u64) -> Value {
        let info = classify_capsule(self, now_ms);
        let CapsuleStatusInfo {
            status,
            status_label,
            aria_hint,
            expires_in_ms,
            renew_in_ms,
            renew_window_start_ms,
            renew_window_started,
            expired_since_ms,
        } = info;

        let mut obj = serde_json::Map::new();
        obj.insert("id".into(), Value::String(self.id.clone()));
        obj.insert("version".into(), Value::String(self.version.clone()));
        if let Some(issuer) = &self.issuer {
            obj.insert("issuer".into(), Value::String(issuer.clone()));
        }
        obj.insert("applied_ms".into(), Value::Number(self.applied_ms.into()));
        if let Some(applied_iso) = ms_to_rfc3339(self.applied_ms) {
            obj.insert("applied".into(), Value::String(applied_iso));
        }
        if let Some(hop_ttl) = self.hop_ttl {
            obj.insert("hop_ttl".into(), Value::Number(u64::from(hop_ttl).into()));
        }
        obj.insert("denies".into(), Value::Number((self.denies as u64).into()));
        obj.insert(
            "contracts".into(),
            Value::Number((self.contracts as u64).into()),
        );
        if let Some(remaining) = self.remaining_hops {
            obj.insert(
                "remaining_hops".into(),
                Value::Number(u64::from(remaining).into()),
            );
        }
        if let Some(lease_until) = self.lease_until_ms {
            obj.insert("lease_until_ms".into(), Value::Number(lease_until.into()));
            if let Some(lease_iso) = ms_to_rfc3339(lease_until) {
                obj.insert("lease_until".into(), Value::String(lease_iso));
            }
        }
        if let Some(renew_within) = self.renew_within_ms {
            obj.insert("renew_within_ms".into(), Value::Number(renew_within.into()));
        }

        obj.insert("status".into(), Value::String(status.to_string()));
        obj.insert("status_label".into(), Value::String(status_label));
        obj.insert("aria_hint".into(), Value::String(aria_hint));
        if let Some(expires) = expires_in_ms {
            obj.insert("expires_in_ms".into(), Value::Number(expires.into()));
        }
        if let Some(expired_since) = expired_since_ms {
            obj.insert(
                "expired_since_ms".into(),
                Value::Number(expired_since.into()),
            );
        }
        if let Some(renew_in) = renew_in_ms {
            obj.insert("renew_in_ms".into(), Value::Number(renew_in.into()));
        }
        obj.insert(
            "renew_window_started".into(),
            Value::Bool(renew_window_started),
        );
        if let Some(renew_start) = renew_window_start_ms {
            obj.insert(
                "renew_window_start_ms".into(),
                Value::Number(renew_start.into()),
            );
            if let Some(renew_iso) = ms_to_rfc3339(renew_start) {
                obj.insert("renew_window_start".into(), Value::String(renew_iso));
            }
        }

        Value::Object(obj)
    }
}

#[derive(Clone)]
pub struct CapsuleStore {
    inner: Arc<Mutex<HashMap<String, CapsuleEntry>>>,
    last_refresh_ms: Arc<AtomicU64>,
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
            last_refresh_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn adopt(&self, capsule: &GatingCapsule, now_ms: u64) -> AdoptOutcome {
        let lease = arw_core::gating::adopt_capsule(capsule);
        let fingerprint = fingerprint_capsule(capsule);
        let remaining_hops = remaining_hops_after_adopt(capsule);
        let mut guard = self.inner.lock().await;
        let outcome = match guard.entry(capsule.id.clone()) {
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
        };
        self.last_refresh_ms.store(now_ms, Ordering::Relaxed);
        outcome
    }

    pub async fn snapshot(&self) -> serde_json::Value {
        let guard = self.inner.lock().await;
        let now_ms = now_ms();
        let mut items: Vec<(u64, Value)> = guard
            .values()
            .map(|entry| (entry.snapshot.applied_ms, entry.snapshot.to_json(now_ms)))
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0));
        let items: Vec<Value> = items.into_iter().map(|(_, value)| value).collect();
        let generated_iso = ms_to_rfc3339(now_ms)
            .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        json!({
            "items": items,
            "count": guard.len(),
            "generated_ms": now_ms,
            "generated": generated_iso,
        })
    }

    pub(crate) async fn teardown(
        &self,
        selection: &CapsuleTeardownSelection<'_>,
        dry_run: bool,
    ) -> CapsuleStoreTeardown {
        let mut guard = self.inner.lock().await;
        let mut removed: Vec<CapsuleSnapshot> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();

        match selection {
            CapsuleTeardownSelection::All => {
                removed.extend(guard.values().map(|entry| entry.snapshot.clone()));
                if !dry_run {
                    guard.clear();
                }
            }
            CapsuleTeardownSelection::Ids(ids) => {
                let mut seen: HashSet<&str> = HashSet::new();
                for id in ids.iter() {
                    if !seen.insert(id.as_str()) {
                        continue;
                    }
                    if dry_run {
                        match guard.get(id.as_str()) {
                            Some(entry) => removed.push(entry.snapshot.clone()),
                            None => not_found.push(id.clone()),
                        }
                    } else {
                        match guard.remove(id.as_str()) {
                            Some(entry) => removed.push(entry.snapshot),
                            None => not_found.push(id.clone()),
                        }
                    }
                }
            }
        }

        let remaining = guard.len();
        CapsuleStoreTeardown {
            removed,
            not_found,
            remaining,
        }
    }

    pub async fn replay_all(&self) -> ReplayOutcome {
        let now = now_ms();
        let mut guard = self.inner.lock().await;
        let mut apply: Vec<(String, GatingCapsule)> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut expired: Vec<CapsuleSnapshot> = Vec::new();
        let mut changed = false;
        for (id, entry) in guard.iter_mut() {
            let mut should_apply = false;
            let mut expired_now = false;

            if let Some(expire) = entry.lease_until_ms {
                if now >= expire {
                    let since_expiry = now.saturating_sub(expire);
                    if entry
                        .renew_within_ms
                        .map(|window| since_expiry <= window)
                        .unwrap_or(false)
                    {
                        should_apply = true;
                    } else {
                        expired_now = true;
                    }
                } else if let Some(window) = entry.renew_within_ms {
                    let until_expiry = expire.saturating_sub(now);
                    if until_expiry <= window {
                        should_apply = true;
                    }
                }
            }

            match entry.remaining_hops {
                Some(0) => {}
                Some(ref mut hops) => {
                    should_apply = true;
                    *hops = hops.saturating_sub(1);
                    entry.snapshot.remaining_hops = Some(*hops);
                    changed = true;
                }
                None => {}
            }

            if expired_now {
                to_remove.push(id.clone());
                expired.push(entry.snapshot.clone());
                changed = true;
                continue;
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
            self.last_refresh_ms.store(now, Ordering::Relaxed);
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
        drop(guard);
        self.last_refresh_ms.store(now, Ordering::Relaxed);
        ReplayOutcome {
            expired,
            changed,
            reapplied,
        }
    }

    pub async fn next_refresh_delay_ms(&self, now_ms: u64, max_wait_ms: u64) -> u64 {
        let max_wait_ms = max_wait_ms.max(MIN_REFRESH_MS);
        let guard = self.inner.lock().await;
        if guard.is_empty() {
            return max_wait_ms;
        }
        let mut soonest = max_wait_ms;
        for entry in guard.values() {
            if let Some(lease_until) = entry.lease_until_ms {
                if now_ms >= lease_until {
                    return 0;
                }
                if let Some(window) = entry.renew_within_ms {
                    let renew_start = lease_until.saturating_sub(window);
                    if now_ms >= renew_start {
                        return 0;
                    }
                    let until_renew = renew_start.saturating_sub(now_ms);
                    soonest = soonest.min(until_renew);
                } else {
                    let until_expire = lease_until.saturating_sub(now_ms);
                    soonest = soonest.min(until_expire);
                }
            }
            if let Some(hops) = entry.remaining_hops {
                if hops > 0 {
                    let hop_wait = max_wait_ms.min(HOP_TICK_MS);
                    soonest = soonest.min(hop_wait);
                }
            }
        }
        soonest
    }

    pub fn last_refresh_ms(&self) -> u64 {
        self.last_refresh_ms.load(Ordering::Relaxed)
    }

    pub fn is_fresh(&self, now_ms: u64, max_stale_ms: u64) -> bool {
        let last = self.last_refresh_ms();
        if last == 0 {
            return false;
        }
        now_ms.saturating_sub(last) <= max_stale_ms
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
    if replay.changed {
        let snapshot = state.capsules().snapshot().await;
        read_models::publish_read_model_patch(&state.bus(), "policy_capsules", &snapshot);
    }
    replay
}

pub async fn refresh_capsules_if_needed(state: &AppState) -> Option<ReplayOutcome> {
    let max_stale = request_refresh_stale_ms();
    let now = now_ms();
    if state.capsules().is_fresh(now, max_stale) {
        return None;
    }
    Some(refresh_capsules(state).await)
}

pub async fn emergency_teardown(
    state: &AppState,
    spec: &CapsuleTeardownSpec<'_>,
) -> CapsuleTeardownOutcome {
    let store = state.capsules();
    let CapsuleStoreTeardown {
        removed: removed_snapshots,
        not_found,
        remaining,
    } = store.teardown(&spec.selection, spec.dry_run).await;

    let now = now_ms();
    let reason = spec
        .reason
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let removed_values: Vec<Value> = removed_snapshots
        .iter()
        .map(|snapshot| snapshot.to_json(now))
        .collect();

    if !spec.dry_run && !removed_values.is_empty() {
        for value in removed_values.iter() {
            let mut event = value.clone();
            if let Some(map) = event.as_object_mut() {
                map.insert("removed_ms".into(), Value::Number(now.into()));
                if let Some(reason_text) = reason.as_ref() {
                    map.insert("removed_reason".into(), Value::String(reason_text.clone()));
                }
            }
            state.bus().publish(TOPIC_POLICY_CAPSULE_TEARDOWN, &event);
        }
        let snapshot = store.snapshot().await;
        read_models::publish_read_model_patch(&state.bus(), "policy_capsules", &snapshot);
    }

    CapsuleTeardownOutcome {
        removed: removed_values,
        not_found,
        remaining,
        dry_run: spec.dry_run,
        reason,
    }
}

fn refresh_max_wait_ms() -> u64 {
    if let Ok(raw_ms) = std::env::var("ARW_CAPSULE_REFRESH_MS") {
        return raw_ms
            .trim()
            .parse::<u64>()
            .map(|v| v.max(MIN_REFRESH_MS))
            .unwrap_or(DEFAULT_REFRESH_SECS * 1_000);
    }
    std::env::var("ARW_CAPSULE_REFRESH_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(|secs| secs.max(1) * 1_000)
        .unwrap_or(DEFAULT_REFRESH_SECS * 1_000)
        .max(MIN_REFRESH_MS)
}

fn request_refresh_stale_ms() -> u64 {
    std::env::var("ARW_CAPSULE_REQUEST_REFRESH_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| refresh_max_wait_ms().max(2 * MIN_REFRESH_MS))
}

pub fn start_refresh_task(state: AppState) -> TaskHandle {
    let bus = state.bus();
    crate::tasks::spawn_supervised_with(
        "capsules.refresh",
        move || {
            let state = state.clone();
            async move {
                let max_wait_ms = refresh_max_wait_ms();
                if max_wait_ms < 1_000 {
                    tracing::debug!(
                        target: "arw::policy",
                        max_sleep_ms = max_wait_ms,
                        "capsule refresh cadence tightened",
                    );
                }
                let initial = refresh_capsules(&state).await;
                if initial.changed || !initial.expired.is_empty() {
                    tracing::debug!(
                        target: "arw::policy",
                        expired = initial.expired.len(),
                        changed = initial.changed,
                        max_sleep_ms = max_wait_ms,
                        "capsule refresh sweep applied",
                    );
                }

                loop {
                    let delay_ms = state
                        .capsules()
                        .next_refresh_delay_ms(now_ms(), max_wait_ms)
                        .await;
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    } else {
                        tokio::task::yield_now().await;
                    }
                    let outcome = refresh_capsules(&state).await;
                    if outcome.changed || !outcome.expired.is_empty() {
                        tracing::debug!(
                            target: "arw::policy",
                            expired = outcome.expired.len(),
                            changed = outcome.changed,
                            max_sleep_ms = max_wait_ms,
                            "capsule refresh sweep applied",
                        );
                    }
                }
            }
        },
        Some(move |restarts| {
            if restarts >= 5 {
                let payload = serde_json::json!({
                    "status": "degraded",
                    "component": "capsules.refresh",
                    "reason": "task_thrashing",
                    "restarts_window": restarts,
                    "window_secs": 30,
                });
                bus.publish(arw_topics::TOPIC_SERVICE_HEALTH, &payload);
            }
        }),
    )
}

pub async fn capsule_mw(state: AppState, req: Request<Body>, next: Next) -> Response {
    let corr = request_ctx::context(&req);
    match apply_capsule(&state, req.headers(), corr.as_ref()).await {
        Ok(_) => next.run(req).await,
        Err(resp) => resp,
    }
}

async fn apply_capsule(
    state: &AppState,
    headers: &HeaderMap,
    corr: Option<&RequestCorrelation>,
) -> Result<(), Response> {
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
            publish_failure(state, capsule_id.as_deref(), LEGACY_HEADER_DETAIL, corr).await;
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
            publish_failure(state, None, kind.as_message(), corr).await;
            return Err(resp);
        }
    };
    if !arw_core::rpu::verify_capsule(&capsule) {
        publish_failure(state, Some(&capsule.id), "verification failed", corr).await;
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "capsule_verification_failed",
            "Capsule verification failed",
        ));
    }
    let now = now_ms();
    let outcome = state.capsules().adopt(&capsule, now).await;
    if outcome.notify {
        let mut event = Map::with_capacity(9);
        event.insert("id".into(), Value::String(outcome.snapshot.id.clone()));
        event.insert(
            "version".into(),
            Value::String(outcome.snapshot.version.clone()),
        );
        event.insert(
            "issuer".into(),
            outcome
                .snapshot
                .issuer
                .as_ref()
                .map(|issuer| Value::String(issuer.clone()))
                .unwrap_or(Value::Null),
        );
        event.insert(
            "applied_ms".into(),
            Value::Number(outcome.snapshot.applied_ms.into()),
        );
        event.insert(
            "hop_ttl".into(),
            outcome
                .snapshot
                .hop_ttl
                .map(|ttl| Value::Number((ttl as u64).into()))
                .unwrap_or(Value::Null),
        );
        event.insert(
            "denies".into(),
            Value::Number((outcome.snapshot.denies as u64).into()),
        );
        event.insert(
            "contracts".into(),
            Value::Number((outcome.snapshot.contracts as u64).into()),
        );
        event.insert(
            "lease_until_ms".into(),
            outcome
                .snapshot
                .lease_until_ms
                .map(|v| Value::Number(v.into()))
                .unwrap_or(Value::Null),
        );
        event.insert(
            "renew_within_ms".into(),
            outcome
                .snapshot
                .renew_within_ms
                .map(|v| Value::Number(v.into()))
                .unwrap_or(Value::Null),
        );
        add_corr_fields(&mut event, corr);
        state
            .bus()
            .publish(TOPIC_POLICY_CAPSULE_APPLIED, &Value::Object(event));
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

fn add_corr_fields(target: &mut Map<String, Value>, corr: Option<&RequestCorrelation>) {
    if let Some(meta) = corr {
        if !target.contains_key("corr_id") {
            target.insert("corr_id".into(), Value::String(meta.corr_id().to_string()));
        }
        if !target.contains_key("request_id") {
            target.insert(
                "request_id".into(),
                Value::String(meta.request_id().to_string()),
            );
        }
    }
}

async fn publish_failure(
    state: &AppState,
    capsule_id: Option<&str>,
    detail: &str,
    corr: Option<&RequestCorrelation>,
) {
    let mut failure = Map::with_capacity(2);
    failure.insert(
        "id".into(),
        capsule_id
            .map(|id| Value::String(id.to_string()))
            .unwrap_or(Value::Null),
    );
    failure.insert("detail".into(), Value::String(detail.to_string()));
    add_corr_fields(&mut failure, corr);
    state
        .bus()
        .publish(TOPIC_POLICY_CAPSULE_FAILED, &Value::Object(failure));

    let mut explain = Map::new();
    explain.insert("detail".into(), Value::String(detail.to_string()));
    if let Some(id) = capsule_id {
        explain.insert("capsule_id".into(), Value::String(id.to_string()));
    }

    let mut decision = Map::with_capacity(3);
    decision.insert("action".into(), Value::String("policy.capsule".into()));
    decision.insert("allow".into(), Value::Bool(false));
    decision.insert("explain".into(), Value::Object(explain));
    add_corr_fields(&mut decision, corr);
    state
        .bus()
        .publish(TOPIC_POLICY_DECISION, &Value::Object(decision));
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

fn classify_capsule(snapshot: &CapsuleSnapshot, now_ms: u64) -> CapsuleStatusInfo {
    let lease_until = snapshot.lease_until_ms;
    let renew_window = snapshot.renew_within_ms;
    let renew_window_start_ms =
        lease_until.and_then(|lease| renew_window.map(|window| lease.saturating_sub(window)));
    let renew_window_started = renew_window_start_ms
        .map(|start| now_ms >= start)
        .unwrap_or(false);

    let mut expires_in_ms = lease_until.map(|lease| lease.saturating_sub(now_ms));
    let mut renew_in_ms = renew_window_start_ms.map(|start| start.saturating_sub(now_ms));
    let mut expired_since_ms = None;

    #[allow(unused_mut)]
    let mut status: &'static str;
    #[allow(unused_mut)]
    let mut status_label: String;
    let mut aria_hint = format!("Capsule {}.", snapshot.id);

    if let Some(lease) = lease_until {
        let expires_in = lease.saturating_sub(now_ms);
        expires_in_ms = Some(expires_in);

        if now_ms >= lease {
            status = "expired";
            status_label = "Expired – renew required".to_string();
            let since = now_ms.saturating_sub(lease);
            expired_since_ms = Some(since);
            aria_hint.push(' ');
            aria_hint.push_str(&format!(
                "Expired {}. Apply a new capsule to restore enforcement.",
                format_relative_past(since)
            ));
            renew_in_ms = None;
        } else if renew_window_started {
            status = "renew_due";
            status_label = if expires_in == 0 {
                "Renew now – expires immediately".to_string()
            } else {
                format!(
                    "Renew now – expires in {}",
                    format_duration_units(expires_in)
                )
            };
            aria_hint.push(' ');
            aria_hint.push_str(&format!(
                "Renewal window active. Capsule expires {}.",
                format_relative_future(expires_in)
            ));
        } else if expires_in <= CAPSULE_EXPIRING_SOON_WINDOW_MS {
            status = "expiring";
            status_label = if expires_in == 0 {
                "Expiring now".to_string()
            } else {
                format!("Expiring soon – {} left", format_duration_units(expires_in))
            };
            aria_hint.push(' ');
            aria_hint.push_str(&format!(
                "Capsule expires {}.",
                format_relative_future(expires_in)
            ));
        } else {
            status = "active";
            if let Some(renew_in) = renew_in_ms {
                if renew_in == 0 {
                    status_label = "Active – renewal window opening".to_string();
                    aria_hint.push(' ');
                    aria_hint.push_str(&format!(
                        "Healthy. Renewal window begins soon and expires {}.",
                        format_relative_future(expires_in)
                    ));
                } else {
                    status_label = format!("Active – renew in {}", format_duration_units(renew_in));
                    aria_hint.push(' ');
                    aria_hint.push_str(&format!(
                        "Healthy. Renewal window opens {} and expiry follows {}.",
                        format_relative_future(renew_in),
                        format_relative_future(expires_in)
                    ));
                }
            } else {
                status_label = format!("Active – expires in {}", format_duration_units(expires_in));
                aria_hint.push(' ');
                aria_hint.push_str(&format!(
                    "Healthy. Capsule expires {}.",
                    format_relative_future(expires_in)
                ));
            }
        }
    } else {
        status = "unbounded";
        status_label = "Active – lease not set".to_string();
        aria_hint.push(' ');
        aria_hint.push_str(
            "Healthy. Capsule does not define a lease duration; renew manually when required.",
        );
    }

    if let Some(hops) = snapshot.remaining_hops {
        aria_hint.push(' ');
        if hops > 0 {
            aria_hint.push_str(&format!(
                "{} hop{} remaining before forced refresh.",
                hops,
                if hops == 1 { "" } else { "s" }
            ));
        } else {
            aria_hint.push_str("Hop limit reached; awaiting refresh.");
        }
    }

    CapsuleStatusInfo {
        status,
        status_label,
        aria_hint,
        expires_in_ms,
        renew_in_ms,
        renew_window_start_ms,
        renew_window_started,
        expired_since_ms,
    }
}

fn ms_to_rfc3339(ms: u64) -> Option<String> {
    let millis = i64::try_from(ms).ok()?;
    Utc.timestamp_millis_opt(millis)
        .single()
        .map(|dt| dt.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn format_duration_units(ms: u64) -> String {
    const SECOND: u64 = 1_000;
    let total_seconds = ms / SECOND;
    if total_seconds == 0 {
        return "under 1 second".to_string();
    }

    let units: &[(u64, &str, &str)] = &[
        (86_400, "day", "days"),
        (3_600, "hour", "hours"),
        (60, "minute", "minutes"),
        (1, "second", "seconds"),
    ];

    let mut remaining = total_seconds;
    let mut parts: Vec<String> = Vec::new();
    for (unit_secs, singular, plural) in units.iter().copied() {
        if remaining >= unit_secs {
            let value = remaining / unit_secs;
            remaining %= unit_secs;
            let label = if value == 1 { singular } else { plural };
            parts.push(format!("{} {}", value, label));
            if parts.len() == 2 {
                break;
            }
        }
    }

    if parts.is_empty() {
        "under 1 second".to_string()
    } else {
        parts.join(" ")
    }
}

fn format_relative_future(ms: u64) -> String {
    if ms == 0 {
        "now".to_string()
    } else {
        format!("in {}", format_duration_units(ms))
    }
}

fn format_relative_past(ms: u64) -> String {
    if ms == 0 {
        "just now".to_string()
    } else {
        format!("{} ago", format_duration_units(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_ctx::{CorrelationSource, RequestCorrelation};
    use crate::test_support::env as test_env;
    use arw_policy::PolicyEngine;
    use arw_topics::{TOPIC_POLICY_CAPSULE_FAILED, TOPIC_POLICY_DECISION, TOPIC_READMODEL_PATCH};
    use axum::http::{HeaderMap, HeaderValue};
    use std::{path::Path, sync::Arc};
    use tempfile::tempdir;
    use tokio::time::{sleep, timeout, Duration};

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

    async fn build_state(dir: &Path, env_guard: &mut test_env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(arw_wasi::NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
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

    #[test]
    fn classify_capsule_reports_status_transitions() {
        let base_now = 1_000_000u64;
        let snapshot = CapsuleSnapshot {
            id: "status".into(),
            version: "1".into(),
            issuer: Some("issuer".into()),
            applied_ms: base_now,
            hop_ttl: None,
            denies: 2,
            contracts: 1,
            remaining_hops: Some(2),
            lease_until_ms: Some(base_now + 120_000),
            renew_within_ms: Some(60_000),
        };

        let active = classify_capsule(&snapshot, base_now);
        assert_eq!(active.status, "active");
        assert_eq!(active.expires_in_ms, Some(120_000));
        assert_eq!(active.renew_in_ms, Some(60_000));
        assert!(active.aria_hint.contains("Healthy"));

        let renew_due = classify_capsule(&snapshot, base_now + 70_000);
        assert_eq!(renew_due.status, "renew_due");
        assert!(renew_due.aria_hint.contains("Renewal window active"));
        assert!(renew_due.renew_window_started);

        let expired = classify_capsule(&snapshot, base_now + 150_000);
        assert_eq!(expired.status, "expired");
        assert!(expired.aria_hint.contains("Expired"));
        assert!(expired.expired_since_ms.is_some());
    }

    #[test]
    fn snapshot_to_json_enriches_metadata() {
        let base_now = 1_000_000u64;
        let snapshot = CapsuleSnapshot {
            id: "json".into(),
            version: "1".into(),
            issuer: None,
            applied_ms: base_now,
            hop_ttl: Some(3),
            denies: 0,
            contracts: 0,
            remaining_hops: Some(1),
            lease_until_ms: Some(base_now + 10_000),
            renew_within_ms: Some(5_000),
        };

        let value = snapshot.to_json(base_now + 6_000);
        let obj = value.as_object().expect("snapshot json object");
        assert_eq!(obj.get("status").and_then(Value::as_str), Some("renew_due"));
        assert!(obj
            .get("status_label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("Renew"));
        assert!(obj
            .get("aria_hint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("Renewal window active"));
        assert!(obj.get("renew_in_ms").and_then(Value::as_u64).is_some());
        assert!(obj
            .get("renew_window_start_ms")
            .and_then(Value::as_u64)
            .is_some());
        assert_eq!(
            obj.get("renew_window_started").and_then(Value::as_bool),
            Some(true)
        );
        assert!(obj.get("expires_in_ms").and_then(Value::as_u64).is_some());
    }

    #[tokio::test]
    async fn replay_all_renews_with_short_window_before_purging() {
        let store = CapsuleStore::new();
        let mut capsule = sample_capsule("renewal-test");
        capsule.hop_ttl = None;
        capsule.signature = None;
        capsule.lease_duration_ms = Some(300);
        capsule.renew_within_ms = Some(1_500);

        store.adopt(&capsule, now_ms()).await;

        sleep(Duration::from_millis(350)).await;

        let replay = store.replay_all().await;
        assert!(replay.expired.is_empty());
        assert_eq!(replay.reapplied.len(), 1);

        let snapshot = store.snapshot().await;
        assert_eq!(snapshot["count"].as_u64(), Some(1));
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
    async fn emergency_teardown_removes_capsules_and_publishes_events() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                TOPIC_POLICY_CAPSULE_TEARDOWN.to_string(),
                TOPIC_READMODEL_PATCH.to_string(),
            ],
            Some(8),
        );

        let capsule = sample_capsule("teardown-test");
        state.capsules().adopt(&capsule, now_ms()).await;

        let ids = vec![String::from("teardown-test")];
        let spec = CapsuleTeardownSpec {
            selection: CapsuleTeardownSelection::Ids(&ids),
            reason: Some(" manual cleanup "),
            dry_run: false,
        };
        let outcome = emergency_teardown(&state, &spec).await;
        assert_eq!(outcome.removed.len(), 1);
        assert_eq!(outcome.not_found.len(), 0);
        assert_eq!(outcome.remaining, 0);
        assert_eq!(outcome.reason.as_deref(), Some("manual cleanup"));

        let teardown_evt = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("teardown event available")
            .expect("bus open");
        assert_eq!(teardown_evt.kind, TOPIC_POLICY_CAPSULE_TEARDOWN);
        assert_eq!(teardown_evt.payload["id"].as_str(), Some("teardown-test"));
        assert_eq!(
            teardown_evt.payload["removed_reason"].as_str(),
            Some("manual cleanup")
        );

        let patch_evt = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("patch event available")
            .expect("bus open");
        assert_eq!(patch_evt.kind, TOPIC_READMODEL_PATCH);
        assert_eq!(patch_evt.payload["id"].as_str(), Some("policy_capsules"));

        let snapshot = state.capsules().snapshot().await;
        assert_eq!(snapshot["count"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn emergency_teardown_dry_run_has_no_side_effects() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let bus = state.bus();
        let mut rx =
            bus.subscribe_filtered(vec![TOPIC_POLICY_CAPSULE_TEARDOWN.to_string()], Some(4));

        let capsule = sample_capsule("dry-run-test");
        state.capsules().adopt(&capsule, now_ms()).await;

        let spec = CapsuleTeardownSpec {
            selection: CapsuleTeardownSelection::All,
            reason: Some("preview"),
            dry_run: true,
        };
        let outcome = emergency_teardown(&state, &spec).await;
        assert!(outcome.dry_run);
        assert_eq!(outcome.removed.len(), 1);
        assert_eq!(outcome.remaining, 1);

        let snapshot = state.capsules().snapshot().await;
        assert_eq!(snapshot["count"].as_u64(), Some(1));

        // Expect no teardown events during dry-run
        assert!(timeout(Duration::from_millis(150), rx.recv())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn refresh_capsules_publishes_patch_on_state_change() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

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
        let mut env_guard = test_env::guard();

        env_guard.remove("ARW_CAPSULE_REFRESH_SECS");
        env_guard.remove("ARW_CAPSULE_REFRESH_MS");
        assert_eq!(refresh_max_wait_ms(), DEFAULT_REFRESH_SECS * 1_000);

        env_guard.set("ARW_CAPSULE_REFRESH_SECS", "0");
        assert_eq!(refresh_max_wait_ms(), 1_000);

        env_guard.set("ARW_CAPSULE_REFRESH_SECS", "7");
        assert_eq!(refresh_max_wait_ms(), 7_000);

        env_guard.set("ARW_CAPSULE_REFRESH_SECS", "not-a-number");
        assert_eq!(refresh_max_wait_ms(), DEFAULT_REFRESH_SECS * 1_000);

        env_guard.set("ARW_CAPSULE_REFRESH_MS", "120");
        assert_eq!(refresh_max_wait_ms(), 120);

        env_guard.set("ARW_CAPSULE_REFRESH_MS", "5");
        assert_eq!(refresh_max_wait_ms(), MIN_REFRESH_MS);
    }

    #[tokio::test]
    async fn publish_failure_attaches_corr_metadata() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
        let bus = state.bus();
        let mut rx = bus.subscribe_filtered(
            vec![
                TOPIC_POLICY_CAPSULE_FAILED.to_string(),
                TOPIC_POLICY_DECISION.to_string(),
            ],
            Some(8),
        );

        let corr = RequestCorrelation::new("req-123", "corr-456", CorrelationSource::Provided);
        publish_failure(&state, Some("caps-demo"), "invalid", Some(&corr)).await;
        assert_eq!(corr.source(), CorrelationSource::Provided);

        let mut events = std::collections::HashMap::new();
        for _ in 0..2 {
            let env = timeout(Duration::from_secs(1), rx.recv())
                .await
                .expect("bus event")
                .expect("bus open");
            events.insert(env.kind.clone(), env.payload);
        }

        let failure = events
            .remove(TOPIC_POLICY_CAPSULE_FAILED)
            .expect("failure event");
        assert_eq!(failure["id"].as_str(), Some("caps-demo"));
        assert_eq!(failure["detail"].as_str(), Some("invalid"));
        assert_eq!(failure["corr_id"].as_str(), Some("corr-456"));
        assert_eq!(failure["request_id"].as_str(), Some("req-123"));

        let decision = events
            .remove(TOPIC_POLICY_DECISION)
            .expect("policy decision event");
        assert_eq!(decision["action"].as_str(), Some("policy.capsule"));
        assert_eq!(decision["allow"].as_bool(), Some(false));
        assert_eq!(decision["corr_id"].as_str(), Some("corr-456"));
        assert_eq!(decision["request_id"].as_str(), Some("req-123"));
        assert_eq!(
            decision["explain"]["capsule_id"].as_str(),
            Some("caps-demo"),
        );
    }

    #[tokio::test]
    async fn next_refresh_delay_honours_renew_window() {
        let store = CapsuleStore::new();
        let now = now_ms();
        let mut capsule = sample_capsule("renew-window");
        capsule.lease_duration_ms = Some(250);
        capsule.renew_within_ms = Some(200);
        capsule.signature = None;
        store.adopt(&capsule, now).await;

        let wait = store.next_refresh_delay_ms(now, 5_000).await;
        assert!(wait <= 200);
        assert!(wait > 0);
    }

    #[tokio::test]
    async fn next_refresh_delay_immediate_on_expiry() {
        let store = CapsuleStore::new();
        let now = now_ms();
        let mut capsule = sample_capsule("expired");
        capsule.lease_duration_ms = Some(10);
        capsule.renew_within_ms = Some(10);
        capsule.signature = None;
        store.adopt(&capsule, now.saturating_sub(20)).await;

        let wait = store.next_refresh_delay_ms(now, 5_000).await;
        assert_eq!(wait, 0);
    }

    #[tokio::test]
    async fn legacy_header_returns_gone_and_emits_failure_events() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = crate::test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;
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

        let result = apply_capsule(&state, &headers, None).await;
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
