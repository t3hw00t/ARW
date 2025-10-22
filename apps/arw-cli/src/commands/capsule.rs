use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use arw_core::{capsule_presets, capsule_trust};
use arw_protocol::GatingCapsule;
use base64::Engine;
use chrono::Utc;
use clap::{Args, Subcommand};
use reqwest::{blocking::Client, header::ACCEPT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

use super::util::{
    format_local_timestamp, format_observation_timestamp, format_relative_from_now,
    resolve_admin_token, truncate_payload, validate_trust_key, with_admin_headers,
};

#[derive(Subcommand)]
pub enum CapCmd {
    /// Print a minimal capsule template (JSON)
    Template(TemplateArgs),
    /// Generate an ed25519 keypair (b64) and print
    GenEd25519(GenKeyArgs),
    /// Sign a capsule file with ed25519 secret key (b64) and print signature
    SignEd25519(SignArgs),
    /// Verify a capsule file signature with ed25519 public key (b64)
    VerifyEd25519(VerifyArgs),
    /// Adopt a signed capsule manifest by sending it to the server
    Adopt(CapsuleAdoptArgs),
    /// Fetch active policy capsules from the server
    Status(CapsuleStatusArgs),
    /// Emergency teardown for active policy capsules
    Teardown(CapsuleTeardownArgs),
    /// Capsule preset helpers
    Preset {
        #[command(subcommand)]
        cmd: CapsulePresetCmd,
    },
    /// Tail capsule audit events
    Audit(CapsuleAuditArgs),
    /// Trust store helpers
    Trust {
        #[command(subcommand)]
        cmd: CapsuleTrustCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum CapsulePresetCmd {
    /// List capsule presets known to the server or local configs
    List(CapsulePresetListArgs),
    /// Adopt a preset by id via the admin API
    Adopt(CapsulePresetAdoptArgs),
}

#[derive(Args)]
pub(crate) struct CapsulePresetListArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of a table
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
    /// Read presets from configs/capsules instead of calling the server
    #[arg(long)]
    local: bool,
}

#[derive(Deserialize)]
pub(crate) struct CapsulePresetListHttp {
    presets: Vec<capsule_presets::CapsulePresetSummary>,
}

#[derive(Args)]
pub(crate) struct CapsulePresetAdoptArgs {
    /// Capsule preset identifier (e.g., capsule.strict-egress)
    id: String,
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Optional audit reason to persist with the adoption
    #[arg(long)]
    reason: Option<String>,
    /// Show capsule status after adoption
    #[arg(long)]
    show_status: bool,
}

#[derive(Deserialize)]
pub(crate) struct CapsuleAdoptHttpResponse {
    ok: bool,
    notify: bool,
    #[serde(default)]
    preset_id: Option<String>,
    capsule_id: String,
}

#[derive(Args)]
pub(crate) struct CapsuleAuditArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Maximum entries to return (default 50, max 500)
    #[arg(long, default_value_t = 50)]
    limit: usize,
    /// Additional CSV of prefixes to include alongside policy.capsule.*
    #[arg(long)]
    prefix: Option<String>,
    /// Emit raw JSON instead of a table
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
}

#[derive(Deserialize)]
pub(crate) struct CapsuleAuditHttpResponse {
    limit: usize,
    total_matched: usize,
    truncated: bool,
    entries: Vec<CapsuleAuditEntry>,
}

#[derive(Deserialize)]
pub(crate) struct CapsuleAuditEntry {
    time: String,
    kind: String,
    payload: JsonValue,
}

#[derive(Subcommand)]
pub(crate) enum CapsuleTrustCmd {
    /// List trusted capsule issuers
    List(CapsuleTrustListArgs),
    /// Add or replace a trusted capsule issuer
    Add(CapsuleTrustAddArgs),
    /// Remove a trusted capsule issuer
    Remove(CapsuleTrustRemoveArgs),
    /// Rotate a trusted issuer's keypair (ed25519 only)
    Rotate(CapsuleTrustRotateArgs),
}

#[derive(Args)]
pub(crate) struct CapsuleTrustListArgs {
    /// Emit raw JSON instead of a table
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
pub(crate) struct CapsuleTrustAddArgs {
    /// Issuer identifier (e.g., local-admin)
    #[arg(long)]
    id: String,
    /// Signing algorithm (ed25519 or secp256k1)
    #[arg(long, default_value = "ed25519")]
    alg: String,
    /// Public key (base64)
    #[arg(long, conflicts_with = "key_file")]
    key: Option<String>,
    /// Read public key (base64) from file
    #[arg(long = "key-file", value_hint = clap::ValueHint::FilePath, conflicts_with = "key")]
    key_file: Option<PathBuf>,
    /// Replace existing issuer entry if present
    #[arg(long)]
    replace: bool,
}

#[derive(Args)]
pub(crate) struct CapsuleTrustRemoveArgs {
    /// Issuer identifier to remove
    #[arg(long)]
    id: String,
    /// Succeed silently if the issuer is missing
    #[arg(long)]
    allow_missing: bool,
}

#[derive(Args)]
pub(crate) struct CapsuleTrustRotateArgs {
    /// Issuer identifier to rotate (ed25519)
    #[arg(long)]
    id: String,
    /// Signing algorithm (currently only ed25519 is supported)
    #[arg(long, default_value = "ed25519")]
    alg: String,
    /// Write new public key (base64) to file
    #[arg(long = "out-pub", value_hint = clap::ValueHint::FilePath)]
    out_pub: Option<PathBuf>,
    /// Write new private key (base64) to file
    #[arg(long = "out-priv", value_hint = clap::ValueHint::FilePath)]
    out_priv: Option<PathBuf>,
    /// Base URL of the service when --reload is supplied
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token for --reload; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds for --reload
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Call POST /admin/rpu/reload after rotating
    #[arg(long)]
    reload: bool,
}

#[derive(Args)]
pub(crate) struct TemplateArgs {
    /// Pretty-print JSON (default on unless --compact)
    #[arg(long)]
    pretty: bool,
    /// Print compact JSON (overrides --pretty)
    #[arg(long)]
    compact: bool,
}

#[derive(Args)]
pub(crate) struct GenKeyArgs {
    /// Write public key to this file (optional)
    #[arg(long)]
    out_pub: Option<String>,
    /// Write private key to this file (optional; keep secure)
    #[arg(long)]
    out_priv: Option<String>,
    /// Issuer string to include in JSON summary (default: local-admin)
    #[arg(long)]
    issuer: Option<String>,
}

#[derive(Args)]
pub(crate) struct SignArgs {
    /// Secret key (b64)
    sk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Write signature to this file (optional)
    #[arg(long)]
    out: Option<String>,
}

#[derive(Args)]
pub(crate) struct VerifyArgs {
    /// Public key (b64)
    pk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Signature (b64)
    sig_b64: String,
}

#[derive(Args)]
pub(crate) struct CapsuleAdoptArgs {
    /// Capsule manifest to adopt (must include signature)
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    file: PathBuf,
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Show capsule status after adoption
    #[arg(long)]
    show_status: bool,
    /// Skip local signature verification before sending
    #[arg(long)]
    skip_verify: bool,
}

#[derive(Args)]
pub(crate) struct CapsuleStatusArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Number of capsules to display (text mode only)
    #[arg(long, default_value_t = 5)]
    limit: usize,
}

#[derive(Args, Clone)]
pub(crate) struct CapsuleTeardownArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Capsule ID to remove (repeat for multiple). Use --all to remove every capsule.
    #[arg(long, value_name = "ID")]
    id: Vec<String>,
    /// Remove every capsule regardless of ID
    #[arg(long)]
    all: bool,
    /// Optional operator reason recorded with events
    #[arg(long)]
    reason: Option<String>,
    /// Preview without removing capsules
    #[arg(long)]
    dry_run: bool,
    /// Timeout seconds
    #[arg(long, default_value_t = 10)]
    timeout: u64,
    /// Emit raw JSON instead of text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Serialize)]
struct CapsuleTeardownPayload {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    ids: Vec<String>,
    all: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    dry_run: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct CapsuleTeardownResponseDto {
    ok: bool,
    #[serde(default)]
    removed: Vec<JsonValue>,
    #[serde(default)]
    not_found: Vec<String>,
    remaining: usize,
    dry_run: bool,
    #[serde(default)]
    reason: Option<String>,
}

pub fn execute(cmd: CapCmd) -> Result<()> {
    match cmd {
        CapCmd::Template(args) => {
            let duration = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            let now = (duration.as_millis()).min(u128::from(u64::MAX)) as u64;
            let tpl = json!({
              "id":"example",
              "version":"1",
              "issued_at_ms": now,
              "issuer": "local-admin",
              "hop_ttl": 1,
              "propagate": "children",
              "denies": [],
              "contracts": [
                {"id":"block-tools","patterns":["tools:*"],"valid_from_ms":0}
              ]
            });
            let serialized = if args.compact {
                serde_json::to_string(&tpl)
                    .map_err(|e| anyhow!("failed to render capsule template JSON (compact): {e}"))
            } else {
                serde_json::to_string_pretty(&tpl)
                    .map_err(|e| anyhow!("failed to render capsule template JSON (pretty): {e}"))
            }?;
            println!("{}", serialized);
            Ok(())
        }
        CapCmd::GenEd25519(args) => cmd_gen_ed25519(
            args.out_pub.as_deref(),
            args.out_priv.as_deref(),
            args.issuer.as_deref(),
        ),
        CapCmd::SignEd25519(args) => {
            cmd_sign_ed25519(&args.sk_b64, &args.capsule_json, args.out.as_deref())
        }
        CapCmd::VerifyEd25519(args) => {
            cmd_verify_ed25519(&args.pk_b64, &args.capsule_json, &args.sig_b64)?;
            println!("ok");
            Ok(())
        }
        CapCmd::Adopt(args) => cmd_capsule_adopt(&args),
        CapCmd::Status(args) => cmd_capsule_status(&args),
        CapCmd::Teardown(args) => cmd_capsule_teardown(&args),
        CapCmd::Preset { cmd } => match cmd {
            CapsulePresetCmd::List(args) => cmd_capsule_preset_list(&args),
            CapsulePresetCmd::Adopt(args) => cmd_capsule_preset_adopt(&args),
        },
        CapCmd::Audit(args) => cmd_capsule_audit(&args),
        CapCmd::Trust { cmd } => match cmd {
            CapsuleTrustCmd::List(args) => cmd_capsule_trust_list(&args),
            CapsuleTrustCmd::Add(args) => cmd_capsule_trust_add(&args),
            CapsuleTrustCmd::Remove(args) => cmd_capsule_trust_remove(&args),
            CapsuleTrustCmd::Rotate(args) => cmd_capsule_trust_rotate(&args),
        },
    }
}

fn cmd_gen_ed25519(
    out_pub: Option<&str>,
    out_priv: Option<&str>,
    issuer: Option<&str>,
) -> Result<()> {
    let (pk_b64, sk_b64) = generate_ed25519_pair_b64()?;
    if let Some(p) = out_pub {
        std::fs::write(p, &pk_b64)?;
    }
    if let Some(p) = out_priv {
        std::fs::write(p, &sk_b64)?;
    }
    let iss = issuer.unwrap_or("local-admin");
    println!(
        "{}",
        serde_json::json!({"issuer": iss, "alg":"ed25519","pubkey_b64": pk_b64, "privkey_b64": sk_b64 })
    );
    eprintln!("Note: store private key securely; add pubkey to configs/trust_capsules.json");
    Ok(())
}

fn cmd_capsule_adopt(args: &CapsuleAdoptArgs) -> Result<()> {
    let contents = std::fs::read_to_string(&args.file)
        .with_context(|| format!("reading capsule file {}", args.file.display()))?;
    let capsule: GatingCapsule = serde_json::from_str(&contents)
        .with_context(|| format!("parsing capsule file {}", args.file.display()))?;
    let signature_missing = capsule
        .signature
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    if signature_missing {
        bail!(
            "capsule '{}' is missing a signature; sign it with `arw-cli capsule sign-ed25519` first",
            args.file.display()
        );
    }

    if !args.skip_verify && !arw_core::rpu::verify_capsule(&capsule) {
        bail!(
            "capsule '{}' failed local signature verification. Update configs/trust_capsules.json or pass --skip-verify to override.",
            args.file.display()
        );
    }

    let serialized =
        serde_json::to_string(&capsule).context("serializing capsule payload for request")?;

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/policy/capsules", base);

    let mut req = client.get(&url);
    req = req.header("X-ARW-Capsule", &serialized);
    req = req.header(ACCEPT, "application/json");
    req = with_admin_headers(req, token.as_deref());

    let resp = req.send().with_context(|| format!("requesting {}", url))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("capsule adoption failed with {}: {}", status, text.trim());
    }

    println!("Capsule '{}' adopted via {}.", args.file.display(), url);

    if args.show_status {
        println!();
        let status_args = CapsuleStatusArgs {
            base: args.base.clone(),
            admin_token: args.admin_token.clone(),
            timeout: args.timeout,
            json: false,
            pretty: false,
            limit: 5,
        };
        cmd_capsule_status(&status_args)?;
    }

    Ok(())
}

fn cmd_capsule_status(args: &CapsuleStatusArgs) -> Result<()> {
    const CAPSULE_EXPIRING_SOON_WINDOW_MS: u64 = 60_000;

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/policy/capsules", base);
    let resp = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting {}", url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        anyhow::bail!("server returned {}: {}", status, text.trim());
    }
    let snapshot: JsonValue = resp.json().context("parsing capsule snapshot")?;

    if args.json {
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
        } else {
            println!("{}", serde_json::to_string(&snapshot)?);
        }
        return Ok(());
    }

    let now_ms = Utc::now().timestamp_millis() as u64;
    let count = snapshot
        .get("count")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0);
    let generated_ms = snapshot.get("generated_ms").and_then(JsonValue::as_u64);
    let generated_label = generated_ms
        .map(|ms| {
            format!(
                "{} ({})",
                format_local_timestamp(ms),
                format_relative_from_now(ms, now_ms)
            )
        })
        .or_else(|| {
            snapshot
                .get("generated")
                .and_then(JsonValue::as_str)
                .map(|s| s.to_string())
        });

    println!("Active policy capsules: {}", count);
    if let Some(label) = generated_label {
        println!("Generated: {}", label);
    }

    let items: Vec<JsonValue> = snapshot
        .get("items")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        println!("No capsule entries.");
        return Ok(());
    }

    let mut status_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut expiring_soon: u64 = 0;
    let mut expired: u64 = 0;
    let mut next_expiry: Option<(u64, String, String)> = None;

    for item in &items {
        if let Some(status) = item.get("status").and_then(JsonValue::as_str) {
            *status_counts.entry(status.to_string()).or_insert(0) += 1;
        }

        if let Some(lease_until) = item.get("lease_until_ms").and_then(JsonValue::as_u64) {
            if lease_until <= now_ms {
                expired += 1;
            } else if lease_until.saturating_sub(now_ms) <= CAPSULE_EXPIRING_SOON_WINDOW_MS {
                expiring_soon += 1;
            }
            let should_update = next_expiry
                .as_ref()
                .map(|(current, _, _)| lease_until < *current)
                .unwrap_or(true);
            if should_update && lease_until > now_ms {
                let label = item
                    .get("status_label")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("Expiry")
                    .to_string();
                let id = item
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("capsule")
                    .to_string();
                next_expiry = Some((lease_until, label, id));
            }
        }
    }

    let renew_due = status_counts.get("renew_due").copied().unwrap_or(0);
    let expiring_status = status_counts
        .get("expiring")
        .copied()
        .unwrap_or(expiring_soon);

    let summary_line = if count == 0 {
        "No active policy capsules.".to_string()
    } else {
        match (renew_due, expiring_status, expired) {
            (0, 0, 0) => "All policy capsules are healthy.".to_string(),
            _ => format!(
                "{} capsule{} healthy; {} awaiting renewal; {} expiring; {} expired.",
                count.saturating_sub(renew_due + expiring_status + expired),
                if count == 1 { "" } else { "s" },
                renew_due,
                expiring_status,
                expired
            ),
        }
    };

    println!("Summary: {}", summary_line);
    if let Some((expiry, label, id)) = &next_expiry {
        let cleaned_label = label.trim_end_matches('.');
        println!(
            "Next expiry: {} ({} · {})",
            format_relative_from_now(*expiry, now_ms),
            format_local_timestamp(*expiry),
            cleaned_label
        );
        println!("             capsule: {}", id);
    }

    if !status_counts.is_empty() {
        println!("Status counts:");
        for (status, total) in &status_counts {
            println!("  - {}: {}", format_status_label(status), total);
        }
    }

    let limit = args.limit.max(1);
    println!("Capsule sample (showing up to {}):", limit);
    for item in items.iter().take(limit) {
        let id = item
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or("capsule");
        let label = item
            .get("status_label")
            .and_then(JsonValue::as_str)
            .or_else(|| item.get("status").and_then(JsonValue::as_str))
            .unwrap_or("unknown");
        let aria = item
            .get("aria_hint")
            .and_then(JsonValue::as_str)
            .unwrap_or("");
        if aria.is_empty() {
            println!("  - {} :: {}", id, label);
        } else {
            println!("  - {} :: {} :: {}", id, label, aria);
        }
    }

    Ok(())
}

fn cmd_capsule_teardown(args: &CapsuleTeardownArgs) -> Result<()> {
    if !args.all && args.id.is_empty() {
        bail!("provide --all or at least one --id");
    }

    let mut seen = HashSet::new();
    let mut ids: Vec<String> = Vec::new();
    for raw in &args.id {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            ids.push(trimmed.to_string());
        }
    }
    if !args.all && ids.is_empty() {
        bail!("all provided --id values were blank; specify --all or valid ids");
    }

    let reason = args
        .reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let payload = CapsuleTeardownPayload {
        ids,
        all: args.all,
        reason,
        dry_run: args.dry_run,
    };

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/policy/capsules/teardown", base);
    let resp = with_admin_headers(client.post(&url).json(&payload), token.as_deref())
        .send()
        .with_context(|| format!("requesting {}", url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("server returned {}: {}", status, text.trim());
    }

    let response: CapsuleTeardownResponseDto =
        resp.json().context("parsing capsule teardown response")?;

    if args.json {
        let value = serde_json::to_value(&response)?;
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("{}", serde_json::to_string(&value)?);
        }
        return Ok(());
    }

    if response.dry_run {
        println!("Dry-run preview — no capsules removed.");
    } else {
        println!("Emergency teardown completed.");
    }
    if let Some(reason) = response.reason.as_ref() {
        println!("Reason: {}", reason);
    }
    println!("Remaining active capsules: {}", response.remaining);

    if !response.removed.is_empty() {
        let noun = if response.dry_run {
            "Would remove"
        } else {
            "Removed"
        };
        println!("{} capsules ({}):", noun, response.removed.len());
        for entry in &response.removed {
            println!("  - {}", summarize_teardown_capsule(entry));
        }
    } else if !response.dry_run {
        println!("Removed capsules: none");
    }

    if !response.not_found.is_empty() {
        println!(
            "Not found ({}): {}",
            response.not_found.len(),
            response.not_found.join(", ")
        );
    }

    Ok(())
}

fn cmd_capsule_preset_list(args: &CapsulePresetListArgs) -> Result<()> {
    let summaries = if args.local {
        let presets = capsule_presets::list_capsule_presets()
            .context("loading capsule presets from configs/capsules")?;
        presets.into_iter().map(|p| p.summary).collect::<Vec<_>>()
    } else {
        let client = Client::builder()
            .timeout(Duration::from_secs(args.timeout))
            .build()
            .context("building HTTP client")?;
        let base = args.base.trim_end_matches('/');
        let url = format!("{}/admin/policy/capsules/presets", base);
        let token = resolve_admin_token(&args.admin_token);
        let resp = with_admin_headers(
            client.get(&url).header(ACCEPT, "application/json"),
            token.as_deref(),
        )
        .send()
        .with_context(|| format!("requesting {}", url))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            bail!("server returned {}: {}", status, text.trim());
        }
        let body: CapsulePresetListHttp =
            resp.json().context("parsing capsule presets response")?;
        body.presets
    };

    if args.json {
        let value = json!({
            "count": summaries.len(),
            "presets": summaries,
        });
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("{}", serde_json::to_string(&value)?);
        }
        return Ok(());
    }

    if summaries.is_empty() {
        if args.local {
            println!("No capsule presets found under configs/capsules.");
        } else {
            println!("Server reports no capsule presets.");
        }
        return Ok(());
    }

    println!(
        "{:<28} {:<10} {:<14} {:>6} {:>7} Source",
        "ID", "Version", "Issuer", "Denies", "Contracts"
    );
    println!("{}", "-".repeat(80));
    for preset in &summaries {
        let issuer = preset.issuer.as_deref().unwrap_or("-");
        println!(
            "{:<28} {:<10} {:<14} {:>6} {:>7} {}",
            preset.id,
            preset.version.as_deref().unwrap_or("1"),
            issuer,
            preset.denies,
            preset.contracts,
            if args.local {
                preset.path.clone()
            } else {
                preset.file_name.clone()
            }
        );
    }
    println!();
    if args.local {
        let dir = summaries
            .first()
            .and_then(|summary| {
                Path::new(&summary.path)
                    .parent()
                    .map(|p| p.display().to_string())
            })
            .unwrap_or_else(|| "configs/capsules".to_string());
        println!("Source directory: {}", dir);
    }

    Ok(())
}

fn cmd_capsule_preset_adopt(args: &CapsulePresetAdoptArgs) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/policy/capsules/adopt", base);
    let token = resolve_admin_token(&args.admin_token);
    let mut body = json!({ "preset_id": args.id.trim() });
    if let Some(reason) = args
        .reason
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        body.as_object_mut()
            .unwrap()
            .insert("reason".to_string(), JsonValue::String(reason.to_string()));
    }
    let resp = with_admin_headers(client.post(&url).json(&body), token.as_deref())
        .send()
        .with_context(|| format!("requesting {}", url))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("server returned {}: {}", status, text.trim());
    }
    let adopted: CapsuleAdoptHttpResponse =
        resp.json().context("parsing capsule adopt response")?;
    if !adopted.ok {
        bail!("server reported failure adopting preset {}", args.id);
    }
    println!(
        "Adopted preset '{}' (capsule {}) — notify={}.",
        args.id, adopted.capsule_id, adopted.notify
    );
    if let Some(preset_id) = adopted.preset_id {
        println!("Server preset id: {}", preset_id);
    }
    if args.show_status {
        println!();
        let status_args = CapsuleStatusArgs {
            base: args.base.clone(),
            admin_token: args.admin_token.clone(),
            timeout: args.timeout,
            json: false,
            pretty: false,
            limit: 5,
        };
        cmd_capsule_status(&status_args)?;
    }
    Ok(())
}

fn cmd_capsule_audit(args: &CapsuleAuditArgs) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let mut url = reqwest::Url::parse(&format!("{}/admin/policy/capsules/audit", base))?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("limit", &args.limit.min(500).to_string());
        if let Some(prefix) = args
            .prefix
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            qp.append_pair("prefix", prefix);
        }
    }
    let token = resolve_admin_token(&args.admin_token);
    let resp = with_admin_headers(
        client.get(url).header(ACCEPT, "application/json"),
        token.as_deref(),
    )
    .send()
    .context("requesting capsule audit stream")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        bail!("server returned {}: {}", status, text.trim());
    }
    if args.json {
        let value: JsonValue = resp.json().context("parsing capsule audit response")?;
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("{}", serde_json::to_string(&value)?);
        }
        return Ok(());
    }
    let audit: CapsuleAuditHttpResponse = resp.json().context("parsing capsule audit response")?;

    println!(
        "Entries: {} (limit {}); total matched {}; truncated: {}",
        audit.entries.len(),
        audit.limit,
        audit.total_matched,
        audit.truncated
    );
    for entry in audit.entries {
        let ts = format_observation_timestamp(&entry.time);
        let capsule_id = entry
            .payload
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or("-");
        let issuer = entry
            .payload
            .get("issuer")
            .and_then(JsonValue::as_str)
            .unwrap_or("-");
        let status = entry
            .payload
            .get("status_label")
            .and_then(JsonValue::as_str)
            .or_else(|| entry.payload.get("status").and_then(JsonValue::as_str))
            .unwrap_or("-");
        let note = entry
            .payload
            .get("reason")
            .and_then(JsonValue::as_str)
            .or_else(|| entry.payload.get("aria_hint").and_then(JsonValue::as_str));
        println!(
            "[{}] {:<26} {:<26} issuer {:<14} status {}",
            ts,
            truncate_payload(&entry.kind, 26),
            truncate_payload(capsule_id, 26),
            truncate_payload(issuer, 14),
            truncate_payload(status, 24),
        );
        if let Some(note) = note {
            println!("      note: {}", truncate_payload(note, 90));
        }
        if entry.kind == "policy.capsule.teardown" {
            if let Some(removed) = entry.payload.get("removed").and_then(JsonValue::as_array) {
                let preview = removed
                    .iter()
                    .map(|item| {
                        item.get("id")
                            .and_then(JsonValue::as_str)
                            .unwrap_or("capsule")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if !preview.is_empty() {
                    println!("      removed: {}", truncate_payload(&preview, 90));
                }
            }
        }
    }
    Ok(())
}

fn cmd_capsule_trust_list(args: &CapsuleTrustListArgs) -> Result<()> {
    let path = capsule_trust::trust_store_path();
    let entries = capsule_trust::load_trust_entries()
        .with_context(|| format!("reading {}", path.display()))?;
    if args.json {
        let value = json!({
            "path": path.display().to_string(),
            "entries": entries,
        });
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("{}", serde_json::to_string(&value)?);
        }
        return Ok(());
    }
    println!("Trust store: {}", path.display());
    if entries.is_empty() {
        println!("No trusted capsule issuers.");
        return Ok(());
    }
    println!("{:<24} {:<10} Key (base64)", "ID", "Algorithm");
    println!("{}", "-".repeat(72));
    for entry in entries {
        println!(
            "{:<24} {:<10} {}",
            entry.id,
            entry.alg,
            truncate_payload(&entry.key_b64, 44)
        );
    }
    Ok(())
}

fn cmd_capsule_trust_add(args: &CapsuleTrustAddArgs) -> Result<()> {
    let mut entries =
        capsule_trust::load_trust_entries().context("loading existing trust entries")?;
    let key_b64 = read_key_b64(args)?;
    validate_trust_key(&args.alg, &key_b64)?;
    if let Some(entry) = entries.iter_mut().find(|e| e.id == args.id) {
        if !args.replace {
            bail!(
                "issuer '{}' already exists; pass --replace to overwrite",
                args.id
            );
        }
        entry.alg = args.alg.clone();
        entry.key_b64 = key_b64;
    } else {
        entries.push(capsule_trust::TrustEntry {
            id: args.id.clone(),
            alg: args.alg.clone(),
            key_b64,
        });
        entries.sort_by(|a, b| a.id.cmp(&b.id));
    }
    capsule_trust::save_trust_entries(&entries).context("writing trust store")?;
    arw_core::rpu::reload_trust();
    println!(
        "Issuer '{}' ({}) saved to {}",
        args.id,
        args.alg,
        capsule_trust::trust_store_path().display()
    );
    Ok(())
}

fn cmd_capsule_trust_remove(args: &CapsuleTrustRemoveArgs) -> Result<()> {
    let mut entries =
        capsule_trust::load_trust_entries().context("loading existing trust entries")?;
    let before = entries.len();
    entries.retain(|entry| entry.id != args.id);
    if entries.len() == before {
        if args.allow_missing {
            println!("Issuer '{}' not present; nothing to do.", args.id);
            return Ok(());
        } else {
            bail!("issuer '{}' not found", args.id);
        }
    }
    capsule_trust::save_trust_entries(&entries).context("writing trust store")?;
    arw_core::rpu::reload_trust();
    println!(
        "Removed issuer '{}' from {}",
        args.id,
        capsule_trust::trust_store_path().display()
    );
    Ok(())
}

fn cmd_capsule_trust_rotate(args: &CapsuleTrustRotateArgs) -> Result<()> {
    if args.alg != "ed25519" {
        bail!("only ed25519 rotation is currently supported");
    }
    let mut entries =
        capsule_trust::load_trust_entries().context("loading existing trust entries")?;
    let entry = entries
        .iter_mut()
        .find(|entry| entry.id == args.id)
        .ok_or_else(|| anyhow!("issuer '{}' not found", args.id))?;

    let (pub_b64, priv_b64) = generate_ed25519_pair_b64()?;
    entry.alg = args.alg.clone();
    entry.key_b64 = pub_b64.clone();
    capsule_trust::save_trust_entries(&entries).context("writing trust store")?;
    arw_core::rpu::reload_trust();

    if let Some(path) = args.out_pub.as_ref() {
        std::fs::write(path, &pub_b64)
            .with_context(|| format!("writing public key to {}", path.display()))?;
    }
    if let Some(path) = args.out_priv.as_ref() {
        std::fs::write(path, &priv_b64)
            .with_context(|| format!("writing private key to {}", path.display()))?;
    }

    let output = json!({
        "issuer": args.id,
        "alg": args.alg,
        "pubkey_b64": pub_b64,
        "privkey_b64": priv_b64,
        "trust_store": capsule_trust::trust_store_path().display().to_string(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    eprintln!("Note: store the private key securely and distribute the public key to peers.");

    if args.reload {
        let client = Client::builder()
            .timeout(Duration::from_secs(args.timeout))
            .build()
            .context("building HTTP client for reload")?;
        let base = args.base.trim_end_matches('/');
        let url = format!("{}/admin/rpu/reload", base);
        let token = resolve_admin_token(&args.admin_token);
        let resp = with_admin_headers(client.post(&url), token.as_deref())
            .send()
            .with_context(|| format!("requesting {}", url))?;
        let status = resp.status();
        if status.is_success() {
            println!("Trust store reload requested via {}", url);
        } else {
            let text = resp.text().unwrap_or_default();
            eprintln!(
                "Warning: reload request returned {}: {}",
                status,
                text.trim()
            );
        }
    }

    Ok(())
}

fn summarize_teardown_capsule(value: &JsonValue) -> String {
    let id = value
        .get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or("capsule");
    let version = value
        .get("version")
        .and_then(JsonValue::as_str)
        .unwrap_or("?");
    let status = value
        .get("status_label")
        .and_then(JsonValue::as_str)
        .or_else(|| value.get("status").and_then(JsonValue::as_str))
        .unwrap_or("status unknown");
    let lease = value
        .get("lease_until")
        .and_then(JsonValue::as_str)
        .map(|s| format!("lease until {}", s))
        .or_else(|| {
            value
                .get("lease_until_ms")
                .and_then(JsonValue::as_u64)
                .map(|ms| format!("lease ms {}", ms))
        })
        .unwrap_or_else(|| "no lease".to_string());
    format!("{} v{} :: {} ({})", id, version, status, lease)
}

fn format_status_label(raw: &str) -> String {
    match raw {
        "active" => "Active".to_string(),
        "renew_due" => "Renew window".to_string(),
        "expiring" => "Expiring soon".to_string(),
        "expired" => "Expired".to_string(),
        "unbounded" => "No lease".to_string(),
        other => other.replace('_', " "),
    }
}

pub(crate) fn generate_ed25519_pair_b64() -> Result<(String, String)> {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use rand_core::TryRngCore;
    let mut rng = OsRng;
    let mut sk_bytes = [0u8; 32];
    rng.try_fill_bytes(&mut sk_bytes)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let sk = SigningKey::from_bytes(&sk_bytes);
    let pk = sk.verifying_key();
    let sk_b64 = base64::engine::general_purpose::STANDARD.encode(sk.to_bytes());
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk.to_bytes());
    Ok((pk_b64, sk_b64))
}

fn cmd_sign_ed25519(sk_b64: &str, capsule_file: &str, out: Option<&str>) -> Result<()> {
    use ed25519_dalek::{Signer, SigningKey};
    let sk_bytes = base64::engine::general_purpose::STANDARD.decode(sk_b64)?;
    let sk = SigningKey::from_bytes(&sk_bytes.as_slice().try_into()?);
    let mut cap: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(capsule_file)?)?;
    if let Some(obj) = cap.as_object_mut() {
        obj.remove("signature");
    }
    let msg = serde_json::to_vec(&cap)?;
    let sig = sk.sign(&msg);
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
    if let Some(p) = out {
        std::fs::write(p, &sig_b64)?;
    }
    println!("{}", sig_b64);
    Ok(())
}

fn cmd_verify_ed25519(pk_b64: &str, capsule_file: &str, sig_b64: &str) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk_bytes = base64::engine::general_purpose::STANDARD.decode(pk_b64)?;
    let vk = VerifyingKey::from_bytes(&pk_bytes.as_slice().try_into()?)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let mut cap: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(capsule_file)?)?;
    if let Some(obj) = cap.as_object_mut() {
        obj.remove("signature");
    }
    let msg = serde_json::to_vec(&cap)?;
    let sig_bytes = base64::engine::general_purpose::STANDARD.decode(sig_b64)?;
    let sig = Signature::from_bytes(&sig_bytes.as_slice().try_into()?);
    vk.verify(&msg, &sig)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

fn read_key_b64(args: &CapsuleTrustAddArgs) -> Result<String> {
    if let Some(key) = args.key.as_ref() {
        Ok(key.trim().to_string())
    } else if let Some(path) = args.key_file.as_ref() {
        Ok(std::fs::read_to_string(path)
            .with_context(|| format!("reading key from {}", path.display()))?
            .trim()
            .to_string())
    } else {
        bail!("provide --key <b64> or --key-file <path>");
    }
}
