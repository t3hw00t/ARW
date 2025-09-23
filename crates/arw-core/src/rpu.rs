//! Regulatory Provenance Unit (RPU) — skeleton.
//! Verifies and adopts policy capsules; currently passthrough with stubs.

use crate::gating;
use base64::{engine::general_purpose::STANDARD as b64, Engine};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

#[derive(Debug, Deserialize, Clone)]
struct TrustEntry {
    id: String,
    alg: String, // "ed25519" | "secp256k1"
    key_b64: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct TrustConfig {
    #[serde(default)]
    issuers: Vec<TrustEntry>,
}

static TRUST: OnceCell<RwLock<TrustConfig>> = OnceCell::new();
static TRUST_LAST_MS: OnceCell<AtomicU64> = OnceCell::new();

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn load_trust() -> TrustConfig {
    if let Some(cell) = TRUST.get() {
        return cell.read().unwrap().clone();
    }
    let path = std::env::var("ARW_TRUST_CAPSULES")
        .ok()
        .unwrap_or_else(|| "configs/trust_capsules.json".to_string());
    let cfg = if Path::new(&path).exists() {
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<TrustConfig>(&s).unwrap_or_default(),
            Err(_) => TrustConfig::default(),
        }
    } else {
        TrustConfig::default()
    };
    let _ = TRUST.set(RwLock::new(cfg.clone()));
    TRUST_LAST_MS
        .get_or_init(|| AtomicU64::new(now_ms()))
        .store(now_ms(), Ordering::Relaxed);
    cfg
}

/// Force reload trust store from disk (best-effort)
pub fn reload_trust() {
    let path = std::env::var("ARW_TRUST_CAPSULES")
        .ok()
        .unwrap_or_else(|| "configs/trust_capsules.json".to_string());
    let cfg = if Path::new(&path).exists() {
        match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<TrustConfig>(&s).unwrap_or_default(),
            Err(_) => TrustConfig::default(),
        }
    } else {
        TrustConfig::default()
    };
    if let Some(cell) = TRUST.get() {
        *cell.write().unwrap() = cfg;
    } else {
        let _ = TRUST.set(RwLock::new(cfg));
    }
    TRUST_LAST_MS
        .get_or_init(|| AtomicU64::new(now_ms()))
        .store(now_ms(), Ordering::Relaxed);
}

/// Public snapshot of the trust store without exposing keys.
#[derive(Debug, Serialize, Clone)]
pub struct TrustIssuer {
    pub id: String,
    pub alg: String,
}

/// Return a redacted view of the current trust issuers (id, alg only).
pub fn trust_snapshot() -> Vec<TrustIssuer> {
    if let Some(cell) = TRUST.get() {
        return cell
            .read()
            .map(|cfg| {
                (cfg.issuers.iter())
                    .map(|e| TrustIssuer {
                        id: e.id.clone(),
                        alg: e.alg.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let cfg = load_trust();
    cfg.issuers
        .into_iter()
        .map(|e| TrustIssuer {
            id: e.id,
            alg: e.alg,
        })
        .collect()
}

/// Milliseconds since epoch of the last successful trust load/reload.
pub fn trust_last_reload_ms() -> u64 {
    TRUST_LAST_MS
        .get_or_init(|| AtomicU64::new(now_ms()))
        .load(Ordering::Relaxed)
}

fn signing_bytes(cap: &arw_protocol::GatingCapsule) -> Vec<u8> {
    // Build a copy with signature cleared to ensure deterministic bytes
    let mut c = cap.clone();
    c.signature = None;
    serde_json::to_vec(&c).unwrap_or_default()
}

/// Attempt to verify a capsule's provenance. Stubbed to `true` pending trust store integration.
pub fn verify_capsule(cap: &arw_protocol::GatingCapsule) -> bool {
    let trust = load_trust();
    let issuer = match cap.issuer.as_deref() {
        Some(s) => s,
        None => return false,
    };
    let entry = match trust.issuers.iter().find(|e| e.id == issuer) {
        Some(e) => e,
        None => return false,
    };
    let sig_b64 = match &cap.signature {
        Some(s) => s,
        None => return false,
    };
    let sig = match b64.decode(sig_b64.as_bytes()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let msg = signing_bytes(cap);
    match entry.alg.as_str() {
        "ed25519" => {
            if sig.len() != 64 {
                return false;
            }
            let key = match b64.decode(entry.key_b64.as_bytes()) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if key.len() != 32 {
                return false;
            }
            let key_arr: [u8; 32] = match key.as_slice().try_into() {
                Ok(a) => a,
                Err(_) => return false,
            };
            let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&key_arr) else {
                return false;
            };
            let sig_arr: [u8; 64] = match sig.as_slice().try_into() {
                Ok(a) => a,
                Err(_) => return false,
            };
            let sigd = ed25519_dalek::Signature::from_bytes(&sig_arr);
            vk.verify_strict(&msg, &sigd).is_ok() && abac_allows(cap)
        }
        "secp256k1" => {
            // Try DER then 64-byte fixed
            let key = match b64.decode(entry.key_b64.as_bytes()) {
                Ok(v) => v,
                Err(_) => return false,
            };
            let Ok(vk) = k256::ecdsa::VerifyingKey::from_sec1_bytes(&key) else {
                return false;
            };
            // Hash the message with SHA-256 for ECDSA
            use k256::ecdsa::signature::hazmat::PrehashVerifier;
            use k256::sha2::{Digest, Sha256};
            let digest = Sha256::digest(&msg);
            let dbytes = digest.as_slice();
            let ok = if let Ok(sig_der) = k256::ecdsa::Signature::from_der(&sig) {
                vk.verify_prehash(dbytes, &sig_der).is_ok()
            } else if sig.len() == 64 {
                if let Ok(sig_fix) = k256::ecdsa::Signature::from_slice(&sig) {
                    vk.verify_prehash(dbytes, &sig_fix).is_ok()
                } else {
                    false
                }
            } else {
                false
            };
            ok && abac_allows(cap)
        }
        _ => false,
    }
}

/// Helper: parse header JSON and adopt if verified.
pub fn adopt_from_header_json(s: &str) -> bool {
    match serde_json::from_str::<arw_protocol::GatingCapsule>(s) {
        Ok(cap) => {
            if !verify_capsule(&cap) {
                return false;
            }
            gating::adopt_capsule(&cap);
            true
        }
        Err(_) => false,
    }
}

fn abac_allows(cap: &arw_protocol::GatingCapsule) -> bool {
    // Minimal checks: TTL, issued_at bounds, propagate sanity
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if cap.issued_at_ms > now.saturating_add(5 * 60 * 1000) {
        return false;
    } // future-dated too far
    if let Some(ttl) = cap.hop_ttl {
        if ttl == 0 {
            return false;
        }
    }
    if let Some(p) = &cap.propagate {
        if !matches!(p.as_str(), "none" | "children" | "peers" | "all") {
            return false;
        }
    }
    true
}
