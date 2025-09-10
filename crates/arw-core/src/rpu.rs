//! Regulatory Provenance Unit (RPU) — skeleton.
//! Verifies and adopts policy capsules; currently passthrough with stubs.

use crate::gating;

/// Attempt to verify a capsule's provenance. Stubbed to `true` pending trust store integration.
pub fn verify_capsule(_cap: &arw_protocol::GatingCapsule) -> bool {
    // TODO: signature verification, trust roots, ABAC
    true
}

/// Verify and adopt a capsule. Returns true if adopted.
pub fn verify_and_adopt(cap: &arw_protocol::GatingCapsule) -> bool {
    if !verify_capsule(cap) {
        return false;
    }
    gating::adopt_capsule(cap);
    true
}

/// Helper: parse header JSON and adopt if verified.
pub fn adopt_from_header_json(s: &str) -> bool {
    match serde_json::from_str::<arw_protocol::GatingCapsule>(s) {
        Ok(cap) => verify_and_adopt(&cap),
        Err(_) => false,
    }
}

