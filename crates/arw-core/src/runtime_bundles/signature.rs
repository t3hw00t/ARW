use anyhow::{Context, Result};
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt::Write;

use super::signers::RuntimeBundleSignerRegistry;

/// Report describing validation for a single manifest signature entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestSignatureReport {
    #[serde(default)]
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key_b64: Option<String>,
    #[serde(default)]
    pub hash_matches: bool,
    #[serde(default)]
    pub signature_valid: bool,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default)]
    pub rejected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Aggregate verification result for a bundle manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestVerification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_sha256: Option<String>,
    #[serde(default)]
    pub signatures: Vec<ManifestSignatureReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub trust_enforced: bool,
    #[serde(default)]
    pub trusted_signatures: usize,
    #[serde(default)]
    pub rejected_signatures: usize,
    #[serde(default)]
    pub ok: bool,
}

/// Compute the canonical payload and SHA-256 hex digest for a manifest.
pub fn canonical_payload_bytes(manifest: &Value) -> Result<(Vec<u8>, String)> {
    if !manifest.is_object() {
        anyhow::bail!("bundle manifest root must be a JSON object");
    }
    let mut sanitized = manifest.clone();
    if let Some(obj) = sanitized.as_object_mut() {
        obj.remove("signatures");
    }
    let canonical = canonicalize_manifest_value(&sanitized);
    let bytes =
        serde_json::to_vec(&canonical).context("serializing canonical bundle manifest JSON")?;
    let digest = compute_sha256_hex(&bytes);
    Ok((bytes, digest))
}

/// Verify all signature entries present in the manifest.
///
/// Any malformed entry is reported in-place and results in `ok = false`, but the
/// function never returns an error—callers receive structured warnings instead.
pub fn verify_manifest_signatures(manifest: &Value) -> ManifestVerification {
    verify_manifest_signatures_with_registry(manifest, None, None)
}

/// Verify signature entries with an optional trusted signer registry.
pub fn verify_manifest_signatures_with_registry(
    manifest: &Value,
    registry: Option<&RuntimeBundleSignerRegistry>,
    channel_hint: Option<&str>,
) -> ManifestVerification {
    let channel_normalized = channel_hint.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let mut result = ManifestVerification {
        canonical_sha256: None,
        signatures: Vec::new(),
        warnings: Vec::new(),
        trust_enforced: registry.is_some(),
        trusted_signatures: 0,
        rejected_signatures: 0,
        ok: true,
    };

    let (payload_bytes, payload_sha_hex) = match canonical_payload_bytes(manifest) {
        Ok(tuple) => {
            result.canonical_sha256 = Some(format!("sha256:{}", tuple.1));
            tuple
        }
        Err(err) => {
            result.ok = false;
            result.warnings.push(err.to_string());
            return result;
        }
    };

    let Some(signatures_value) = manifest.as_object().and_then(|obj| obj.get("signatures")) else {
        result.ok = false;
        result
            .warnings
            .push("manifest has no signatures array".to_string());
        return result;
    };

    let signatures = match signatures_value.as_array() {
        Some(array) => array,
        None => {
            result.ok = false;
            result
                .warnings
                .push("manifest signatures entry is not an array".to_string());
            return result;
        }
    };

    if signatures.is_empty() {
        result.ok = false;
        result
            .warnings
            .push("manifest signatures array is empty".to_string());
        return result;
    }

    let mut trusted_count = 0usize;
    let mut rejected_count = 0usize;

    for (index, entry) in signatures.iter().enumerate() {
        let mut report = ManifestSignatureReport {
            index,
            key_id: entry
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            issuer: entry
                .get("issuer")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            issued_at: entry
                .get("issued_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            manifest_sha256: entry
                .get("manifest_sha256")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            public_key_b64: entry
                .get("public_key_b64")
                .or_else(|| entry.get("public_key"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            hash_matches: false,
            signature_valid: false,
            trusted: false,
            rejected: false,
            error: None,
        };

        let mut errors: Vec<String> = Vec::new();

        if let Some(ref recorded) = report.manifest_sha256 {
            report.hash_matches = manifest_hash_matches(recorded, &payload_sha_hex);
            if !report.hash_matches {
                errors.push(format!(
                    "manifest_sha256 {} does not match canonical sha256:{}",
                    recorded, payload_sha_hex
                ));
            }
        } else {
            errors.push("manifest_sha256 missing".to_string());
        }

        let Some(pk_b64) = report.public_key_b64.as_deref() else {
            errors.push("public_key_b64 missing".to_string());
            report.hash_matches &= false;
            result.ok = false;
            if !errors.is_empty() {
                report.error = Some(errors.join("; "));
            }
            result.signatures.push(report);
            continue;
        };

        let pk_bytes = match base64::engine::general_purpose::STANDARD.decode(pk_b64) {
            Ok(bytes) => bytes,
            Err(err) => {
                errors.push(format!("invalid public_key_b64: {}", err));
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };

        let pk_array: [u8; 32] = match pk_bytes.as_slice().try_into() {
            Ok(array) => array,
            Err(_) => {
                errors.push("public key must be 32 bytes".to_string());
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };

        let verifying_key = match VerifyingKey::from_bytes(&pk_array) {
            Ok(key) => key,
            Err(err) => {
                errors.push(format!("invalid ed25519 public key: {}", err));
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };

        let sig_b64 = match entry.get("signature").and_then(|v| v.as_str()) {
            Some(value) => value,
            None => {
                errors.push("signature missing".to_string());
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };

        let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(sig_b64) {
            Ok(bytes) => bytes,
            Err(err) => {
                errors.push(format!("invalid signature encoding: {}", err));
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };

        let sig_array: [u8; 64] = match sig_bytes.as_slice().try_into() {
            Ok(array) => array,
            Err(_) => {
                errors.push("signature must be 64 bytes".to_string());
                result.ok = false;
                report.error = Some(errors.join("; "));
                result.signatures.push(report);
                continue;
            }
        };
        let signature = Signature::from_bytes(&sig_array);

        match verifying_key.verify(&payload_bytes, &signature) {
            Ok(()) => report.signature_valid = true,
            Err(err) => {
                errors.push(format!("signature verification failed: {}", err));
                result.ok = false;
            }
        }

        if report.signature_valid {
            if let Some(registry) = registry {
                if registry.is_trusted(
                    report.key_id.as_deref(),
                    report.public_key_b64.as_deref(),
                    channel_normalized,
                ) {
                    report.trusted = true;
                    trusted_count += 1;
                } else {
                    report.rejected = true;
                    rejected_count += 1;
                    let message = if let Some(key_id) = report.key_id.as_deref() {
                        if let Some(channel) = channel_normalized {
                            format!(
                                "signature with key_id {} is not trusted for channel {}",
                                key_id, channel
                            )
                        } else {
                            format!(
                                "signature with key_id {} is not present in signer registry",
                                key_id
                            )
                        }
                    } else if let Some(channel) = channel_normalized {
                        format!(
                            "signature public key is not trusted for channel {}",
                            channel
                        )
                    } else if let Some(pk) = report.public_key_b64.as_deref() {
                        format!("signature public key {} is not trusted", pk)
                    } else {
                        "signature is not trusted by signer registry".to_string()
                    };
                    errors.push(message);
                }
            }
        }

        if !report.hash_matches {
            result.ok = false;
        }

        if !errors.is_empty() {
            report.error = Some(errors.join("; "));
        }

        result.signatures.push(report);
    }

    result.trusted_signatures = trusted_count;
    result.rejected_signatures = rejected_count;

    if result.trust_enforced && trusted_count == 0 {
        result.ok = false;
        let warning = if let Some(channel) = channel_normalized {
            format!(
                "no trusted signatures matched signer registry for channel {}",
                channel
            )
        } else {
            "no trusted signatures matched signer registry".to_string()
        };
        result.warnings.push(warning);
    }

    result
}

/// Derive a default key identifier (ed25519-sha256:<prefix>) from the public key bytes.
pub fn default_manifest_key_id(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    let mut hex = String::new();
    for byte in digest.iter().take(10) {
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    format!("ed25519-sha256:{}", hex)
}

/// Compare a recorded manifest hash against the expected canonical digest.
pub fn manifest_hash_matches(recorded: &str, expected_hex: &str) -> bool {
    let trimmed = recorded.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = if let Some(rest) = trimmed.strip_prefix("sha256:") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix("SHA256:") {
        rest
    } else {
        trimmed
    };
    normalized.eq_ignore_ascii_case(expected_hex)
}

fn canonicalize_manifest_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut new_map = Map::new();
            for (key, val) in entries {
                new_map.insert(key.clone(), canonicalize_manifest_value(val));
            }
            Value::Object(new_map)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(canonicalize_manifest_value).collect())
        }
        other => other.clone(),
    }
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_bundles::signers::{RuntimeBundleSignerEntry, RuntimeBundleSignerRegistry};
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    #[test]
    fn canonical_payload_strips_signatures_block() -> Result<()> {
        let manifest = json!({
            "bundle": { "id": "example", "name": "Example", "adapter": "process" },
            "signatures": [{ "key_id": "old", "signature": "stub" }]
        });
        let (payload, sha) = canonical_payload_bytes(&manifest)?;
        let roundtrip: Value = serde_json::from_slice(&payload)?;
        assert!(roundtrip.get("signatures").is_none(), "signatures removed");
        assert_eq!(sha.len(), 64, "sha256 hex length");
        Ok(())
    }

    #[test]
    fn verify_manifest_detects_missing_signatures() {
        let manifest = json!({
            "bundle": { "id": "example", "name": "Example", "adapter": "process" }
        });
        let verification = verify_manifest_signatures(&manifest);
        assert!(!verification.ok);
        assert_eq!(
            verification.warnings,
            vec!["manifest has no signatures array".to_string()]
        );
    }

    #[test]
    fn verify_manifest_accepts_valid_signature() -> Result<()> {
        let manifest = json!({
            "bundle": { "id": "example", "name": "Example", "adapter": "process" }
        });
        let (payload_bytes, payload_sha) = canonical_payload_bytes(&manifest)?;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
        let verifying_key = signing_key.verifying_key();
        let signature = signing_key.sign(&payload_bytes);
        let signed_manifest = json!({
            "bundle": manifest.get("bundle").cloned().unwrap(),
            "signatures": [{
                "alg": "ed25519",
                "key_id": default_manifest_key_id(&verifying_key.to_bytes()),
                "public_key_b64": base64::engine::general_purpose::STANDARD.encode(verifying_key.to_bytes()),
                "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
                "manifest_sha256": format!("sha256:{}", payload_sha),
                "issued_at": "2025-10-12T00:00:00Z"
            }]
        });

        let verification = verify_manifest_signatures(&signed_manifest);
        assert!(verification.ok, "signature should verify");
        assert_eq!(verification.signatures.len(), 1);
        let report = &verification.signatures[0];
        assert!(report.signature_valid);
        assert!(report.hash_matches);
        assert!(
            !report.trusted,
            "no registry provided so signature remains untrusted"
        );
        assert!(!report.rejected);
        assert!(!verification.trust_enforced);
        assert_eq!(verification.trusted_signatures, 0);
        assert_eq!(verification.rejected_signatures, 0);
        assert!(report.error.is_none());
        Ok(())
    }

    #[test]
    fn verify_manifest_flags_untrusted_signer() -> Result<()> {
        let manifest = json!({
            "bundle": { "id": "example", "name": "Example", "adapter": "process" }
        });
        let (payload_bytes, payload_sha) = canonical_payload_bytes(&manifest)?;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
        let verifying_key = signing_key.verifying_key();
        let signature = signing_key.sign(&payload_bytes);
        let key_id = default_manifest_key_id(&verifying_key.to_bytes());
        let public_key_b64 =
            base64::engine::general_purpose::STANDARD.encode(verifying_key.to_bytes());

        let signed_manifest = json!({
            "bundle": manifest.get("bundle").cloned().unwrap(),
            "signatures": [{
                "alg": "ed25519",
                "key_id": key_id,
                "public_key_b64": public_key_b64,
                "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
                "manifest_sha256": format!("sha256:{}", payload_sha),
                "issued_at": "2025-10-12T00:00:00Z"
            }]
        });

        let registry = RuntimeBundleSignerRegistry::from_entries(
            1,
            vec![RuntimeBundleSignerEntry {
                key_id: "preview-signer".to_string(),
                public_key_b64: base64::engine::general_purpose::STANDARD
                    .encode(rand::random::<[u8; 32]>()),
                issuer: Some("ci@example.com".to_string()),
                channels: vec!["preview".to_string()],
                notes: None,
                expires_at: None,
            }],
        )?;

        let verification = verify_manifest_signatures_with_registry(
            &signed_manifest,
            Some(&registry),
            Some("preview"),
        );
        assert!(!verification.ok, "no trusted signatures should fail");
        assert_eq!(verification.trusted_signatures, 0);
        assert_eq!(verification.rejected_signatures, 1);
        assert_eq!(verification.signatures.len(), 1);
        let report = &verification.signatures[0];
        assert!(report.signature_valid);
        assert!(report.rejected);
        assert!(!report.trusted);
        let msg = report.error.as_deref().unwrap_or("");
        assert!(
            msg.contains("not trusted for channel") || msg.contains("not present"),
            "unexpected verification error: {msg}"
        );
        Ok(())
    }

    #[test]
    fn verify_manifest_accepts_trusted_signer() -> Result<()> {
        let manifest = json!({
            "bundle": { "id": "example", "name": "Example", "adapter": "process" }
        });
        let (payload_bytes, payload_sha) = canonical_payload_bytes(&manifest)?;
        let signing_key = SigningKey::from_bytes(&rand::random::<[u8; 32]>());
        let verifying_key = signing_key.verifying_key();
        let signature = signing_key.sign(&payload_bytes);
        let public_key = verifying_key.to_bytes();
        let public_key_b64 = base64::engine::general_purpose::STANDARD.encode(public_key);
        let key_id = default_manifest_key_id(&public_key);

        let signed_manifest = json!({
            "bundle": manifest.get("bundle").cloned().unwrap(),
            "signatures": [{
                "alg": "ed25519",
                "key_id": key_id.clone(),
                "public_key_b64": public_key_b64.clone(),
                "signature": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
                "manifest_sha256": format!("sha256:{}", payload_sha),
                "issued_at": "2025-10-12T00:00:00Z"
            }]
        });

        let registry = RuntimeBundleSignerRegistry::from_entries(
            1,
            vec![RuntimeBundleSignerEntry {
                key_id: key_id.clone(),
                public_key_b64: public_key_b64.clone(),
                issuer: Some("ci@example.com".to_string()),
                channels: vec!["preview".to_string()],
                notes: Some("trusted signer".to_string()),
                expires_at: None,
            }],
        )?;

        let verification = verify_manifest_signatures_with_registry(
            &signed_manifest,
            Some(&registry),
            Some("preview"),
        );
        assert!(verification.ok, "trusted signature should pass");
        assert_eq!(verification.trusted_signatures, 1);
        assert_eq!(verification.rejected_signatures, 0);
        assert!(verification.trust_enforced);
        assert_eq!(verification.signatures.len(), 1);
        let report = &verification.signatures[0];
        assert!(report.signature_valid);
        assert!(report.hash_matches);
        assert!(report.trusted);
        assert!(!report.rejected);
        Ok(())
    }
}
