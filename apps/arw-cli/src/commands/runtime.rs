use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::create_dir_all;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use arw_core::runtime_bundles::{
    signature::{
        canonical_payload_bytes, default_manifest_key_id, verify_manifest_signatures_with_registry,
        ManifestVerification,
    },
    signers::RuntimeBundleSignerRegistry,
};
use arw_core::{load_effective_paths, resolve_config_path, runtime_bundles};
use arw_runtime::{RuntimeAccelerator, RuntimeModality, RuntimeSeverity, RuntimeState};
use base64::Engine;
use chrono::{DateTime, Local, SecondsFormat, Utc};
use clap::{Args, Subcommand};
use reqwest::{blocking::Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::util::{
    append_json_output, append_text_output, format_bytes, parse_byte_limit_arg,
    resolve_admin_token, with_admin_headers,
};

#[cfg(test)]
mod runtime_bundle_manifest_tests {
    use super::*;
    use serde_json::{json, Value as JsonValue};
    use tempfile::tempdir;

    #[test]
    fn sign_and_verify_manifest_roundtrip() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let manifest_path = tmp.path().join("bundle.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&json!({
                "bundle": {
                    "id": "llama.cpp-preview/linux-x86_64-cpu",
                    "name": "Test LLaMA",
                    "adapter": "process",
                    "modalities": ["text"],
                    "profiles": ["balanced"],
                    "accelerator": "cpu"
                },
                "catalog": { "channel": "preview" },
                "installed_at": "2025-10-12T00:00:00Z"
            }))?,
        )?;

        let (_pk_b64, sk_b64) = crate::commands::capsule::generate_ed25519_pair_b64()?;
        let sign_args = RuntimeBundlesManifestSignArgs {
            manifest: manifest_path.clone(),
            key_b64: Some(sk_b64),
            key_file: None,
            issuer: Some("ci".to_string()),
            key_id: None,
            output: None,
            compact: false,
        };
        cmd_runtime_bundles_manifest_sign(&sign_args)?;

        let manifest_bytes = std::fs::read(&sign_args.manifest)
            .context("reading signed manifest for verification test")?;
        let manifest_value: JsonValue =
            serde_json::from_slice(&manifest_bytes).context("parsing signed manifest JSON")?;
        let signatures = manifest_value
            .get("signatures")
            .and_then(|v| v.as_array())
            .expect("signatures array present");
        assert_eq!(signatures.len(), 1);

        let mut verify_args = RuntimeBundlesManifestVerifyArgs {
            manifest: sign_args.manifest.clone(),
            json: false,
            pretty: false,
            require_trusted: false,
        };
        cmd_runtime_bundles_manifest_verify(&verify_args)?;
        verify_args.require_trusted = true;
        let err = cmd_runtime_bundles_manifest_verify(&verify_args)
            .expect_err("require_trusted should fail for unsigned test key");
        assert!(
            err.to_string()
                .contains("no trusted signatures matched the signer registry"),
            "expected failure due to untrusted signatures, got {err:?}"
        );
        Ok(())
    }
}

#[cfg(test)]
mod runtime_bundle_audit_tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn audit_detects_missing_signatures() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let bundle_dir = root.join("unsigned");
        std::fs::create_dir_all(bundle_dir.join("artifacts"))?;
        let manifest = json!({
            "bundle": {
                "id": "llama.cpp-preview/linux-x86_64-cpu",
                "name": "Unsigned LLaMA",
                "adapter": "process",
                "modalities": ["text"],
                "accelerator": "cpu",
                "profiles": ["balanced"]
            },
            "catalog": { "channel": "preview" },
            "installed_at": "2025-10-12T00:00:00Z"
        });
        std::fs::write(
            bundle_dir.join("bundle.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;

        let args_ok = RuntimeBundlesAuditArgs {
            dest: Some(root.to_path_buf()),
            json: false,
            pretty: false,
            require_signed: false,
            remote: false,
            base: RuntimeBaseArgs {
                base: "http://127.0.0.1:8091".to_string(),
                admin_token: None,
                timeout: 5,
            },
        };
        cmd_runtime_bundles_audit(&args_ok)?;

        let args_fail = RuntimeBundlesAuditArgs {
            dest: Some(root.to_path_buf()),
            json: false,
            pretty: false,
            require_signed: true,
            remote: false,
            base: RuntimeBaseArgs {
                base: "http://127.0.0.1:8091".to_string(),
                admin_token: None,
                timeout: 5,
            },
        };
        assert!(cmd_runtime_bundles_audit(&args_fail).is_err());
        Ok(())
    }

    #[test]
    fn audit_accepts_signed_manifest() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let bundle_dir = root.join("signed");
        std::fs::create_dir_all(bundle_dir.join("artifacts"))?;
        let manifest = json!({
            "bundle": {
                "id": "llama.cpp-preview/linux-x86_64-cpu",
                "name": "Signed LLaMA",
                "adapter": "process",
                "modalities": ["text"],
                "accelerator": "cpu",
                "profiles": ["balanced"]
            },
            "catalog": { "channel": "preview" },
            "installed_at": "2025-10-12T00:00:00Z"
        });
        let manifest_path = bundle_dir.join("bundle.json");
        std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)?;

        let (pk, sk) = crate::commands::capsule::generate_ed25519_pair_b64()?;
        let sign_args = RuntimeBundlesManifestSignArgs {
            manifest: manifest_path.clone(),
            key_b64: Some(sk),
            key_file: None,
            issuer: Some("test".to_string()),
            key_id: None,
            output: None,
            compact: false,
        };
        cmd_runtime_bundles_manifest_sign(&sign_args)?;

        let registry_path = root.join("bundle_signers.json");
        let registry = json!({
            "version": 1,
            "signers": [{
                "key_id": "test-signer",
                "public_key_b64": pk,
                "issuer": "test",
                "channels": ["preview"]
            }]
        });
        std::fs::write(&registry_path, serde_json::to_vec_pretty(&registry)?)?;
        let previous_signers = std::env::var("ARW_RUNTIME_BUNDLE_SIGNERS").ok();
        std::env::set_var(
            "ARW_RUNTIME_BUNDLE_SIGNERS",
            registry_path.to_string_lossy().as_ref(),
        );
        let args = RuntimeBundlesAuditArgs {
            dest: Some(root.to_path_buf()),
            json: false,
            pretty: false,
            require_signed: true,
            remote: false,
            base: RuntimeBaseArgs {
                base: "http://127.0.0.1:8091".to_string(),
                admin_token: None,
                timeout: 5,
            },
        };
        let result = cmd_runtime_bundles_audit(&args);
        if let Some(value) = previous_signers {
            std::env::set_var("ARW_RUNTIME_BUNDLE_SIGNERS", value);
        } else {
            std::env::remove_var("ARW_RUNTIME_BUNDLE_SIGNERS");
        }
        result?;
        Ok(())
    }
}

#[derive(Subcommand)]
pub enum RuntimeCmd {
    /// Show runtime supervisor snapshot with restart budgets
    Status(RuntimeStatusArgs),
    /// Request a managed runtime restore
    Restore(RuntimeRestoreArgs),
    /// Request a managed runtime shutdown
    Shutdown(RuntimeShutdownArgs),
    /// Inspect managed runtime bundle catalogs
    Bundles {
        #[command(subcommand)]
        cmd: RuntimeBundlesCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum RuntimeBundlesCmd {
    /// List bundle catalogs discovered under configs/runtime
    List(RuntimeBundlesListArgs),
    /// Request the server to rescan bundle catalogs
    Reload(RuntimeBundlesReloadArgs),
    /// Download bundle artifacts into the managed runtime directory
    Install(RuntimeBundlesInstallArgs),
    /// Import local artifacts into the managed runtime directory
    Import(RuntimeBundlesImportArgs),
    /// Roll back a managed runtime bundle to a previous revision
    Rollback(RuntimeBundlesRollbackArgs),
    /// Bundle manifest helpers (signing, verification)
    Manifest {
        #[command(subcommand)]
        cmd: RuntimeBundlesManifestCmd,
    },
    /// Audit installed bundles for signature coverage and health
    Audit(RuntimeBundlesAuditArgs),
    /// List bundles whose signatures verified but are missing trusted signer entries
    TrustShortfall(RuntimeBundlesTrustShortfallArgs),
}

fn runtime_bundles_list_remote(args: &RuntimeBundlesListArgs) -> Result<()> {
    if args.dir.is_some() {
        eprintln!("note: --dir is ignored when --remote is set");
    }
    let snapshot = fetch_runtime_bundle_snapshot_remote(&args.base)?;

    if args.json {
        let payload = serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({}));
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!("{}", payload);
        }
        return Ok(());
    }

    println!(
        "Remote runtime bundle inventory (base: {})",
        args.base.base_url()
    );
    print_bundle_summary(
        &snapshot.catalogs,
        &snapshot.roots,
        snapshot.generated.as_deref(),
        &snapshot.installations,
        snapshot.signature_summary.as_ref(),
    );
    Ok(())
}

fn catalog_view_from_source(
    src: runtime_bundles::RuntimeBundleCatalogSource,
) -> CliRuntimeBundleCatalog {
    let runtime_bundles::RuntimeBundleCatalogSource { path, catalog } = src;
    CliRuntimeBundleCatalog {
        path: path.to_string_lossy().into_owned(),
        version: catalog.version,
        channel: catalog.channel,
        notes: catalog.notes,
        bundles: catalog.bundles,
    }
}

fn print_bundle_summary(
    catalogs: &[CliRuntimeBundleCatalog],
    roots: &[String],
    generated: Option<&str>,
    installations: &[CliRuntimeBundleInstallation],
    signature_summary: Option<&CliSignatureSummary>,
) {
    if !roots.is_empty() {
        println!("Roots: {}", roots.join(", "));
    }
    if let Some(ts) = generated {
        println!("Generated: {}", ts);
    }
    if let Some(summary) = signature_summary {
        println!(
            "Signatures: total {} | with manifest {} | verified {} | trusted {} | trust shortfall {} | rejected {} | failed {} | warnings {} | missing {} | enforced {}",
            summary.total,
            summary.with_manifest,
            summary.verified,
            summary.trusted,
            summary.trust_shortfall,
            summary.rejected,
            summary.failed,
            summary.warnings,
            summary.missing_signatures,
            if summary.enforced { "yes" } else { "no" }
        );
        if summary.trust_shortfall > 0 {
            println!(
                "  note: {} manifest{} verified but lack a trusted signer registry entry for their channel; install matching entries in configs/runtime/bundle_signers.json to promote them",
                summary.trust_shortfall,
                if summary.trust_shortfall == 1 { "" } else { "s" }
            );
        }
    }
    println!(
        "Found {} bundle catalog{}.",
        catalogs.len(),
        if catalogs.len() == 1 { "" } else { "s" }
    );
    if catalogs.is_empty() {
        println!("(no bundle catalogs declared)");
    }

    for catalog in catalogs {
        println!(
            "\n{} — version {}{}",
            catalog.path,
            catalog.version,
            catalog
                .channel
                .as_deref()
                .map(|c| format!(" (channel: {})", c))
                .unwrap_or_default()
        );
        if let Some(notes) = catalog.notes.as_deref() {
            println!("  {}", notes);
        }
        if catalog.bundles.is_empty() {
            println!("  (no bundles declared)");
            continue;
        }
        for bundle in &catalog.bundles {
            let modalities = if bundle.modalities.is_empty() {
                "—".to_string()
            } else {
                bundle
                    .modalities
                    .iter()
                    .map(modality_slug)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let accelerator = bundle
                .accelerator
                .as_ref()
                .map(accelerator_slug)
                .unwrap_or("—");
            let platforms = if bundle.platforms.is_empty() {
                "—".to_string()
            } else {
                bundle
                    .platforms
                    .iter()
                    .map(|p| match p.min_version.as_deref() {
                        Some(min) if !min.is_empty() => {
                            format!("{}-{} (>= {})", p.os, p.arch, min)
                        }
                        _ => format!("{}-{}", p.os, p.arch),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            println!("  - {} [{}]", bundle.name, bundle.id);
            println!(
                "    adapter: {} | modalities: {} | accelerator: {}",
                bundle.adapter, modalities, accelerator
            );
            println!("    platforms: {}", platforms);
            if !bundle.profiles.is_empty() {
                println!("    profiles: {}", bundle.profiles.join(", "));
            }
            if !bundle.artifacts.is_empty() {
                let mut artifacts = Vec::new();
                for artifact in &bundle.artifacts {
                    let mut label = artifact.kind.clone();
                    if let Some(fmt) = artifact.format.as_deref() {
                        label.push_str(&format!(" ({})", fmt));
                    }
                    if let Some(url) = artifact.url.as_deref() {
                        label.push_str(&format!(" -> {}", url));
                    } else if let Some(notes) = artifact.notes.as_deref() {
                        label.push_str(&format!(" — {}", notes));
                    }
                    artifacts.push(label);
                }
                println!("    artifacts: {}", artifacts.join("; "));
            }
            if let Some(license) = bundle.license.as_deref() {
                println!("    license: {}", license);
            }
            if let Some(support) = bundle.support.as_ref() {
                let mut support_notes = Vec::new();
                if let Some(glibc) = support.min_glibc.as_deref() {
                    support_notes.push(format!("glibc >= {}", glibc));
                }
                if let Some(macos) = support.min_macos.as_deref() {
                    support_notes.push(format!("macOS >= {}", macos));
                }
                if let Some(windows) = support.min_windows.as_deref() {
                    support_notes.push(format!("Windows >= {}", windows));
                }
                if let Some(driver) = support.driver_notes.as_deref() {
                    support_notes.push(driver.to_string());
                }
                if !support_notes.is_empty() {
                    println!("    support: {}", support_notes.join("; "));
                }
            }
            if let Some(consent) = bundle_consent_summary(bundle) {
                println!("    {}", consent);
            }
            if !bundle.notes.is_empty() {
                println!("    notes: {}", bundle.notes[0]);
                if bundle.notes.len() > 1 {
                    println!(
                        "    (+{} additional note{})",
                        bundle.notes.len() - 1,
                        if bundle.notes.len() > 2 { "s" } else { "" }
                    );
                }
            }
        }
    }

    if !installations.is_empty() {
        println!(
            "\nDiscovered {} installed bundle{}:",
            installations.len(),
            if installations.len() == 1 { "" } else { "s" }
        );
        for inst in installations {
            let label = inst.name.as_deref().unwrap_or(inst.id.as_str());
            println!("  - {} [{}]", label, inst.id);
            let mut detail_parts: Vec<String> = Vec::new();
            if let Some(adapter) = inst.adapter.as_deref() {
                detail_parts.push(format!("adapter: {}", adapter));
            }
            if let Some(accel) = inst.accelerator.as_deref() {
                detail_parts.push(format!("accelerator: {}", accel));
            }
            if !inst.modalities.is_empty() {
                detail_parts.push(format!("modalities: {}", inst.modalities.join("/")));
            }
            if !inst.profiles.is_empty() {
                detail_parts.push(format!("profiles: {}", inst.profiles.join("/")));
            }
            if let Some(channel) = inst.channel.as_deref() {
                detail_parts.push(format!("channel: {}", channel));
            }
            if let Some(ts) = inst.installed_at.as_deref() {
                detail_parts.push(format!("installed_at: {}", ts));
            }
            if inst.installed_at.is_none() {
                if let Some(ts) = inst.imported_at.as_deref() {
                    detail_parts.push(format!("imported_at: {}", ts));
                }
            }
            if let Some(root) = inst.root.as_deref() {
                detail_parts.push(format!("root: {}", root));
            }
            if let Some(meta_path) = inst.metadata_path.as_deref() {
                detail_parts.push(format!("metadata: {}", meta_path));
            }
            if !detail_parts.is_empty() {
                println!("    {}", detail_parts.join(" | "));
            }
            if !inst.artifacts.is_empty() {
                let total_bytes: u64 = inst.artifacts.iter().filter_map(|a| a.bytes).sum();
                let names = inst
                    .artifacts
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if total_bytes > 0 {
                    println!(
                        "    artifacts: {} (total {})",
                        names,
                        format_bytes(total_bytes)
                    );
                } else {
                    println!("    artifacts: {}", names);
                }
            }
            if let Some(sig) = inst.signature.as_ref() {
                let status_label = if sig.ok {
                    "verified"
                } else {
                    "needs attention"
                };
                let mut signature_parts = vec![format!("signature: {}", status_label)];
                if let Some(sha) = sig.canonical_sha256.as_deref() {
                    signature_parts.push(format!("canonical {}", sha));
                }
                println!("    {}", signature_parts.join(" | "));
                for entry in &sig.signatures {
                    let key = entry.key_id.as_deref().unwrap_or("<unknown>");
                    let state = if entry.signature_valid && entry.hash_matches {
                        "valid"
                    } else if entry.signature_valid {
                        "hash mismatch"
                    } else {
                        "invalid"
                    };
                    let mut entry_line = format!("      key {} -> {}", key, state);
                    if let Some(issuer) = entry.issuer.as_deref() {
                        entry_line.push_str(&format!(" (issuer: {})", issuer));
                    }
                    println!("{}", entry_line);
                    if let Some(err) = entry.error.as_deref() {
                        println!("        error: {}", err);
                    }
                }
                if !sig.warnings.is_empty() {
                    println!("      warnings: {}", sig.warnings.join(" | "));
                }
            }
            if let Some(bundle) = inst.bundle.as_ref() {
                if let Some(consent) = bundle_consent_summary(bundle) {
                    println!("    {}", consent);
                }
            }
        }
    } else {
        println!("\nDiscovered 0 installed bundles.");
    }
}

#[derive(Args)]
pub(crate) struct RuntimeBaseArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 10)]
    timeout: u64,
}

impl RuntimeBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args)]
pub(crate) struct RuntimeStatusArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Emit JSON instead of human summary
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Poll continuously and print summaries on interval
    #[arg(long, conflicts_with = "json")]
    watch: bool,
    /// Seconds between polls when --watch is enabled
    #[arg(long, default_value_t = 15, requires = "watch")]
    interval: u64,
    /// Append output to this file (creates directories as needed)
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
    /// Rotate output file when it reaches this many bytes (requires --output)
    #[arg(
        long,
        value_name = "BYTES",
        requires = "output",
        value_parser = parse_byte_limit_arg,
        help = "Rotate after BYTES (supports K/M/G/T suffixes; min 64KB unless 0)"
    )]
    output_rotate: Option<u64>,
}

#[derive(Args)]
pub(crate) struct RuntimeRestoreArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Runtime identifier
    id: String,
    /// Disable automatic restart flag in the request payload
    #[arg(long)]
    no_restart: bool,
    /// Optional preset name to pass through to the supervisor
    #[arg(long)]
    preset: Option<String>,
}

#[derive(Args)]
pub(crate) struct RuntimeShutdownArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Runtime identifier
    id: String,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesListArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Directory containing bundle catalogs (defaults to configs/runtime/)
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
    /// Directory containing installed bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long = "install-dir", value_name = "DIR")]
    install_dir: Option<PathBuf>,
    /// Fetch bundle catalogs from a running server instead of local files
    #[arg(long)]
    remote: bool,
    /// Emit JSON instead of human-readable output
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesTrustShortfallArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Directory containing bundle catalogs (defaults to configs/runtime/)
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
    /// Directory containing installed bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long = "install-dir", value_name = "DIR")]
    install_dir: Option<PathBuf>,
    /// Fetch bundle catalogs from a running server instead of local files
    #[arg(long)]
    remote: bool,
    /// Emit JSON instead of human-readable output
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesReloadArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Emit JSON response
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesInstallArgs {
    /// Directory containing bundle catalogs (defaults to configs/runtime/)
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
    /// Fetch bundle catalogs from a running server instead of local files
    #[arg(long)]
    remote: bool,
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Destination root for installed bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long, value_name = "DIR")]
    dest: Option<PathBuf>,
    /// Preview actions without downloading artifacts
    #[arg(long)]
    dry_run: bool,
    /// Overwrite existing files and metadata
    #[arg(long)]
    force: bool,
    /// Only download artifacts whose kind matches one of the provided values
    #[arg(long = "artifact-kind", value_name = "KIND")]
    artifact_kinds: Vec<String>,
    /// Only download artifacts whose format matches one of the provided values
    #[arg(long = "artifact-format", value_name = "FORMAT")]
    artifact_formats: Vec<String>,
    /// Bundle identifiers to install
    #[arg(value_name = "BUNDLE_ID", required = true)]
    bundles: Vec<String>,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesImportArgs {
    /// Bundle identifier that the imported payload should live under
    #[arg(long = "bundle", value_name = "BUNDLE_ID")]
    bundle: String,
    /// Destination root for imported bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long, value_name = "DIR")]
    dest: Option<PathBuf>,
    /// Preview actions without touching the filesystem
    #[arg(long)]
    dry_run: bool,
    /// Overwrite existing files and directories
    #[arg(long)]
    force: bool,
    /// Optional metadata JSON to copy into bundle.json
    #[arg(long, value_name = "FILE")]
    metadata: Option<PathBuf>,
    /// Files or directories to import
    #[arg(value_name = "PATH", required = true)]
    paths: Vec<PathBuf>,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesRollbackArgs {
    /// Destination root containing managed runtime bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long, value_name = "DIR")]
    dest: Option<PathBuf>,
    /// Bundle identifier to roll back
    #[arg(long = "bundle", value_name = "BUNDLE_ID")]
    bundle: String,
    /// Revision to restore (defaults to the most recent revision)
    #[arg(long = "revision", value_name = "REVISION")]
    revision: Option<String>,
    /// List available revisions instead of applying a rollback
    #[arg(long)]
    list: bool,
    /// Preview actions without modifying anything
    #[arg(long)]
    dry_run: bool,
    /// Emit JSON instead of human-readable output
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Subcommand)]
pub(crate) enum RuntimeBundlesManifestCmd {
    /// Sign a bundle manifest with an ed25519 key
    Sign(RuntimeBundlesManifestSignArgs),
    /// Verify bundle manifest signatures
    Verify(RuntimeBundlesManifestVerifyArgs),
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesManifestSignArgs {
    /// Bundle manifest JSON file (bundle.json)
    #[arg(value_name = "MANIFEST")]
    manifest: PathBuf,
    /// Secret key in base64 (ed25519 32-byte)
    #[arg(long, value_name = "B64", conflicts_with = "key_file")]
    key_b64: Option<String>,
    /// File containing base64-encoded secret key (ed25519)
    #[arg(long, value_name = "FILE", conflicts_with = "key_b64")]
    key_file: Option<PathBuf>,
    /// Optional issuer label to embed in the signature entry
    #[arg(long, value_name = "ISSUER")]
    issuer: Option<String>,
    /// Optional key identifier (defaults to ed25519-sha256:<fingerprint>)
    #[arg(long, value_name = "KEY_ID")]
    key_id: Option<String>,
    /// Write output to a different file instead of modifying MANIFEST in place
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Write compact JSON instead of pretty formatting
    #[arg(long)]
    compact: bool,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesManifestVerifyArgs {
    /// Bundle manifest JSON file (bundle.json)
    #[arg(value_name = "MANIFEST")]
    manifest: PathBuf,
    /// Emit JSON instead of text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Require at least one trusted signature (fails when registry has no match)
    #[arg(long)]
    require_trusted: bool,
}

#[derive(Args)]
pub(crate) struct RuntimeBundlesAuditArgs {
    /// Destination root containing managed runtime bundles (defaults to <state_dir>/runtime/bundles)
    #[arg(long, value_name = "DIR", conflicts_with = "remote")]
    dest: Option<PathBuf>,
    /// Emit JSON instead of text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Fail when any bundle is missing a valid signature
    #[arg(long)]
    require_signed: bool,
    /// Inspect bundles on a running server instead of local filesystem
    #[arg(long, conflicts_with = "dest")]
    remote: bool,
    #[command(flatten)]
    base: RuntimeBaseArgs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliRuntimeBundleSnapshot {
    #[serde(default)]
    generated: Option<String>,
    #[serde(default)]
    generated_ms: Option<u64>,
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    catalogs: Vec<CliRuntimeBundleCatalog>,
    #[serde(default)]
    installations: Vec<CliRuntimeBundleInstallation>,
    #[serde(default)]
    signature_summary: Option<CliSignatureSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliRuntimeBundleCatalog {
    path: String,
    version: u32,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    bundles: Vec<runtime_bundles::RuntimeBundle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliRuntimeBundleInstallation {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    adapter: Option<String>,
    #[serde(default)]
    profiles: Vec<String>,
    #[serde(default)]
    modalities: Vec<String>,
    #[serde(default)]
    accelerator: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    installed_at: Option<String>,
    #[serde(default)]
    imported_at: Option<String>,
    #[serde(default)]
    source: Option<JsonValue>,
    #[serde(default)]
    metadata_path: Option<String>,
    #[serde(default)]
    artifacts: Vec<CliRuntimeBundleArtifact>,
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    bundle: Option<runtime_bundles::RuntimeBundle>,
    #[serde(default)]
    signature: Option<ManifestVerification>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CliSignatureSummary {
    #[serde(default)]
    total: usize,
    #[serde(default)]
    with_manifest: usize,
    #[serde(default)]
    verified: usize,
    #[serde(default)]
    failed: usize,
    #[serde(default)]
    warnings: usize,
    #[serde(default)]
    missing_signatures: usize,
    #[serde(default)]
    trusted: usize,
    #[serde(default)]
    rejected: usize,
    #[serde(default)]
    trust_shortfall: usize,
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    enforced: bool,
}

#[derive(Debug, Clone, Serialize)]
struct BundleTrustShortfallEntry {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    installed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    imported_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

impl From<SignatureSummary> for CliSignatureSummary {
    fn from(src: SignatureSummary) -> Self {
        Self {
            total: src.total,
            with_manifest: src.with_manifest,
            verified: src.verified,
            failed: src.failed,
            warnings: src.warnings,
            missing_signatures: src.missing_signatures,
            trusted: src.trusted,
            rejected: src.rejected,
            trust_shortfall: src.trust_shortfall,
            ok: src.ok,
            enforced: src.enforced,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliRuntimeBundleArtifact {
    name: String,
    #[serde(default)]
    bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliRuntimeBundlesReloadResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

fn load_runtime_bundle_snapshot_local(dir: Option<PathBuf>) -> Result<CliRuntimeBundleSnapshot> {
    let base_dir = if let Some(dir) = dir {
        dir
    } else {
        resolve_config_path("configs/runtime").ok_or_else(|| {
            anyhow!("unable to locate configs/runtime/; pass --dir to point at bundle catalogs")
        })?
    };

    if !base_dir.exists() {
        bail!("bundle directory {} does not exist", base_dir.display());
    }

    let sources = runtime_bundles::load_catalogs_from_dir(&base_dir)?;
    let catalogs: Vec<CliRuntimeBundleCatalog> =
        sources.into_iter().map(catalog_view_from_source).collect();
    let roots = vec![base_dir.display().to_string()];
    let now = Utc::now();
    Ok(CliRuntimeBundleSnapshot {
        generated: Some(now.to_rfc3339_opts(SecondsFormat::Millis, true)),
        generated_ms: Some(now.timestamp_millis().max(0) as u64),
        roots,
        catalogs,
        installations: Vec::new(),
        signature_summary: None,
    })
}

fn load_local_runtime_bundle_installations(
    root: &Path,
) -> Result<Vec<CliRuntimeBundleInstallation>> {
    let mut installs = Vec::new();
    let signers = match RuntimeBundleSignerRegistry::load_default() {
        Ok(Some(registry)) => Some(Arc::new(registry)),
        Ok(None) => None,
        Err(err) => {
            eprintln!("warning: failed to load runtime bundle signer registry: {err}");
            None
        }
    };
    let read_dir = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(installs),
        Err(err) => {
            return Err(anyhow!(
                "reading runtime bundle install root {}: {err}",
                root.display()
            ))
        }
    };
    for entry in read_dir {
        let entry = entry.with_context(|| {
            format!(
                "reading entry within runtime bundle install root {}",
                root.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().with_context(|| {
            format!(
                "reading file type for runtime bundle entry {}",
                path.display()
            )
        })?;
        if !file_type.is_dir() {
            continue;
        }
        if let Some(install) = load_local_runtime_bundle_installation(root, &path, signers.clone())?
        {
            installs.push(install);
        }
    }
    installs.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.root.cmp(&b.root)));
    Ok(installs)
}

fn load_local_runtime_bundle_installation(
    root: &Path,
    dir: &Path,
    signers: Option<Arc<RuntimeBundleSignerRegistry>>,
) -> Result<Option<CliRuntimeBundleInstallation>> {
    let metadata_path = dir.join("bundle.json");
    let metadata_value = match std::fs::read(&metadata_path) {
        Ok(bytes) => match serde_json::from_slice::<JsonValue>(&bytes) {
            Ok(value) => Some(value),
            Err(err) => {
                eprintln!(
                    "warning: failed to parse runtime bundle metadata {}: {err}",
                    metadata_path.display()
                );
                None
            }
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => {
            eprintln!(
                "warning: failed to read runtime bundle metadata {}: {err}",
                metadata_path.display()
            );
            None
        }
    };
    let mut bundle_struct: Option<runtime_bundles::RuntimeBundle> = None;
    let mut id = dir
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bundle")
        .to_string();
    let mut name: Option<String> = None;
    let mut adapter: Option<String> = None;
    let mut profiles: Vec<String> = Vec::new();
    let mut modalities: Vec<String> = Vec::new();
    let mut accelerator: Option<String> = None;
    let mut catalog_channel: Option<String> = None;
    let mut installed_at: Option<String> = None;
    let mut imported_at: Option<String> = None;
    let mut source: Option<JsonValue> = None;

    if let Some(metadata) = metadata_value.as_ref() {
        if let Some(bundle_node) = metadata.get("bundle") {
            if let Ok(parsed) =
                serde_json::from_value::<runtime_bundles::RuntimeBundle>(bundle_node.clone())
            {
                id = parsed.id.clone();
                name = Some(parsed.name.clone());
                adapter = Some(parsed.adapter.clone());
                profiles = parsed.profiles.clone();
                modalities = parsed
                    .modalities
                    .iter()
                    .map(|m| modality_slug(m).to_string())
                    .collect();
                accelerator = parsed
                    .accelerator
                    .as_ref()
                    .map(|acc| accelerator_slug(acc).to_string());
                bundle_struct = Some(parsed);
            } else {
                if let Some(bundle_id) = bundle_node
                    .get("id")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    id = bundle_id.to_string();
                }
                name = bundle_node
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                adapter = bundle_node
                    .get("adapter")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                profiles = parse_strings_from_json(bundle_node.get("profiles"));
                modalities = parse_strings_from_json(bundle_node.get("modalities"));
                accelerator = bundle_node
                    .get("accelerator")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
            }
        }
        catalog_channel = metadata
            .pointer("/catalog/channel")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        installed_at = metadata
            .get("installed_at")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        imported_at = metadata
            .get("imported_at")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        source = metadata.get("source").cloned();
    }

    if modalities.is_empty() {
        modalities = parse_strings_from_json(
            metadata_value
                .as_ref()
                .and_then(|value| value.pointer("/bundle/modalities")),
        );
    }

    if accelerator.is_none() {
        accelerator = metadata_value
            .as_ref()
            .and_then(|value| value.pointer("/bundle/accelerator"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
    }

    let signer_registry = signers.as_deref();
    let signature_status = metadata_value.as_ref().map(|metadata| {
        verify_manifest_signatures_with_registry(
            metadata,
            signer_registry,
            catalog_channel.as_deref(),
        )
    });

    let artifacts_dir = dir.join("artifacts");
    let artifacts = load_local_runtime_bundle_artifacts(&artifacts_dir);

    if metadata_value.is_none() && artifacts.is_empty() {
        return Ok(None);
    }

    Ok(Some(CliRuntimeBundleInstallation {
        id,
        name,
        adapter,
        profiles,
        modalities,
        accelerator,
        channel: catalog_channel,
        installed_at,
        imported_at,
        source,
        metadata_path: metadata_value
            .as_ref()
            .map(|_| metadata_path.display().to_string()),
        artifacts,
        root: Some(root.display().to_string()),
        bundle: bundle_struct,
        signature: signature_status,
    }))
}

fn load_local_runtime_bundle_artifacts(dir: &Path) -> Vec<CliRuntimeBundleArtifact> {
    let mut artifacts = Vec::new();
    let read_dir = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return artifacts,
        Err(err) => {
            eprintln!(
                "warning: failed to read runtime bundle artifacts in {}: {err}",
                dir.display()
            );
            return artifacts;
        }
    };
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("warning: failed to read artifact entry: {err}");
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(err) => {
                eprintln!(
                    "warning: failed to read artifact file type {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let bytes = match entry.metadata() {
            Ok(meta) => Some(meta.len()),
            Err(err) => {
                eprintln!(
                    "warning: failed to read artifact metadata {}: {err}",
                    path.display()
                );
                None
            }
        };
        artifacts.push(CliRuntimeBundleArtifact { name, bytes });
    }
    artifacts.sort_by(|a, b| a.name.cmp(&b.name));
    artifacts
}

fn parse_strings_from_json(node: Option<&JsonValue>) -> Vec<String> {
    let mut values = Vec::new();
    if let Some(JsonValue::Array(items)) = node {
        for entry in items {
            if let Some(text) = entry.as_str() {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    values.push(trimmed.to_string());
                }
            }
        }
    }
    values
}

#[cfg(test)]
mod runtime_bundle_list_tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn load_local_installation_with_metadata() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let bundle_dir = root.join("bundle_one");
        std::fs::create_dir_all(bundle_dir.join("artifacts"))?;
        std::fs::write(
            bundle_dir.join("bundle.json"),
            serde_json::to_vec_pretty(&json!({
                "bundle": {
                    "id": "llama.cpp-preview/linux-x86_64-cpu",
                    "name": "Test LLaMA",
                    "adapter": "process",
                    "modalities": ["text"],
                    "accelerator": "cpu",
                    "profiles": ["balanced"]
                },
                "catalog": { "channel": "preview" },
                "installed_at": "2025-10-11T12:00:00Z"
            }))?,
        )?;
        std::fs::write(bundle_dir.join("artifacts").join("weights.bin"), b"content")?;

        let installs = load_local_runtime_bundle_installations(root)?;
        assert_eq!(installs.len(), 1);
        let install = &installs[0];
        assert_eq!(install.id, "llama.cpp-preview/linux-x86_64-cpu");
        assert_eq!(install.name.as_deref(), Some("Test LLaMA"));
        assert_eq!(install.adapter.as_deref(), Some("process"));
        assert_eq!(install.modalities, vec!["text"]);
        assert_eq!(install.accelerator.as_deref(), Some("cpu"));
        assert_eq!(install.channel.as_deref(), Some("preview"));
        assert_eq!(install.artifacts.len(), 1);
        assert_eq!(install.artifacts[0].name, "weights.bin");
        assert!(install.artifacts[0].bytes.is_some());
        let expected_metadata = bundle_dir.join("bundle.json").display().to_string();
        assert_eq!(
            install.metadata_path.as_deref(),
            Some(expected_metadata.as_str())
        );
        assert!(install.bundle.is_some());
        Ok(())
    }

    #[test]
    fn load_local_installation_without_metadata_but_artifacts() -> Result<()> {
        let tmp = tempdir().expect("tempdir");
        let root = tmp.path();
        let bundle_dir = root.join("bare");
        std::fs::create_dir_all(bundle_dir.join("artifacts"))?;
        std::fs::write(bundle_dir.join("artifacts").join("stub.bin"), b"123")?;

        let installs = load_local_runtime_bundle_installations(root)?;
        assert_eq!(installs.len(), 1);
        let install = &installs[0];
        assert_eq!(install.id, "bare");
        assert!(install.name.is_none());
        assert!(install.bundle.is_none());
        assert_eq!(install.artifacts.len(), 1);
        Ok(())
    }
}

fn fetch_runtime_bundle_snapshot_remote(
    base: &RuntimeBaseArgs,
) -> Result<CliRuntimeBundleSnapshot> {
    let token = resolve_admin_token(&base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(base.timeout))
        .build()
        .context("building HTTP client")?;
    let base_url = base.base_url();
    let url = format!("{}/state/runtime/bundles", base_url);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime bundle snapshot from {}", url))?;
    let status = resp.status();
    let text = resp.text().context("reading runtime bundle snapshot")?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        bail!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access runtime bundles"
        );
    }
    if !status.is_success() {
        bail!("runtime bundle request failed: {} {}", status, text.trim());
    }

    let mut snapshot: CliRuntimeBundleSnapshot =
        serde_json::from_str(&text).context("parsing runtime bundle snapshot JSON")?;
    if snapshot.generated.is_none() {
        snapshot.generated = Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    if snapshot.generated_ms.is_none() {
        snapshot.generated_ms = Some(Utc::now().timestamp_millis().max(0) as u64);
    }
    Ok(snapshot)
}

fn cmd_runtime_status(args: &RuntimeStatusArgs) -> Result<()> {
    if args.watch {
        eprintln!("watching runtime supervisor; press Ctrl-C to exit");
        return watch_runtime_status(args);
    }
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let (status, body) = request_runtime_supervisor(&client, base, token.as_deref())?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow::anyhow!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN"
        ));
    }
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "runtime supervisor request failed: {} {}",
            status,
            body
        ));
    }

    let matrix_snapshot = match fetch_runtime_matrix(&client, base, token.as_deref()) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            if args.json {
                return Err(err);
            }
            eprintln!("warning: {}", err);
            None
        }
    };

    if args.json {
        let json = combine_runtime_snapshots(&body, matrix_snapshot.clone());
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string())
            );
        } else {
            println!("{}", json);
        }
        if let Some(path) = args.output.as_ref() {
            append_json_output(path.as_path(), &json, args.pretty, args.output_rotate)?;
        }
        return Ok(());
    }

    let summary = render_runtime_summary(&body);
    println!("{}", summary);
    let mut combined = summary.clone();
    if let Some(matrix) = matrix_snapshot {
        let matrix_text = render_runtime_matrix_summary(&matrix);
        println!();
        println!("{}", matrix_text);
        combined.push_str("\n\n");
        combined.push_str(&matrix_text);
    }
    if let Some(path) = args.output.as_ref() {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        append_text_output(
            path.as_path(),
            Some(stamp.as_str()),
            &combined,
            args.output_rotate,
        )?;
    }
    Ok(())
}

fn watch_runtime_status(args: &RuntimeStatusArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let base_interval = args.interval.max(1);
    let max_backoff = base_interval.max(60);
    let mut sleep_secs = base_interval;

    loop {
        match request_runtime_supervisor(&client, base, token.as_deref()) {
            Ok((status, supervisor)) => {
                if status == StatusCode::UNAUTHORIZED {
                    anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
                }
                if !status.is_success() {
                    eprintln!(
                        "[runtime watch] supervisor request failed: {} {}",
                        status, supervisor
                    );
                    sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
                } else {
                    let matrix_snapshot = match request_runtime_matrix(
                        &client,
                        base,
                        token.as_deref(),
                    ) {
                        Ok((matrix_status, matrix_body)) => {
                            if matrix_status == StatusCode::UNAUTHORIZED {
                                anyhow::bail!(
                                    "runtime matrix request unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN"
                                );
                            }
                            if matrix_status == StatusCode::NOT_FOUND {
                                None
                            } else if !matrix_status.is_success() {
                                eprintln!(
                                    "[runtime watch] matrix request failed: {} {}",
                                    matrix_status, matrix_body
                                );
                                None
                            } else {
                                Some(matrix_body)
                            }
                        }
                        Err(err) => {
                            eprintln!("[runtime watch] error fetching matrix: {err:?}");
                            None
                        }
                    };
                    let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    let summary = render_runtime_summary(&supervisor);
                    println!("=== {} ===", stamp);
                    println!("{}", summary);
                    let mut combined = summary.clone();
                    if let Some(matrix) = matrix_snapshot {
                        let matrix_text = render_runtime_matrix_summary(&matrix);
                        println!();
                        println!("{}", matrix_text);
                        combined.push_str("\n\n");
                        combined.push_str(&matrix_text);
                    }
                    println!();
                    io::stdout().flush().ok();
                    if let Some(path) = args.output.as_ref() {
                        append_text_output(
                            path.as_path(),
                            Some(stamp.as_str()),
                            &combined,
                            args.output_rotate,
                        )?;
                    }
                    sleep_secs = base_interval;
                }
            }
            Err(err) => {
                eprintln!("[runtime watch] error: {err:?}");
                sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
            }
        }

        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

fn cmd_runtime_restore(args: &RuntimeRestoreArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let url = format!("{}/orchestrator/runtimes/{}/restore", base, args.id);

    let mut payload = serde_json::Map::new();
    payload.insert("restart".to_string(), JsonValue::Bool(!args.no_restart));
    if let Some(preset) = &args.preset {
        if !preset.trim().is_empty() {
            payload.insert("preset".to_string(), JsonValue::String(preset.clone()));
        }
    }

    let body = JsonValue::Object(payload);
    let mut req = client.post(&url).json(&body);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime restore for {}", args.id))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime restore response")?;

    match status {
        reqwest::StatusCode::ACCEPTED => {
            let runtime_id = body
                .get("runtime_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&args.id);
            let pending = body
                .get("pending")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            println!(
                "Restore accepted for {} (pending: {}).",
                runtime_id, pending
            );
            if let Some(budget) = body.get("restart_budget") {
                if let Some(line) = budget_summary(budget) {
                    println!("{}", line);
                }
            }
            Ok(())
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            let runtime_id = body
                .get("runtime_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&args.id);
            let reason = body
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("Restart budget exhausted");
            println!("Restore denied for {}: {}", runtime_id, reason);
            if let Some(budget) = body.get("restart_budget") {
                if let Some(line) = budget_summary(budget) {
                    println!("{}", line);
                }
            }
            Err(anyhow::anyhow!("restart budget exhausted"))
        }
        _ => Err(anyhow::anyhow!(
            "runtime restore failed: {} {}",
            status,
            body
        )),
    }
}

fn cmd_runtime_shutdown(args: &RuntimeShutdownArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let url = format!("{}/orchestrator/runtimes/{}/shutdown", base, args.id);

    let mut req = client.post(&url);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime shutdown for {}", args.id))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime shutdown response")?;

    match status {
        reqwest::StatusCode::ACCEPTED => {
            let runtime_id = body
                .get("runtime_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&args.id);
            println!("Shutdown requested for {}.", runtime_id);
            Ok(())
        }
        _ => Err(anyhow::anyhow!(
            "runtime shutdown failed: {} {}",
            status,
            body
        )),
    }
}

fn cmd_runtime_bundles_list(args: &RuntimeBundlesListArgs) -> Result<()> {
    if args.remote {
        return runtime_bundles_list_remote(args);
    }

    let mut snapshot = load_runtime_bundle_snapshot_local(args.dir.clone())?;
    let install_root = default_runtime_bundle_root(&args.install_dir)?;
    let installations = load_local_runtime_bundle_installations(&install_root)?;
    let root_str = install_root.display().to_string();
    if !snapshot.roots.iter().any(|entry| entry == &root_str) {
        snapshot.roots.push(root_str);
    }
    snapshot.installations = installations;
    let (summary, _) = summarize_installations(&snapshot.installations, false);
    snapshot.signature_summary = Some(CliSignatureSummary::from(summary));

    if args.json {
        let payload = serde_json::to_value(&snapshot).unwrap_or_else(|_| json!({}));
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!("{}", payload);
        }
        return Ok(());
    }

    println!("Local runtime bundle inventory");
    print_bundle_summary(
        &snapshot.catalogs,
        &snapshot.roots,
        snapshot.generated.as_deref(),
        &snapshot.installations,
        snapshot.signature_summary.as_ref(),
    );
    Ok(())
}

fn cmd_runtime_bundles_reload(args: &RuntimeBundlesReloadArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let url = format!("{}/admin/runtime/bundles/reload", base);
    let mut req = client.post(&url);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime bundle reload via {}", url))?;
    let status = resp.status();
    let text = resp
        .text()
        .context("reading runtime bundle reload response")?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to reload runtime bundles"
        );
    }
    if !status.is_success() {
        let detail = if text.trim().is_empty() {
            "".to_string()
        } else {
            format!(" {}", text.trim())
        };
        anyhow::bail!("runtime bundle reload failed: {}{}", status, detail);
    }

    let payload: CliRuntimeBundlesReloadResponse =
        serde_json::from_str(&text).context("parsing runtime bundle reload JSON")?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| text.clone())
            );
        } else {
            println!("{}", serde_json::to_string(&payload).unwrap_or(text));
        }
        return Ok(());
    }

    if payload.ok {
        println!("Runtime bundle catalogs reloaded.");
    } else if let Some(err) = payload.error {
        anyhow::bail!("runtime bundle reload failed: {}", err);
    } else {
        anyhow::bail!("runtime bundle reload failed");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum SnapshotKind {
    Install,
    Import,
    Rollback,
}

impl SnapshotKind {
    fn as_str(&self) -> &'static str {
        match self {
            SnapshotKind::Install => "install",
            SnapshotKind::Import => "import",
            SnapshotKind::Rollback => "rollback",
        }
    }
}

fn history_timestamp() -> String {
    Utc::now().format("%Y%m%dT%H%M%S%3fZ").to_string()
}

#[derive(Debug, Clone)]
struct BundleHistoryEntry {
    revision: String,
    path: PathBuf,
    saved_at: Option<String>,
    kind: Option<String>,
    summary: Option<JsonValue>,
}

fn snapshot_existing_bundle(
    bundle_dir: &Path,
    bundle_id: &str,
    kind: SnapshotKind,
    dry_run: bool,
) -> Result<Option<BundleHistoryEntry>> {
    let bundle_json = bundle_dir.join("bundle.json");
    let artifacts_dir = bundle_dir.join("artifacts");
    let payload_dir = bundle_dir.join("payload");
    let mut has_state = false;
    has_state |= bundle_json.exists();
    has_state |= artifacts_dir.exists();
    has_state |= payload_dir.exists();
    if !has_state {
        return Ok(None);
    }

    let summary = if bundle_json.is_file() {
        match std::fs::read(&bundle_json) {
            Ok(bytes) => serde_json::from_slice::<JsonValue>(&bytes)
                .ok()
                .and_then(|value| bundle_history_summary(&value, bundle_id)),
            Err(_) => None,
        }
    } else {
        None
    };

    let timestamp = history_timestamp();
    let revision = format!("rev-{}", timestamp);
    if dry_run {
        println!(
            "[dry-run] would snapshot existing bundle {} into history/{} ({})",
            bundle_id,
            revision,
            kind.as_str()
        );
        return Ok(Some(BundleHistoryEntry {
            revision,
            path: bundle_dir
                .join("history")
                .join(format!("rev-{}", timestamp)),
            saved_at: Some(timestamp),
            kind: Some(kind.as_str().to_string()),
            summary,
        }));
    }

    let history_root = bundle_dir.join("history");
    create_dir_all(&history_root)
        .with_context(|| format!("creating {}", history_root.display()))?;
    let entry_dir = history_root.join(&revision);
    create_dir_all(&entry_dir).with_context(|| format!("creating {}", entry_dir.display()))?;

    if bundle_json.exists() {
        let target = entry_dir.join("bundle.json");
        std::fs::rename(&bundle_json, &target).with_context(|| {
            format!(
                "moving bundle.json to history revision {}",
                entry_dir.display()
            )
        })?;
    }
    if artifacts_dir.exists() {
        let target = entry_dir.join("artifacts");
        std::fs::rename(&artifacts_dir, &target).with_context(|| {
            format!(
                "moving artifacts directory to history revision {}",
                entry_dir.display()
            )
        })?;
    }
    if payload_dir.exists() {
        let target = entry_dir.join("payload");
        std::fs::rename(&payload_dir, &target).with_context(|| {
            format!(
                "moving payload directory to history revision {}",
                entry_dir.display()
            )
        })?;
    }

    let mut info_map = JsonMap::new();
    info_map.insert("bundle".into(), JsonValue::String(bundle_id.to_string()));
    info_map.insert("saved_at".into(), JsonValue::String(timestamp.clone()));
    info_map.insert("kind".into(), JsonValue::String(kind.as_str().to_string()));
    if let Some(summary_value) = summary.clone() {
        info_map.insert("summary".into(), summary_value);
    }
    let info = JsonValue::Object(info_map);
    let info_path = entry_dir.join("info.json");
    std::fs::write(&info_path, serde_json::to_vec_pretty(&info)?)
        .with_context(|| format!("writing history metadata {}", info_path.display()))?;

    Ok(Some(BundleHistoryEntry {
        revision,
        path: entry_dir,
        saved_at: Some(timestamp),
        kind: Some(kind.as_str().to_string()),
        summary,
    }))
}

fn list_bundle_history(bundle_dir: &Path) -> Result<Vec<BundleHistoryEntry>> {
    let history_root = bundle_dir.join("history");
    if !history_root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&history_root)
        .with_context(|| format!("reading history directory {}", history_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("rev-") {
            continue;
        }
        let info_path = entry.path().join("info.json");
        let (saved_at, kind, summary) = if info_path.exists() {
            match std::fs::read_to_string(&info_path)
                .ok()
                .and_then(|s| serde_json::from_str::<JsonValue>(&s).ok())
            {
                Some(value) => {
                    let saved_at = value
                        .get("saved_at")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let kind = value
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let summary = value.get("summary").cloned();
                    (saved_at, kind, summary)
                }
                None => (None, None, None),
            }
        } else {
            (None, None, None)
        };
        entries.push(BundleHistoryEntry {
            revision: name_str.to_string(),
            path: entry.path(),
            saved_at,
            kind,
            summary,
        });
    }
    entries.sort_by(|a, b| b.revision.cmp(&a.revision));
    Ok(entries)
}

#[derive(Debug)]
enum DownloadStatus {
    Downloaded { path: PathBuf, bytes: u64 },
    SkippedExisting { path: PathBuf },
    DryRun { path: PathBuf },
}

fn default_runtime_bundle_root(explicit: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = explicit {
        return Ok(dir.clone());
    }
    let paths = load_effective_paths();
    let state_dir = paths
        .get("state_dir")
        .and_then(JsonValue::as_str)
        .map(PathBuf::from)
        .context("state_dir missing from effective paths")?;
    Ok(state_dir.join("runtime").join("bundles"))
}

fn sanitize_bundle_dir(id: &str) -> String {
    let mut base = String::with_capacity(id.len());
    for ch in id.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' => base.push(ch),
            '-' | '_' | '.' => base.push(ch),
            '/' | '\\' => base.push_str("__"),
            _ => base.push('_'),
        }
    }
    if base.is_empty() {
        base.push_str("bundle");
    }
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    let mut suffix = String::new();
    for byte in digest.iter().take(4) {
        write!(&mut suffix, "{:02x}", byte).expect("hex write");
    }
    format!("{}__{}", base, suffix)
}

fn sanitize_filename(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len().max(1));
    for ch in segment.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' => out.push(ch),
            '-' | '_' | '.' => out.push(ch),
            _ => out.push('_'),
        }
    }
    if out.is_empty() {
        "_".into()
    } else {
        out
    }
}

fn artifact_file_name(artifact: &runtime_bundles::RuntimeBundleArtifact, index: usize) -> String {
    if let Some(url) = artifact.url.as_deref() {
        let trimmed = url.split('?').next().unwrap_or(url).trim_end_matches('/');
        if let Some(name) = trimmed.rsplit('/').next() {
            let clean = sanitize_filename(name);
            if clean != "_" {
                return clean;
            }
        }
    }
    if let Some(fmt) = artifact.format.as_deref() {
        sanitize_filename(&format!("{}-{:02}.{}", artifact.kind, index + 1, fmt))
    } else {
        sanitize_filename(&format!("{}-{:02}", artifact.kind, index + 1))
    }
}

fn artifact_matches_filters(
    artifact: &runtime_bundles::RuntimeBundleArtifact,
    kind_filters: &HashSet<String>,
    format_filters: &HashSet<String>,
) -> bool {
    if !kind_filters.is_empty() {
        let kind = artifact.kind.to_ascii_lowercase();
        if !kind_filters.contains(&kind) {
            return false;
        }
    }
    if !format_filters.is_empty() {
        let fmt = artifact
            .format
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if fmt.is_empty() {
            return false;
        }
        if !format_filters.contains(&fmt) {
            return false;
        }
    }
    true
}

fn download_bundle_artifact(
    client: &Client,
    url: &str,
    dest: &Path,
    sha256: Option<&str>,
    dry_run: bool,
    force: bool,
) -> Result<DownloadStatus> {
    if dry_run {
        return Ok(DownloadStatus::DryRun {
            path: dest.to_path_buf(),
        });
    }

    if let Some(parent) = dest.parent() {
        create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    if dest.exists() {
        if force {
            std::fs::remove_file(dest)
                .with_context(|| format!("removing existing artifact {}", dest.display()))?;
        } else {
            return Ok(DownloadStatus::SkippedExisting {
                path: dest.to_path_buf(),
            });
        }
    }

    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("downloading artifact from {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("download failed ({}) for {}", status, url);
    }

    let parent_dir = dest
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut tmp = tempfile::Builder::new()
        .prefix("arw-bundle-")
        .tempfile_in(&parent_dir)
        .context("allocating temporary download file")?;
    let mut hasher = Sha256::new();
    let mut total_bytes = 0u64;
    {
        let writer = tmp.as_file_mut();
        let mut buf = [0u8; 8192];
        loop {
            let count = resp
                .read(&mut buf)
                .with_context(|| format!("reading bytes from {}", url))?;
            if count == 0 {
                break;
            }
            writer
                .write_all(&buf[..count])
                .with_context(|| format!("writing chunk to {}", dest.display()))?;
            hasher.update(&buf[..count]);
            total_bytes += count as u64;
        }
        writer
            .flush()
            .with_context(|| format!("flushing downloaded artifact to {}", dest.display()))?;
    }
    let digest = hasher.finalize();
    if let Some(expected) = sha256 {
        let expected_norm = expected.trim().to_ascii_lowercase();
        let mut actual = String::new();
        for byte in digest.iter() {
            write!(&mut actual, "{:02x}", byte).expect("hex write");
        }
        if actual != expected_norm {
            anyhow::bail!(
                "artifact hash mismatch for {} (expected {}, got {})",
                url,
                expected,
                actual
            );
        }
    }

    tmp.persist(dest)
        .with_context(|| format!("persisting download to {}", dest.display()))?;
    Ok(DownloadStatus::Downloaded {
        path: dest.to_path_buf(),
        bytes: total_bytes,
    })
}

fn write_bundle_metadata_json(
    bundle_dir: &Path,
    metadata: &JsonValue,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let metadata_path = bundle_dir.join("bundle.json");
    if dry_run {
        println!(
            "[dry-run] would write metadata to {}",
            metadata_path.display()
        );
        return Ok(());
    }
    if metadata_path.exists() {
        if force {
            std::fs::remove_file(&metadata_path)
                .with_context(|| format!("removing {}", metadata_path.display()))?;
        } else {
            println!(
                "Metadata already exists at {} (use --force to overwrite)",
                metadata_path.display()
            );
            return Ok(());
        }
    }
    if let Some(parent) = metadata_path.parent() {
        create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let bytes =
        serde_json::to_vec_pretty(metadata).context("serializing bundle metadata to JSON")?;
    std::fs::write(&metadata_path, bytes)
        .with_context(|| format!("writing {}", metadata_path.display()))?;
    println!("Wrote metadata {}", metadata_path.display());
    Ok(())
}

fn cmd_runtime_bundles_install(args: &RuntimeBundlesInstallArgs) -> Result<()> {
    let snapshot = if args.remote {
        fetch_runtime_bundle_snapshot_remote(&args.base)?
    } else {
        load_runtime_bundle_snapshot_local(args.dir.clone())?
    };
    if snapshot.catalogs.is_empty() {
        anyhow::bail!("no runtime bundle catalogs discovered");
    }

    let install_root = default_runtime_bundle_root(&args.dest)?;
    if args.dry_run {
        println!("[dry-run] bundle install root: {}", install_root.display());
    } else {
        if install_root.exists() && !install_root.is_dir() {
            anyhow::bail!(
                "install root {} exists but is not a directory",
                install_root.display()
            );
        }
        create_dir_all(&install_root)
            .with_context(|| format!("creating {}", install_root.display()))?;
    }

    let kind_filters: HashSet<String> = args
        .artifact_kinds
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    let format_filters: HashSet<String> = args
        .artifact_formats
        .iter()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .collect();
    let roots_snapshot = snapshot.roots.clone();
    let source_info: JsonValue = if args.remote {
        json!({ "kind": "remote", "base": args.base.base_url() })
    } else {
        json!({ "kind": "local", "roots": roots_snapshot })
    };

    let mut missing: Vec<String> = Vec::new();
    let mut total_downloads = 0usize;
    let mut total_bytes = 0u64;

    let mut client_builder = Client::builder();
    if args.base.timeout == 0 {
        client_builder = client_builder.timeout(None);
    } else {
        client_builder = client_builder.timeout(Duration::from_secs(args.base.timeout.max(60)));
    }
    client_builder = client_builder.user_agent("arw-cli/managed-runtime");
    let download_client = client_builder.build().context("building download client")?;

    for bundle_id in &args.bundles {
        let mut matched: Option<(CliRuntimeBundleCatalog, runtime_bundles::RuntimeBundle)> = None;
        for catalog in &snapshot.catalogs {
            if let Some(bundle) = catalog.bundles.iter().find(|b| &b.id == bundle_id) {
                matched = Some((catalog.clone(), bundle.clone()));
                break;
            }
        }
        let Some((catalog, bundle)) = matched else {
            missing.push(bundle_id.clone());
            continue;
        };

        let dir_name = sanitize_bundle_dir(bundle_id);
        let bundle_dir = install_root.join(&dir_name);
        let artifacts_dir = bundle_dir.join("artifacts");

        let existing_state = bundle_dir.exists()
            && (bundle_dir.join("bundle.json").exists()
                || artifacts_dir.exists()
                || bundle_dir.join("payload").exists());
        if existing_state && !args.force {
            if args.dry_run {
                println!(
                    "[dry-run] bundle {} already staged at {} (would require --force)",
                    bundle_id,
                    bundle_dir.display()
                );
                continue;
            } else {
                anyhow::bail!(
                    "bundle {} already staged at {} (use --force to overwrite)",
                    bundle_id,
                    bundle_dir.display()
                );
            }
        }

        if existing_state {
            snapshot_existing_bundle(&bundle_dir, bundle_id, SnapshotKind::Install, args.dry_run)?;
        }

        if args.dry_run {
            println!(
                "[dry-run] install bundle {} -> {}",
                bundle_id,
                bundle_dir.display()
            );
        } else {
            create_dir_all(&artifacts_dir).with_context(|| {
                format!("creating artifacts directory {}", artifacts_dir.display())
            })?;
        }

        let mut downloaded = 0usize;
        let mut skipped_existing = 0usize;
        let mut missing_urls = 0usize;

        for (idx, artifact) in bundle.artifacts.iter().enumerate() {
            if !artifact_matches_filters(artifact, &kind_filters, &format_filters) {
                continue;
            }
            let Some(url) = artifact.url.as_deref() else {
                missing_urls += 1;
                println!(
                    "Bundle {} artifact {} has no URL; use `arw-cli runtime bundles import` to stage it manually.",
                    bundle_id,
                    artifact.kind
                );
                continue;
            };
            let file_name = artifact_file_name(artifact, idx);
            let dest_path = artifacts_dir.join(file_name);
            match download_bundle_artifact(
                &download_client,
                url,
                &dest_path,
                artifact.sha256.as_deref(),
                args.dry_run,
                args.force,
            )? {
                DownloadStatus::Downloaded { path, bytes } => {
                    downloaded += 1;
                    total_downloads += 1;
                    total_bytes += bytes;
                    println!("Downloaded {} ({} bytes)", path.display(), bytes);
                }
                DownloadStatus::SkippedExisting { path } => {
                    skipped_existing += 1;
                    println!(
                        "Skipping existing artifact {} (use --force to overwrite)",
                        path.display()
                    );
                }
                DownloadStatus::DryRun { path } => {
                    println!("[dry-run] would download {} -> {}", url, path.display());
                    downloaded += 1;
                }
            }
        }

        let metadata = json!({
            "bundle": bundle,
            "catalog": {
                "path": catalog.path,
                "channel": catalog.channel,
                "notes": catalog.notes,
            },
            "source": source_info.clone(),
            "installed_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        });
        write_bundle_metadata_json(&bundle_dir, &metadata, args.dry_run, args.force)?;

        println!(
            "Bundle {} summary: {} downloaded, {} skipped (existing), {} without URLs.",
            bundle_id, downloaded, skipped_existing, missing_urls
        );
    }

    if !missing.is_empty() {
        anyhow::bail!("bundle id(s) not found in catalog: {}", missing.join(", "));
    }

    if args.dry_run {
        println!(
            "[dry-run] install complete ({} bundle{} requested).",
            args.bundles.len(),
            if args.bundles.len() == 1 { "" } else { "s" }
        );
    } else {
        println!("Install root: {}", install_root.display());
        if total_downloads > 0 {
            println!(
                "Downloaded {} artifact{} ({} bytes).",
                total_downloads,
                if total_downloads == 1 { "" } else { "s" },
                total_bytes
            );
        }
    }
    Ok(())
}

fn copy_file_into(src: &Path, dest_dir: &Path, force: bool, dry_run: bool) -> Result<u64> {
    let file_name = src
        .file_name()
        .ok_or_else(|| anyhow!("source file {} has no name", src.display()))?;
    let dest_path = dest_dir.join(file_name);
    if dry_run {
        println!(
            "[dry-run] copy {} -> {}",
            src.display(),
            dest_path.display()
        );
        return Ok(0);
    }
    if !src.is_file() {
        anyhow::bail!("{} is not a file", src.display());
    }
    if let Some(parent) = dest_path.parent() {
        create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if dest_path.exists() {
        if force {
            std::fs::remove_file(&dest_path)
                .with_context(|| format!("removing {}", dest_path.display()))?;
        } else {
            anyhow::bail!(
                "destination file {} exists (use --force to overwrite)",
                dest_path.display()
            );
        }
    }
    let bytes = std::fs::copy(src, &dest_path)
        .with_context(|| format!("copying {} -> {}", src.display(), dest_path.display()))?;
    println!("Copied file {} -> {}", src.display(), dest_path.display());
    Ok(bytes)
}

fn copy_directory_into(
    src: &Path,
    dest_dir: &Path,
    force: bool,
    dry_run: bool,
) -> Result<(usize, u64)> {
    if !src.is_dir() {
        anyhow::bail!("{} is not a directory", src.display());
    }
    let dir_name = src
        .file_name()
        .ok_or_else(|| anyhow!("source directory {} has no name", src.display()))?;
    let target_root = dest_dir.join(dir_name);
    if dry_run {
        println!(
            "[dry-run] copy directory {} -> {}",
            src.display(),
            target_root.display()
        );
        return Ok((0, 0));
    }
    if target_root.exists() {
        if force {
            if target_root.is_file() {
                std::fs::remove_file(&target_root)
                    .with_context(|| format!("removing {}", target_root.display()))?;
            } else {
                std::fs::remove_dir_all(&target_root)
                    .with_context(|| format!("removing {}", target_root.display()))?;
            }
        } else {
            anyhow::bail!(
                "destination {} exists (use --force to overwrite)",
                target_root.display()
            );
        }
    }
    create_dir_all(&target_root).with_context(|| format!("creating {}", target_root.display()))?;

    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in WalkDir::new(src) {
        let entry = entry.with_context(|| format!("walking {}", src.display()))?;
        let path = entry.path();
        let rel = match path.strip_prefix(src) {
            Ok(r) if !r.as_os_str().is_empty() => r,
            _ => continue,
        };
        let target = target_root.join(rel);
        if entry.file_type().is_dir() {
            create_dir_all(&target).with_context(|| format!("creating {}", target.display()))?;
            continue;
        }
        if entry.file_type().is_symlink() {
            anyhow::bail!(
                "symlink {} not supported during import; flatten archives before importing",
                path.display()
            );
        }
        if let Some(parent) = target.parent() {
            create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let copied =
            std::fs::copy(path, &target).with_context(|| format!("copying {}", path.display()))?;
        files += 1;
        bytes += copied;
    }
    println!(
        "Copied directory {} -> {}",
        src.display(),
        target_root.display()
    );
    Ok((files, bytes))
}

fn cmd_runtime_bundles_import(args: &RuntimeBundlesImportArgs) -> Result<()> {
    let bundle_root = default_runtime_bundle_root(&args.dest)?;
    if args.dry_run {
        println!("[dry-run] bundle import root: {}", bundle_root.display());
    } else {
        if bundle_root.exists() && !bundle_root.is_dir() {
            anyhow::bail!(
                "bundle root {} exists but is not a directory",
                bundle_root.display()
            );
        }
        create_dir_all(&bundle_root)
            .with_context(|| format!("creating {}", bundle_root.display()))?;
    }

    let dir_name = sanitize_bundle_dir(&args.bundle);
    let bundle_dir = bundle_root.join(&dir_name);
    let payload_dir = bundle_dir.join("payload");
    let artifacts_dir = bundle_dir.join("artifacts");
    let existing_state = bundle_dir.exists()
        && (bundle_dir.join("bundle.json").exists()
            || artifacts_dir.exists()
            || payload_dir.exists());
    if existing_state && !args.force {
        if args.dry_run {
            println!(
                "[dry-run] bundle {} already staged at {} (would require --force)",
                args.bundle,
                bundle_dir.display()
            );
            return Ok(());
        } else {
            anyhow::bail!(
                "bundle {} already staged at {} (use --force to overwrite)",
                args.bundle,
                bundle_dir.display()
            );
        }
    }

    if existing_state {
        snapshot_existing_bundle(
            &bundle_dir,
            &args.bundle,
            SnapshotKind::Import,
            args.dry_run,
        )?;
    }

    if args.dry_run {
        println!(
            "[dry-run] bundle {} staged at {}",
            args.bundle,
            bundle_dir.display()
        );
    } else {
        if bundle_dir.exists() && !bundle_dir.is_dir() {
            anyhow::bail!(
                "bundle path {} exists but is not a directory",
                bundle_dir.display()
            );
        }
        create_dir_all(&bundle_dir)
            .with_context(|| format!("creating {}", bundle_dir.display()))?;
        if payload_dir.exists() && !payload_dir.is_dir() {
            anyhow::bail!(
                "payload path {} exists but is not a directory",
                payload_dir.display()
            );
        }
        create_dir_all(&payload_dir)
            .with_context(|| format!("creating {}", payload_dir.display()))?;
    }

    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    for path in &args.paths {
        if !path.exists() {
            anyhow::bail!("path {} does not exist", path.display());
        }
        if path.is_file() {
            let bytes = copy_file_into(path, &payload_dir, args.force, args.dry_run)?;
            if !args.dry_run {
                total_files += 1;
                total_bytes += bytes;
            }
        } else if path.is_dir() {
            let (files, bytes) = copy_directory_into(path, &payload_dir, args.force, args.dry_run)?;
            if !args.dry_run {
                total_files += files;
                total_bytes += bytes;
            }
        } else {
            anyhow::bail!("{} is neither file nor directory", path.display());
        }
    }

    if let Some(meta_src) = &args.metadata {
        if !meta_src.exists() {
            anyhow::bail!("metadata file {} does not exist", meta_src.display());
        }
        let metadata_path = bundle_dir.join("bundle.json");
        if args.dry_run {
            println!(
                "[dry-run] would copy metadata {} -> {}",
                meta_src.display(),
                metadata_path.display()
            );
        } else {
            if metadata_path.exists() {
                if args.force {
                    std::fs::remove_file(&metadata_path)
                        .with_context(|| format!("removing {}", metadata_path.display()))?;
                } else {
                    anyhow::bail!(
                        "metadata file {} exists (use --force to overwrite)",
                        metadata_path.display()
                    );
                }
            }
            if let Some(parent) = metadata_path.parent() {
                create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::copy(meta_src, &metadata_path)
                .with_context(|| format!("copying metadata {}", meta_src.display()))?;
            println!("Imported metadata {}", metadata_path.display());
        }
    } else {
        let metadata = json!({
            "bundle": {
                "id": args.bundle,
            },
            "source": {
                "kind": "import",
            },
            "imported_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        });
        write_bundle_metadata_json(&bundle_dir, &metadata, args.dry_run, args.force)?;
    }

    if args.dry_run {
        println!(
            "[dry-run] import complete for bundle {} (payload at {})",
            args.bundle,
            payload_dir.display()
        );
    } else {
        println!(
            "Imported bundle {} into {} ({} file{}, {} bytes).",
            args.bundle,
            bundle_dir.display(),
            total_files,
            if total_files == 1 { "" } else { "s" },
            total_bytes
        );
    }
    Ok(())
}

fn copy_path_recursive(src: &Path, dest: &Path) -> Result<()> {
    if src.is_file() {
        if let Some(parent) = dest.parent() {
            create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::copy(src, dest)
            .with_context(|| format!("copying file {} -> {}", src.display(), dest.display()))?;
        return Ok(());
    }

    if src.is_dir() {
        create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
        for entry in WalkDir::new(src) {
            let entry = entry.with_context(|| format!("walking {}", src.display()))?;
            let path = entry.path();
            let rel = match path.strip_prefix(src) {
                Ok(rel) if !rel.as_os_str().is_empty() => rel,
                _ => continue,
            };
            let target = dest.join(rel);
            if entry.file_type().is_dir() {
                create_dir_all(&target)
                    .with_context(|| format!("creating {}", target.display()))?;
                continue;
            }
            if entry.file_type().is_symlink() {
                anyhow::bail!(
                    "symlink {} not supported during rollback copy",
                    path.display()
                );
            }
            if let Some(parent) = target.parent() {
                create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::copy(path, &target)
                .with_context(|| format!("copying {} -> {}", path.display(), target.display()))?;
        }
        return Ok(());
    }

    anyhow::bail!("unsupported path type {}", src.display());
}

fn bundle_history_summary(metadata: &JsonValue, fallback_id: &str) -> Option<JsonValue> {
    let mut map = JsonMap::new();
    let id = metadata
        .pointer("/bundle/id")
        .and_then(|v| v.as_str())
        .unwrap_or(fallback_id);
    map.insert("id".into(), JsonValue::String(id.to_string()));

    if let Some(name) = metadata.pointer("/bundle/name").and_then(|v| v.as_str()) {
        map.insert("name".into(), JsonValue::String(name.to_string()));
    }
    if let Some(adapter) = metadata.pointer("/bundle/adapter").and_then(|v| v.as_str()) {
        map.insert("adapter".into(), JsonValue::String(adapter.to_string()));
    }
    if let Some(channel) = metadata
        .pointer("/catalog/channel")
        .and_then(|v| v.as_str())
    {
        map.insert("channel".into(), JsonValue::String(channel.to_string()));
    }
    if let Some(source_kind) = metadata.pointer("/source/kind").and_then(|v| v.as_str()) {
        map.insert(
            "source_kind".into(),
            JsonValue::String(source_kind.to_string()),
        );
    }
    if let Some(modalities) = metadata
        .pointer("/bundle/modalities")
        .and_then(|v| v.as_array())
    {
        let list: Vec<JsonValue> = modalities
            .iter()
            .filter_map(|v| v.as_str().map(|s| JsonValue::String(s.to_string())))
            .collect();
        if !list.is_empty() {
            map.insert("modalities".into(), JsonValue::Array(list));
        }
    }
    if let Some(profiles) = metadata
        .pointer("/bundle/profiles")
        .and_then(|v| v.as_array())
    {
        let list: Vec<JsonValue> = profiles
            .iter()
            .filter_map(|v| v.as_str().map(|s| JsonValue::String(s.to_string())))
            .collect();
        if !list.is_empty() {
            map.insert("profiles".into(), JsonValue::Array(list));
        }
    }

    if map.is_empty() {
        None
    } else {
        Some(JsonValue::Object(map))
    }
}

fn format_history_summary(summary: &JsonValue) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(name) = summary.get("name").and_then(|v| v.as_str()) {
        parts.push(name.to_string());
    }
    if let Some(channel) = summary.get("channel").and_then(|v| v.as_str()) {
        parts.push(format!("channel: {}", channel));
    }
    if let Some(adapter) = summary.get("adapter").and_then(|v| v.as_str()) {
        parts.push(format!("adapter: {}", adapter));
    }
    if let Some(mods) = summary
        .get("modalities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
    {
        if !mods.is_empty() {
            parts.push(format!("modalities: {}", mods.join("/")));
        }
    }
    if let Some(profiles) = summary
        .get("profiles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
    {
        if !profiles.is_empty() {
            parts.push(format!("profiles: {}", profiles.join("/")));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn cmd_runtime_bundles_rollback(args: &RuntimeBundlesRollbackArgs) -> Result<()> {
    let bundle_root = default_runtime_bundle_root(&args.dest)?;
    let dir_name = sanitize_bundle_dir(&args.bundle);
    let bundle_dir = bundle_root.join(&dir_name);
    if !bundle_dir.exists() {
        anyhow::bail!(
            "bundle {} not staged at {}",
            args.bundle,
            bundle_dir.display()
        );
    }

    let history = list_bundle_history(&bundle_dir)?;
    if history.is_empty() {
        anyhow::bail!("no history revisions available for bundle {}", args.bundle);
    }

    if args.list {
        if args.json {
            let items: Vec<JsonValue> = history
                .iter()
                .map(|entry| {
                    json!({
                        "revision": &entry.revision,
                        "saved_at": entry.saved_at,
                        "kind": entry.kind,
                        "path": entry.path.display().to_string(),
                        "summary": entry.summary.clone(),
                    })
                })
                .collect();
            let payload = json!({
                "bundle": args.bundle,
                "history": items,
            });
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
                );
            } else {
                println!("{}", payload);
            }
        } else {
            println!("Revisions for bundle {}:", args.bundle);
            for entry in &history {
                let saved_at = entry.saved_at.as_deref().unwrap_or("<unknown>");
                let kind = entry.kind.as_deref().unwrap_or("<unknown>");
                let mut line = format!(
                    "  - {} (saved_at: {}, kind: {}",
                    entry.revision, saved_at, kind
                );
                if let Some(summary_label) = entry.summary.as_ref().and_then(format_history_summary)
                {
                    line.push_str(&format!("; {}", summary_label));
                }
                line.push(')');
                println!("{}", line);
            }
        }
        return Ok(());
    }

    let desired_revision = if let Some(rev) = &args.revision {
        if rev.starts_with("rev-") {
            rev.clone()
        } else {
            format!("rev-{}", rev)
        }
    } else {
        history
            .first()
            .map(|entry| entry.revision.clone())
            .expect("history is non-empty")
    };

    let target = history
        .into_iter()
        .find(|entry| entry.revision == desired_revision)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "revision {} not found for bundle {}",
                desired_revision,
                args.bundle
            )
        })?;

    if args.dry_run {
        if args.json {
            let payload = json!({
                "bundle": args.bundle,
                "revision": target.revision,
                "dry_run": true,
            });
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
                );
            } else {
                println!("{}", payload);
            }
        } else {
            println!(
                "[dry-run] would restore bundle {} to revision {}",
                args.bundle, target.revision
            );
        }
        return Ok(());
    }

    let snapshot =
        snapshot_existing_bundle(&bundle_dir, &args.bundle, SnapshotKind::Rollback, false)?;
    if !args.json {
        if let Some(snap) = &snapshot {
            println!(
                "Saved current state to history/{} before rollback",
                snap.revision
            );
        }
    }

    let mut restored_components = Vec::new();
    let mut missing_components = Vec::new();
    for component in ["bundle.json", "artifacts", "payload"] {
        let src = target.path.join(component);
        if src.exists() {
            let dest = bundle_dir.join(component);
            if dest.exists() {
                if dest.is_dir() {
                    std::fs::remove_dir_all(&dest).with_context(|| {
                        format!("removing existing directory {}", dest.display())
                    })?;
                } else {
                    std::fs::remove_file(&dest)
                        .with_context(|| format!("removing file {}", dest.display()))?;
                }
            }
            copy_path_recursive(&src, &dest).with_context(|| {
                format!("restoring {} from revision {}", component, target.revision)
            })?;
            restored_components.push(component.to_string());
        } else {
            missing_components.push(component.to_string());
        }
    }

    let info_path = target.path.join("info.json");
    if info_path.exists() {
        if let Some(mut info) = std::fs::read_to_string(&info_path)
            .ok()
            .and_then(|s| serde_json::from_str::<JsonValue>(&s).ok())
        {
            info["restored_at"] =
                JsonValue::String(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
            std::fs::write(&info_path, serde_json::to_vec_pretty(&info)?)
                .with_context(|| format!("updating history metadata {}", info_path.display()))?;
        }
    }

    if args.json {
        let payload = json!({
            "bundle": args.bundle,
            "restored_revision": target.revision,
            "restored_components": restored_components,
            "missing_components": missing_components,
            "snapshot_created": snapshot.as_ref().map(|s| s.revision.clone()),
        });
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!("{}", payload);
        }
    } else {
        if !restored_components.is_empty() {
            println!(
                "Rolled bundle {} back to {} (restored: {}).",
                args.bundle,
                target.revision,
                restored_components.join(", ")
            );
        } else {
            println!(
                "Rolled bundle {} back to {} (no components restored).",
                args.bundle, target.revision
            );
        }
        if !missing_components.is_empty() {
            println!(
                "Revision {} missing components: {}",
                target.revision,
                missing_components.join(", ")
            );
        }
    }

    Ok(())
}

fn cmd_runtime_bundles_manifest_sign(args: &RuntimeBundlesManifestSignArgs) -> Result<()> {
    use ed25519_dalek::{Signer, SigningKey};

    let manifest_path = &args.manifest;
    let manifest_bytes = std::fs::read(manifest_path)
        .with_context(|| format!("reading bundle manifest {}", manifest_path.display()))?;
    let mut manifest: JsonValue =
        serde_json::from_slice(&manifest_bytes).context("parsing bundle manifest JSON")?;
    if !manifest.is_object() {
        anyhow::bail!("bundle manifest root must be a JSON object");
    }

    let sk_b64 = if let Some(ref value) = args.key_b64 {
        value.trim().to_string()
    } else if let Some(ref path) = args.key_file {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading secret key from {}", path.display()))?
            .trim()
            .to_string()
    } else {
        anyhow::bail!("provide --key-b64 or --key-file with an ed25519 secret key");
    };
    let sk_bytes = base64::engine::general_purpose::STANDARD
        .decode(sk_b64.as_bytes())
        .context("decoding ed25519 secret key (base64)")?;
    let signing_key = SigningKey::from_bytes(
        &sk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("ed25519 secret key must be 32 bytes"))?,
    );
    let vk = signing_key.verifying_key();
    let pk_bytes = vk.to_bytes();
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk_bytes);
    let key_id = args
        .key_id
        .clone()
        .unwrap_or_else(|| default_manifest_key_id(&pk_bytes));

    let (payload_bytes, payload_sha_hex) = canonical_payload_bytes(&manifest)?;
    let signature = signing_key.sign(&payload_bytes);
    let signature_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
    let issued_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let manifest_obj = manifest.as_object_mut().expect("checked object above");
    let signatures_entry = manifest_obj
        .entry("signatures".to_string())
        .or_insert_with(|| JsonValue::Array(Vec::new()));
    let signatures = signatures_entry
        .as_array_mut()
        .ok_or_else(|| anyhow!("manifest signatures entry must be an array"))?;
    signatures.retain(|entry| {
        entry
            .get("key_id")
            .and_then(|value| value.as_str())
            .map(|value| value != key_id)
            .unwrap_or(true)
    });

    let mut sig_map = JsonMap::new();
    sig_map.insert("alg".into(), JsonValue::String("ed25519".into()));
    sig_map.insert("key_id".into(), JsonValue::String(key_id.clone()));
    sig_map.insert("public_key_b64".into(), JsonValue::String(pk_b64.clone()));
    sig_map.insert("signature".into(), JsonValue::String(signature_b64.clone()));
    sig_map.insert(
        "manifest_sha256".into(),
        JsonValue::String(format!("sha256:{}", payload_sha_hex)),
    );
    sig_map.insert("issued_at".into(), JsonValue::String(issued_at.clone()));
    if let Some(ref issuer) = args.issuer {
        if !issuer.trim().is_empty() {
            sig_map.insert(
                "issuer".into(),
                JsonValue::String(issuer.trim().to_string()),
            );
        }
    }
    signatures.push(JsonValue::Object(sig_map));

    let output_path = args.output.as_ref().unwrap_or(manifest_path);
    let formatted = if args.compact {
        serde_json::to_vec(&manifest).context("serializing signed manifest")?
    } else {
        serde_json::to_vec_pretty(&manifest).context("serializing signed manifest")?
    };
    std::fs::write(output_path, formatted)
        .with_context(|| format!("writing signed manifest to {}", output_path.display()))?;

    println!(
        "Signed manifest {} with key {}",
        output_path.display(),
        key_id
    );
    println!("Canonical payload sha256: {}", payload_sha_hex);
    println!("Public key (b64): {}", pk_b64);
    println!("Signature (b64): {}", signature_b64);
    Ok(())
}

fn cmd_runtime_bundles_manifest_verify(args: &RuntimeBundlesManifestVerifyArgs) -> Result<()> {
    let manifest_path = &args.manifest;
    let manifest_bytes = std::fs::read(manifest_path)
        .with_context(|| format!("reading bundle manifest {}", manifest_path.display()))?;
    let manifest: JsonValue =
        serde_json::from_slice(&manifest_bytes).context("parsing bundle manifest JSON")?;
    if !manifest.is_object() {
        anyhow::bail!("bundle manifest root must be a JSON object");
    }

    let signer_registry = match RuntimeBundleSignerRegistry::load_default() {
        Ok(Some(registry)) => Some(registry),
        Ok(None) => None,
        Err(err) => {
            eprintln!("warning: failed to load runtime bundle signer registry: {err}");
            None
        }
    };
    let channel_hint = manifest
        .pointer("/catalog/channel")
        .and_then(|value| value.as_str());
    let verification =
        verify_manifest_signatures_with_registry(&manifest, signer_registry.as_ref(), channel_hint);
    let trust_shortfall = manifest_trust_shortfall(&verification);
    let only_trust_failure = trust_shortfall;
    let overall_ok = if only_trust_failure {
        true
    } else {
        verification.ok
    };

    let summary = json!({
        "manifest": manifest_path.display().to_string(),
        "canonical_sha256": verification.canonical_sha256,
        "signatures": verification.signatures,
        "warnings": verification.warnings,
        "ok": overall_ok,
        "trusted_signatures": verification.trusted_signatures,
        "rejected_signatures": verification.rejected_signatures,
        "trust_enforced": verification.trust_enforced,
    });

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_else(|_| summary.to_string())
            );
        } else {
            println!("{}", summary);
        }
    } else {
        println!("Manifest: {}", manifest_path.display());
        if let Some(ref canonical) = verification.canonical_sha256 {
            println!("Canonical sha256: {}", canonical);
        } else {
            println!("Canonical sha256: <unavailable>");
        }
        if verification.signatures.is_empty() {
            println!("Signatures: none");
        } else {
            println!("Signatures:");
            for report in &verification.signatures {
                let key_id = report.key_id.as_deref().unwrap_or("<unknown>");
                let status = if report.signature_valid {
                    "valid"
                } else {
                    "invalid"
                };
                let hash_status = if report.hash_matches {
                    "hash match"
                } else {
                    "hash mismatch"
                };
                let issuer = report.issuer.as_deref().unwrap_or("unknown issuer");
                let mut line = format!(
                    "  - key {} ({}, {}) – issuer: {}",
                    key_id, status, hash_status, issuer
                );
                if report.trusted {
                    line.push_str(" [trusted]");
                } else if report.rejected {
                    line.push_str(" [untrusted]");
                }
                println!("{}", line);
                if let Some(err) = report.error.as_deref() {
                    println!("    error: {}", err);
                }
            }
        }
        if !verification.warnings.is_empty() {
            println!("Warnings:");
            for warn in &verification.warnings {
                println!("  - {}", warn);
            }
        }
    }

    if args.require_trusted && trust_shortfall {
        anyhow::bail!(
            "bundle manifest verification failed: no trusted signatures matched the signer registry"
        )
    }
    if overall_ok {
        return Ok(());
    }
    if only_trust_failure && !args.require_trusted {
        if !args.json {
            println!(
                "Note: signatures verified but none matched the trusted signer registry. \
pass --require-trusted to fail in this scenario."
            );
        }
        return Ok(());
    }
    anyhow::bail!("bundle manifest verification failed")
}

#[derive(Default, Serialize, Clone)]
struct SignatureSummary {
    total: usize,
    with_manifest: usize,
    verified: usize,
    failed: usize,
    warnings: usize,
    missing_signatures: usize,
    trusted: usize,
    rejected: usize,
    trust_shortfall: usize,
    ok: bool,
    enforced: bool,
}

impl From<&CliSignatureSummary> for SignatureSummary {
    fn from(src: &CliSignatureSummary) -> Self {
        Self {
            total: src.total,
            with_manifest: src.with_manifest,
            verified: src.verified,
            failed: src.failed,
            warnings: src.warnings,
            missing_signatures: src.missing_signatures,
            trusted: src.trusted,
            rejected: src.rejected,
            trust_shortfall: src.trust_shortfall,
            ok: src.ok,
            enforced: src.enforced,
        }
    }
}

fn manifest_trust_shortfall(sig: &ManifestVerification) -> bool {
    sig.trust_enforced
        && sig.trusted_signatures == 0
        && !sig.signatures.is_empty()
        && sig
            .signatures
            .iter()
            .all(|report| report.signature_valid && report.hash_matches)
}

fn installation_trust_shortfall(inst: &CliRuntimeBundleInstallation) -> bool {
    inst.signature
        .as_ref()
        .map(manifest_trust_shortfall)
        .unwrap_or(false)
}

fn installation_trust_shortfall_warnings(inst: &CliRuntimeBundleInstallation) -> Vec<String> {
    let mut reasons = Vec::new();
    let Some(sig) = inst.signature.as_ref() else {
        return reasons;
    };

    reasons.extend(sig.warnings.clone());
    for report in &sig.signatures {
        if let Some(err) = report.error.as_deref() {
            let key = report
                .key_id
                .as_deref()
                .or(report.public_key_b64.as_deref())
                .unwrap_or("<unknown key>");
            reasons.push(format!("{}: {}", key, err));
        }
    }
    if reasons.is_empty() {
        if let Some(channel) = inst.channel.as_deref() {
            reasons.push(format!(
                "No trusted signer registry entry for channel {}",
                channel
            ));
        } else {
            reasons.push("No trusted signer registry entry for this bundle".to_string());
        }
    }
    reasons
}

fn summarize_installations(
    installs: &[CliRuntimeBundleInstallation],
    enforced: bool,
) -> (SignatureSummary, Vec<String>) {
    let mut summary = SignatureSummary {
        enforced,
        ..SignatureSummary::default()
    };
    let mut failing: Vec<String> = Vec::new();
    for inst in installs {
        summary.total += 1;
        match inst.signature.as_ref() {
            Some(sig) => {
                summary.with_manifest += 1;
                summary.trusted += sig.trusted_signatures;
                summary.rejected += sig.rejected_signatures;
                let trust_shortfall_only = manifest_trust_shortfall(sig);
                if sig.ok || trust_shortfall_only {
                    summary.verified += 1;
                    if !sig.warnings.is_empty() || trust_shortfall_only {
                        summary.warnings += 1;
                    }
                    if trust_shortfall_only {
                        summary.trust_shortfall += 1;
                    }
                } else {
                    summary.failed += 1;
                    if sig.signatures.is_empty() {
                        summary.missing_signatures += 1;
                    }
                    failing.push(inst.id.clone());
                }
            }
            None => {
                summary.failed += 1;
                summary.missing_signatures += 1;
                failing.push(inst.id.clone());
            }
        }
    }
    summary.ok = failing.is_empty();
    (summary, failing)
}

fn cmd_runtime_bundles_audit(args: &RuntimeBundlesAuditArgs) -> Result<()> {
    let (installs, summary, failing_ids, context_label, context_kind) = if args.remote {
        let snapshot = fetch_runtime_bundle_snapshot_remote(&args.base)?;
        let installs = snapshot.installations;
        let (computed_summary, failing) = summarize_installations(&installs, false);
        let mut summary = snapshot
            .signature_summary
            .as_ref()
            .map(SignatureSummary::from)
            .unwrap_or_else(|| computed_summary.clone());
        summary.ok = failing.is_empty();
        summary.enforced = summary.enforced || computed_summary.enforced;
        (
            installs,
            summary,
            failing,
            snapshot
                .roots
                .first()
                .cloned()
                .unwrap_or_else(|| args.base.base_url().to_string()),
            "remote",
        )
    } else {
        let root = default_runtime_bundle_root(&args.dest)?;
        let installs = load_local_runtime_bundle_installations(&root)?;
        let (mut summary, failing) = summarize_installations(&installs, false);
        summary.enforced = false;
        (
            installs,
            summary,
            failing,
            root.display().to_string(),
            "local",
        )
    };

    if args.json {
        let payload = json!({
            "context": context_label,
            "context_kind": context_kind,
            "summary": summary.clone(),
            "installations": installs.clone(),
        });
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!("{}", payload);
        }
        if args.require_signed && !failing_ids.is_empty() {
            let ids = failing_ids.join(", ");
            anyhow::bail!("signature verification failed for bundle(s): {}", ids);
        }
        return Ok(());
    } else {
        println!(
            "Bundle {} context: {}",
            if context_kind == "remote" {
                "server"
            } else {
                "root"
            },
            context_label
        );
        println!(
            "Signatures: total {} | with manifest {} | verified {} | trusted {} | rejected {} | failed {} | warnings {} | missing {} | enforced {}",
            summary.total,
            summary.with_manifest,
            summary.verified,
            summary.trusted,
            summary.rejected,
            summary.failed,
            summary.warnings,
            summary.missing_signatures,
            if summary.enforced { "yes" } else { "no" }
        );

        if installs.is_empty() {
            println!("No bundles installed.");
        } else {
            println!();
            for inst in &installs {
                let label = inst.name.as_deref().unwrap_or(inst.id.as_str());
                println!("{} [{}]", label, inst.id);
                match inst.signature.as_ref() {
                    Some(sig) => {
                        let state = if sig.ok {
                            "verified"
                        } else {
                            "needs attention"
                        };
                        let mut detail = vec![format!("signature: {}", state)];
                        if let Some(canonical) = sig.canonical_sha256.as_deref() {
                            detail.push(format!("canonical {}", canonical));
                        }
                        if !sig.warnings.is_empty() {
                            detail.push(format!("warnings: {}", sig.warnings.join(" | ")));
                        }
                        println!("  {}", detail.join(" | "));
                        if !sig.signatures.is_empty() {
                            for entry in &sig.signatures {
                                let key = entry.key_id.as_deref().unwrap_or("<unknown>");
                                let status = match (
                                    entry.signature_valid,
                                    entry.hash_matches,
                                    entry.error.as_ref(),
                                ) {
                                    (true, true, _) => "valid",
                                    (true, false, _) => "hash mismatch",
                                    _ => "invalid",
                                };
                                let mut line = format!("    key {} -> {}", key, status);
                                if let Some(issuer) = entry.issuer.as_deref() {
                                    line.push_str(&format!(" (issuer: {})", issuer));
                                }
                                if entry.trusted {
                                    line.push_str(" [trusted]");
                                } else if entry.rejected {
                                    line.push_str(" [untrusted]");
                                }
                                println!("{}", line);
                                if let Some(err) = entry.error.as_deref() {
                                    println!("      error: {}", err);
                                }
                            }
                        }
                    }
                    None => {
                        println!("  signature: missing (bundle.json lacks signature block)");
                    }
                }
            }
        }
    }
    if args.require_signed && !failing_ids.is_empty() {
        let ids = failing_ids.join(", ");
        anyhow::bail!("signature verification failed for bundle(s): {}", ids);
    }

    Ok(())
}

fn cmd_runtime_bundles_trust_shortfall(args: &RuntimeBundlesTrustShortfallArgs) -> Result<()> {
    if args.remote && args.dir.is_some() {
        eprintln!("note: --dir is ignored when --remote is set");
    }

    let snapshot = if args.remote {
        fetch_runtime_bundle_snapshot_remote(&args.base)?
    } else {
        let mut snapshot = load_runtime_bundle_snapshot_local(args.dir.clone())?;
        let install_root = default_runtime_bundle_root(&args.install_dir)?;
        let installations = load_local_runtime_bundle_installations(&install_root)?;
        let root_str = install_root.display().to_string();
        if !snapshot.roots.iter().any(|entry| entry == &root_str) {
            snapshot.roots.push(root_str);
        }
        snapshot.installations = installations;
        let (computed_summary, _) = summarize_installations(&snapshot.installations, false);
        snapshot.signature_summary = Some(CliSignatureSummary::from(computed_summary));
        snapshot
    };

    let summary = snapshot.signature_summary.clone().or_else(|| {
        let (computed, _) = summarize_installations(&snapshot.installations, false);
        Some(CliSignatureSummary::from(computed))
    });

    let entries: Vec<BundleTrustShortfallEntry> = snapshot
        .installations
        .iter()
        .filter(|inst| installation_trust_shortfall(inst))
        .map(|inst| BundleTrustShortfallEntry {
            id: inst.id.clone(),
            name: inst.name.clone(),
            channel: inst.channel.clone(),
            root: inst.root.clone(),
            metadata_path: inst.metadata_path.clone(),
            installed_at: inst.installed_at.clone(),
            imported_at: inst.imported_at.clone(),
            warnings: installation_trust_shortfall_warnings(inst),
        })
        .collect();

    if args.json {
        let payload = json!({
            "count": entries.len(),
            "bundles": entries,
            "summary": summary,
            "roots": snapshot.roots,
        });
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!("{}", payload);
        }
        return Ok(());
    }

    if entries.is_empty() {
        println!("No bundle trust shortfalls detected.");
    } else {
        println!("Bundles with trust shortfall ({}):", entries.len());
        for entry in &entries {
            let name = entry.name.as_deref().unwrap_or(&entry.id);
            let id_suffix = if entry
                .name
                .as_deref()
                .map(|value| value != entry.id)
                .unwrap_or(false)
            {
                format!(" [{}]", entry.id)
            } else {
                String::new()
            };
            let channel = entry.channel.as_deref().unwrap_or("—");
            let location = entry
                .root
                .as_deref()
                .or(entry.metadata_path.as_deref())
                .unwrap_or("—");
            println!(
                "- {}{} (channel: {}, location: {})",
                name, id_suffix, channel, location
            );
            for warn in &entry.warnings {
                println!("    • {}", warn);
            }
        }
    }

    if let Some(sum) = summary.as_ref() {
        println!(
            "\nSummary: total {} | with manifest {} | trust shortfall {} | trusted {} | rejected {} | missing {} | enforced {}",
            sum.total,
            sum.with_manifest,
            sum.trust_shortfall,
            sum.trusted,
            sum.rejected,
            sum.missing_signatures,
            if sum.enforced { "yes" } else { "no" }
        );
    }
    println!(
        "Install signer entries in configs/runtime/bundle_signers.json (or set ARW_RUNTIME_BUNDLE_SIGNERS) to resolve trust shortfalls."
    );

    Ok(())
}

fn modality_slug(modality: &RuntimeModality) -> &'static str {
    match modality {
        RuntimeModality::Text => "text",
        RuntimeModality::Audio => "audio",
        RuntimeModality::Vision => "vision",
    }
}

fn accelerator_slug(accel: &RuntimeAccelerator) -> &'static str {
    match accel {
        RuntimeAccelerator::Cpu => "cpu",
        RuntimeAccelerator::GpuCuda => "gpu_cuda",
        RuntimeAccelerator::GpuRocm => "gpu_rocm",
        RuntimeAccelerator::GpuMetal => "gpu_metal",
        RuntimeAccelerator::GpuVulkan => "gpu_vulkan",
        RuntimeAccelerator::NpuDirectml => "npu_directml",
        RuntimeAccelerator::NpuCoreml => "npu_coreml",
        RuntimeAccelerator::NpuOther => "npu_other",
        RuntimeAccelerator::Other => "other",
    }
}

fn bundle_consent_summary(bundle: &runtime_bundles::RuntimeBundle) -> Option<String> {
    let modalities: Vec<String> = bundle
        .modalities
        .iter()
        .map(modality_slug)
        .map(|slug| slug.to_string())
        .collect();
    let needs_overlay = bundle
        .modalities
        .iter()
        .any(|mode| matches!(mode, RuntimeModality::Audio | RuntimeModality::Vision));

    let metadata = bundle.metadata.as_ref();
    let consent_meta = metadata.and_then(|value| value.get("consent"));

    if consent_meta.is_none() {
        if needs_overlay {
            let label = if modalities.is_empty() {
                "audio/vision".to_string()
            } else {
                modalities.join(", ")
            };
            return Some(format!(
                "consent: missing metadata for {} modalities (add `metadata.consent` to the bundle catalog)",
                label
            ));
        }
        return Some("consent: not required (text-only runtime)".to_string());
    }

    let consent_obj = match consent_meta.unwrap().as_object() {
        Some(obj) => obj,
        None => {
            return Some("consent: metadata present but malformed (expected object)".to_string());
        }
    };

    let required = consent_obj
        .get("required")
        .and_then(|value| value.as_bool());

    let mut consent_modalities: Vec<String> = match consent_obj.get("modalities") {
        Some(JsonValue::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(|item| item.to_string())
            .collect(),
        Some(JsonValue::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        _ => Vec::new(),
    };
    if consent_modalities.is_empty() && !modalities.is_empty() {
        consent_modalities = modalities.clone();
    }
    let label = if consent_modalities.is_empty() {
        "unspecified".to_string()
    } else {
        consent_modalities.join(", ")
    };

    let note = consent_obj
        .get("note")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string());
    let suffix = note
        .as_deref()
        .map(|value| format!(" – {}", value))
        .unwrap_or_default();

    Some(match required {
        Some(true) => format!("consent: required ({}){}", label, suffix),
        Some(false) => format!("consent: optional ({}){}", label, suffix),
        None => {
            if needs_overlay {
                format!(
                    "consent: annotate requirement for {} modalities{}",
                    label, suffix
                )
            } else {
                format!("consent: not specified ({}){}", label, suffix)
            }
        }
    })
}
fn fetch_runtime_matrix(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<Option<JsonValue>> {
    let (status, body) = request_runtime_matrix(client, base, token)?;
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "runtime matrix request unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN"
        );
    }
    if !status.is_success() {
        anyhow::bail!("runtime matrix request failed: {} {}", status, body);
    }
    Ok(Some(body))
}

fn request_runtime_supervisor(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<(StatusCode, JsonValue)> {
    let url = format!("{}/state/runtime_supervisor", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime supervisor snapshot from {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime supervisor response")?;
    Ok((status, body))
}

fn request_runtime_matrix(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<(StatusCode, JsonValue)> {
    let url = format!("{}/state/runtime_matrix", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime matrix snapshot from {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime matrix response")?;
    Ok((status, body))
}

fn combine_runtime_snapshots(supervisor: &JsonValue, matrix: Option<JsonValue>) -> JsonValue {
    let mut wrapper = serde_json::Map::new();
    wrapper.insert("supervisor".to_string(), supervisor.clone());
    wrapper.insert("matrix".to_string(), matrix.unwrap_or(JsonValue::Null));
    JsonValue::Object(wrapper)
}

#[cfg(test)]
mod combine_snapshot_tests {
    use super::combine_runtime_snapshots;
    use serde_json::json;

    #[test]
    fn includes_matrix_and_supervisor() {
        let supervisor = json!({"runtimes":[], "updated_at":"2025-10-02T05:30:00Z"});
        let matrix = json!({"ttl_seconds": 120, "items": {}});
        let combined = combine_runtime_snapshots(&supervisor, Some(matrix.clone()));
        assert_eq!(combined["supervisor"], supervisor);
        assert_eq!(combined["matrix"], matrix);
        assert_eq!(combined["matrix"]["ttl_seconds"].as_u64(), Some(120));
    }

    #[test]
    fn defaults_matrix_to_null() {
        let supervisor = json!({"runtimes": [json!({"descriptor": {"id": "rt"}})]});
        let combined = combine_runtime_snapshots(&supervisor, None);
        assert!(combined["matrix"].is_null());
        assert_eq!(combined["supervisor"], supervisor);
    }
}

fn render_runtime_matrix_summary(matrix: &JsonValue) -> String {
    let mut lines: Vec<String> = Vec::new();
    let ttl = matrix
        .get("ttl_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);
    let items = matrix
        .get("items")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let node_count = items.len();
    lines.push(format!(
        "Runtime matrix snapshot: ttl {}s ({} node{}).",
        ttl,
        node_count,
        if node_count == 1 { "" } else { "s" }
    ));
    if items.is_empty() {
        lines.push("No runtime matrix entries available.".to_string());
        return lines.join("\n");
    }

    let mut sorted: Vec<(String, JsonValue)> = items.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (node, entry) in sorted {
        let status = entry
            .get("status")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let label = status
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let severity_slug = status
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info");
        let severity = RuntimeSeverity::from_slug(severity_slug);
        let severity_label = status
            .get("severity_label")
            .and_then(|v| v.as_str())
            .unwrap_or(severity.display_label());
        let aria_hint = status
            .get("aria_hint")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let runtime = entry
            .get("runtime")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let total = runtime.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        let states_summary = runtime
            .get("states")
            .and_then(|v| v.as_object())
            .map(|states| {
                let mut pairs: Vec<String> = states
                    .iter()
                    .filter_map(|(k, val)| val.as_u64().map(|count| format!("{}:{}", k, count)))
                    .collect();
                pairs.sort();
                pairs.join(", ")
            })
            .unwrap_or_default();
        let states_fragment = if states_summary.is_empty() {
            String::new()
        } else {
            format!(" [{}]", states_summary)
        };
        let detail = status
            .get("detail")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or(aria_hint);
        let detail_fragment = if detail.is_empty() {
            String::new()
        } else {
            format!(" — {}", detail)
        };
        lines.push(format!(
            "- {}: {} (severity {} / {}) — runtimes total {}{}{}",
            node, label, severity_label, severity_slug, total, states_fragment, detail_fragment
        ));
    }

    lines.join("\n")
}

fn render_runtime_summary(snapshot: &JsonValue) -> String {
    let mut lines: Vec<String> = Vec::new();
    let updated = snapshot
        .get("updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let runtimes = snapshot
        .get("runtimes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if runtimes.is_empty() {
        lines.push(format!(
            "Runtime supervisor snapshot (updated {}): no managed runtimes registered.",
            updated
        ));
        return lines.join("\n");
    }

    let mut ready = 0usize;
    let total = runtimes.len();
    let mut min_remaining: Option<u64> = None;
    let mut next_reset: Option<DateTime<Utc>> = None;
    let mut next_reset_raw: Option<String> = None;
    let mut exhausted = false;

    for runtime in &runtimes {
        let descriptor = runtime
            .get("descriptor")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let status = runtime
            .get("status")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let id = descriptor
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("runtime");
        let name = descriptor
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        let adapter = descriptor
            .get("adapter")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let state_slug = status
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let state = RuntimeState::from_slug(state_slug);
        let state_label = status
            .get("state_label")
            .and_then(|v| v.as_str())
            .unwrap_or(state.display_label());
        let severity_slug = status
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info");
        let severity = RuntimeSeverity::from_slug(severity_slug);
        let severity_label = status
            .get("severity_label")
            .and_then(|v| v.as_str())
            .unwrap_or(severity.display_label());
        let summary = status
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("(no summary)");
        if state == RuntimeState::Ready {
            ready += 1;
        }
        if state == RuntimeState::Error
            || severity == RuntimeSeverity::Error
            || state == RuntimeState::Offline
        {
            exhausted = true;
        }
        let mut detail_lines: Vec<String> = Vec::new();
        if let Some(detail) = status.get("detail").and_then(|v| v.as_array()) {
            let mut parts: Vec<String> = Vec::new();
            for entry in detail {
                if let Some(s) = entry.as_str() {
                    if !s.trim().is_empty() {
                        parts.push(s.trim().to_string());
                    }
                }
            }
            if !parts.is_empty() {
                detail_lines.push(parts.join(" | "));
            }
        }

        let mut budget_line = None;
        if let Some(budget_obj) = status.get("restart_budget").and_then(|v| v.as_object()) {
            let remaining = budget_obj.get("remaining").and_then(|v| v.as_u64());
            if let Some(rem) = remaining {
                if rem == 0 {
                    exhausted = true;
                }
                if min_remaining.map(|cur| rem < cur).unwrap_or(true) {
                    min_remaining = Some(rem);
                    next_reset = budget_obj
                        .get("reset_at")
                        .and_then(|v| v.as_str())
                        .and_then(parse_reset_utc);
                    next_reset_raw = budget_obj
                        .get("reset_at")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
            if let Some(line) = budget_summary(&JsonValue::Object(budget_obj.clone())) {
                budget_line = Some(line);
            }
        }

        let line = format!(
            "- {} ({}) [{}] — {} ({} / {}) · severity {} ({})",
            name, adapter, id, summary, state_label, state_slug, severity_label, severity_slug
        );
        lines.push(line);
        if let Some(bl) = budget_line {
            lines.push(format!("    {}", bl));
        }
        for extra in detail_lines {
            lines.push(format!("    {}", extra));
        }
    }

    let mut header = format!(
        "Runtime supervisor snapshot (updated {}): {}/{} ready",
        updated, ready, total
    );
    if let Some(rem) = min_remaining {
        let plural = if rem == 1 { "" } else { "s" };
        header.push_str(&format!(", minimum {} restart{} left", rem, plural));
    }
    if let Some(reset) = next_reset {
        header.push_str(&format!(", next reset {}", format_reset_time_local(&reset)));
    } else if let Some(raw) = next_reset_raw {
        header.push_str(&format!(", next reset {}", raw));
    }
    if exhausted {
        header.push_str(" — attention required");
    }
    lines.insert(0, header);
    lines.join("\n")
}

fn budget_summary(budget: &JsonValue) -> Option<String> {
    let obj = budget.as_object()?;
    let remaining = obj.get("remaining").and_then(|v| v.as_u64());
    let max = obj.get("max_restarts").and_then(|v| v.as_u64());
    let used = obj.get("used").and_then(|v| v.as_u64());
    let window = obj.get("window_seconds").and_then(|v| v.as_u64());
    let reset_raw = obj.get("reset_at").and_then(|v| v.as_str());
    let mut parts: Vec<String> = Vec::new();
    if let (Some(rem), Some(mx)) = (remaining, max) {
        parts.push(format!("{} of {} restarts remaining", rem, mx));
    } else if let Some(rem) = remaining {
        parts.push(format!("{} restarts remaining", rem));
    } else if let Some(mx) = max {
        parts.push(format!("max {} restarts", mx));
    }
    if let Some(u) = used {
        parts.push(format!("{} used", u));
    }
    if let Some(win) = window {
        parts.push(format!("window {}s", win));
    }
    if let Some(reset) = reset_raw {
        if let Some(dt) = parse_reset_utc(reset) {
            parts.push(format!("resets {}", format_reset_time_local(&dt)));
        } else {
            parts.push(format!("resets {}", reset));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("Restart budget: {}", parts.join(" · ")))
    }
}

fn parse_reset_utc(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn format_reset_time_local(dt: &DateTime<Utc>) -> String {
    dt.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M %Z")
        .to_string()
}

pub fn execute(cmd: RuntimeCmd) -> Result<()> {
    match cmd {
        RuntimeCmd::Status(args) => cmd_runtime_status(&args),
        RuntimeCmd::Restore(args) => cmd_runtime_restore(&args),
        RuntimeCmd::Shutdown(args) => cmd_runtime_shutdown(&args),
        RuntimeCmd::Bundles { cmd } => match cmd {
            RuntimeBundlesCmd::List(args) => cmd_runtime_bundles_list(&args),
            RuntimeBundlesCmd::Reload(args) => cmd_runtime_bundles_reload(&args),
            RuntimeBundlesCmd::Install(args) => cmd_runtime_bundles_install(&args),
            RuntimeBundlesCmd::Import(args) => cmd_runtime_bundles_import(&args),
            RuntimeBundlesCmd::Rollback(args) => cmd_runtime_bundles_rollback(&args),
            RuntimeBundlesCmd::Manifest { cmd } => match cmd {
                RuntimeBundlesManifestCmd::Sign(args) => cmd_runtime_bundles_manifest_sign(&args),
                RuntimeBundlesManifestCmd::Verify(args) => {
                    cmd_runtime_bundles_manifest_verify(&args)
                }
            },
            RuntimeBundlesCmd::Audit(args) => cmd_runtime_bundles_audit(&args),
            RuntimeBundlesCmd::TrustShortfall(args) => cmd_runtime_bundles_trust_shortfall(&args),
        },
    }
}
