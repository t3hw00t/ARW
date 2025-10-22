//! ARW Runtime Adapter SDK helpers.
//!
//! This crate complements `arw-runtime` by providing manifest utilities and
//! validation helpers for third-party adapters. The goal is to give adapter
//! authors a stable, well-documented surface for plugging new runtimes into the
//! supervisor without depending on internal server crates.

use std::path::Path;

pub mod manifest;

pub use arw_runtime::{
    AdapterError, PrepareContext, PreparedRuntime, RuntimeAdapter, RuntimeAdapterMetadata,
    RuntimeDescriptor, RuntimeHandle, RuntimeHealthReport, RuntimeModality, RuntimeRestartBudget,
    RuntimeSeverity, RuntimeState, RuntimeStatus,
};

pub use manifest::{
    AdapterConsent, AdapterHealthSpec, AdapterMetric, AdapterResources, ManifestFormat,
    ManifestLoadError, RuntimeAdapterManifest, ValidationIssue, ValidationReport,
};

/// Load and validate an adapter manifest from disk in a single step.
///
/// This is a small convenience wrapper that combines [`RuntimeAdapterManifest::from_path`]
/// with [`RuntimeAdapterManifest::validate`]. It returns the manifest together with a
/// structured validation report so callers can surface warnings without re-reading the file.
pub fn load_manifest_with_report<P: AsRef<Path>>(
    path: P,
) -> Result<(RuntimeAdapterManifest, ValidationReport), ManifestLoadError> {
    let manifest = RuntimeAdapterManifest::from_path(path)?;
    let report = manifest.validate();
    Ok((manifest, report))
}
