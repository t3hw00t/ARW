use anyhow::{Context, Result};
use arw_core::{gating, gating_keys, hello_core, introspect_tools, load_effective_paths};
use base64::Engine;
use chrono::{DateTime, Local, TimeZone, Utc};
use clap::CommandFactory;
use clap::{Args, Parser, Subcommand};
use reqwest::blocking::Client;
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing_subscriber::{fmt, EnvFilter};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "arw-cli", version, about = "ARW CLI utilities")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print effective runtime/cache/logs paths (JSON)
    Paths(PathsArgs),
    /// Print tool list (JSON)
    Tools(ToolsArgs),
    /// Gating helpers
    Gate {
        #[command(subcommand)]
        cmd: GateCmd,
    },
    /// Policy capsules (templates, keys, signatures)
    Capsule {
        #[command(subcommand)]
        cmd: CapCmd,
    },
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Ping the service and print status
    Ping(PingArgs),
    /// Spec helpers
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// Screenshots maintenance commands
    Screenshots {
        #[command(subcommand)]
        cmd: ScreenshotsCmd,
    },
    /// Managed runtime supervisor helpers
    Runtime {
        #[command(subcommand)]
        cmd: RuntimeCmd,
    },
    /// Event journal helpers
    Events {
        #[command(subcommand)]
        cmd: EventsCmd,
    },
}

#[derive(Subcommand)]
enum GateCmd {
    /// List known gating keys
    Keys(GateKeysArgs),
    /// Gating policy helpers
    Config {
        #[command(subcommand)]
        cmd: GateConfigCmd,
    },
}

#[derive(Subcommand)]
enum GateConfigCmd {
    /// Print the gating config JSON schema
    Schema(GateConfigSchemaArgs),
    /// Render the gating config reference (Markdown)
    Doc(GateConfigDocArgs),
}

#[derive(Subcommand)]
enum CapCmd {
    /// Print a minimal capsule template (JSON)
    Template(TemplateArgs),
    /// Generate an ed25519 keypair (b64) and print
    GenEd25519(GenKeyArgs),
    /// Sign a capsule file with ed25519 secret key (b64) and print signature
    SignEd25519(SignArgs),
    /// Verify a capsule file signature with ed25519 public key (b64)
    VerifyEd25519(VerifyArgs),
    /// Fetch active policy capsules from the server
    Status(CapsuleStatusArgs),
}

#[derive(Args)]
struct PathsArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
struct ToolsArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
struct GateKeysArgs {
    /// Show grouped metadata and stability details
    #[arg(long, conflicts_with_all = ["json", "doc"])]
    details: bool,
    /// Emit JSON instead of text
    #[arg(long, conflicts_with_all = ["details", "doc"])]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Render the Markdown reference (matches docs)
    #[arg(long, conflicts_with_all = ["json", "details"])]
    doc: bool,
}

#[derive(Args)]
struct GateConfigSchemaArgs {
    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
struct GateConfigDocArgs {}

#[derive(Args)]
struct GenKeyArgs {
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
struct SignArgs {
    /// Secret key (b64)
    sk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Write signature to this file (optional)
    #[arg(long)]
    out: Option<String>,
}

#[derive(Args)]
struct TemplateArgs {
    /// Pretty-print JSON (default on unless --compact)
    #[arg(long)]
    pretty: bool,
    /// Print compact JSON (overrides --pretty)
    #[arg(long)]
    compact: bool,
}

#[derive(Args)]
struct VerifyArgs {
    /// Public key (b64)
    pk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Signature (b64)
    sig_b64: String,
}

#[derive(Args)]
struct CapsuleStatusArgs {
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

#[derive(Args)]
struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, powershell, elvish)
    shell: clap_complete::Shell,
    /// Output directory (writes a file). If not set, prints to stdout.
    #[arg(long)]
    out_dir: Option<String>,
}

#[derive(Args)]
struct PingArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
}

#[derive(Subcommand)]
enum SpecCmd {
    /// Fetch /spec/health and print JSON
    Health(SpecHealthArgs),
}

#[derive(Args)]
struct SpecHealthArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Subcommand)]
enum ScreenshotsCmd {
    /// Re-run OCR for screenshots missing per-language sidecars
    BackfillOcr(BackfillOcrArgs),
}

#[derive(Subcommand)]
enum RuntimeCmd {
    /// Show runtime supervisor snapshot with restart budgets
    Status(RuntimeStatusArgs),
    /// Request a managed runtime restore
    Restore(RuntimeRestoreArgs),
}

#[derive(Subcommand)]
enum EventsCmd {
    /// Tail the journal via /admin/events/journal
    Journal(EventsJournalArgs),
}

#[derive(Args)]
struct RuntimeBaseArgs {
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

#[derive(Args)]
struct RuntimeStatusArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Emit JSON instead of human summary
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
struct RuntimeRestoreArgs {
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
struct EventsJournalArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Maximum number of entries to request (server caps at 1000)
    #[arg(long, default_value_t = 200)]
    limit: usize,
    /// CSV of event prefixes to include (dot.case)
    #[arg(long)]
    prefix: Option<String>,
    /// Emit raw JSON instead of text summary
    #[arg(long, conflicts_with = "follow")]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Show journal source files in text mode
    #[arg(long)]
    show_sources: bool,
    /// Poll continuously for new entries
    #[arg(long)]
    follow: bool,
    /// Poll interval in seconds when following (default 5)
    #[arg(long, default_value_t = 5, requires = "follow")]
    interval: u64,
    /// Skip entries at or before this RFC3339 timestamp on the first fetch
    #[arg(long = "after")]
    after_cursor: Option<String>,
}

#[derive(Args)]
struct BackfillOcrArgs {
    /// Base URL of the service running arw-server
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token (falls back to ARW_ADMIN_TOKEN)
    #[arg(long)]
    admin_token: Option<String>,
    /// Language to OCR (tessdata language code)
    #[arg(long, default_value = "eng")]
    lang: String,
    /// Force OCR even if a cached sidecar exists
    #[arg(long)]
    force: bool,
    /// Only print the files that would be processed
    #[arg(long)]
    dry_run: bool,
    /// Limit number of screenshots to process
    #[arg(long)]
    limit: Option<usize>,
    /// Timeout in seconds for each HTTP call
    #[arg(long, default_value_t = 20)]
    timeout: u64,
    /// Show per-file progress
    #[arg(long)]
    verbose: bool,
}

#[derive(Debug, Clone)]
struct ScreenshotTarget {
    path: PathBuf,
    sidecar: PathBuf,
}

#[derive(Debug, Default, Clone, Copy)]
struct ScanStats {
    scanned: usize,
    skipped_cached: usize,
    skipped_other: usize,
}

fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Paths(args)) => {
            let v = load_effective_paths();
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                );
            } else {
                println!("{}", v);
            }
        }
        Some(Commands::Tools(args)) => {
            let list = introspect_tools();
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".to_string())
                );
            } else {
                match serde_json::to_string(&list) {
                    Ok(s) => println!("{}", s),
                    Err(_) => println!("[]"),
                }
            }
        }
        Some(Commands::Gate { cmd }) => match cmd {
            GateCmd::Keys(args) => {
                if args.json {
                    let payload = gating_keys::render_json(None);
                    if args.pretty {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
                        );
                    } else {
                        println!(
                            "{}",
                            serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
                        );
                    }
                } else if args.doc {
                    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
                    print!("{}", gating_keys::render_markdown(&now));
                } else if args.details {
                    let groups = gating_keys::groups();
                    let total_keys: usize = groups.iter().map(|g| g.keys.len()).sum();
                    println!(
                        "Total groups: {} | Total keys: {}\n",
                        groups.len(),
                        total_keys
                    );
                    for group in groups {
                        println!("{} — {}", group.name, group.summary);
                        for key in group.keys {
                            println!("  {:<24} {:<8} {}", key.id, key.stability, key.summary);
                        }
                        println!();
                    }
                } else {
                    for key in gating_keys::list() {
                        println!("{}", key);
                    }
                }
            }
            GateCmd::Config { cmd } => match cmd {
                GateConfigCmd::Schema(args) => {
                    let schema = gating::gating_config_schema_json();
                    if args.pretty {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&schema)
                                .unwrap_or_else(|_| "{}".to_string())
                        );
                    } else {
                        println!(
                            "{}",
                            serde_json::to_string(&schema).unwrap_or_else(|_| "{}".to_string())
                        );
                    }
                }
                GateConfigCmd::Doc(_) => {
                    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
                    print!("{}", gating::render_config_markdown(&now));
                }
            },
        },
        Some(Commands::Capsule { cmd }) => match cmd {
            CapCmd::Template(args) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let tpl = serde_json::json!({
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
                if args.compact {
                    println!("{}", serde_json::to_string(&tpl).unwrap());
                } else {
                    // default pretty unless explicitly compact
                    if args.pretty || !args.compact {
                        println!("{}", serde_json::to_string_pretty(&tpl).unwrap());
                    } else {
                        println!("{}", serde_json::to_string(&tpl).unwrap());
                    }
                }
            }
            CapCmd::GenEd25519(args) => {
                if let Err(e) = cmd_gen_ed25519(
                    args.out_pub.as_deref(),
                    args.out_priv.as_deref(),
                    args.issuer.as_deref(),
                ) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            CapCmd::SignEd25519(args) => {
                if let Err(e) =
                    cmd_sign_ed25519(&args.sk_b64, &args.capsule_json, args.out.as_deref())
                {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            CapCmd::VerifyEd25519(args) => {
                if let Err(e) = cmd_verify_ed25519(&args.pk_b64, &args.capsule_json, &args.sig_b64)
                {
                    eprintln!("{}", e);
                    std::process::exit(1);
                } else {
                    println!("ok");
                }
            }
            CapCmd::Status(args) => {
                if let Err(e) = cmd_capsule_status(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Completions(args)) => {
            if let Err(e) = cmd_completions(args.shell, args.out_dir.as_deref()) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Ping(args)) => {
            if let Err(e) = cmd_ping(&args) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Spec { cmd: spec }) => match spec {
            SpecCmd::Health(args) => {
                if let Err(e) = cmd_spec_health(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Screenshots { cmd }) => match cmd {
            ScreenshotsCmd::BackfillOcr(args) => {
                if let Err(e) = cmd_backfill_ocr(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Runtime { cmd }) => match cmd {
            RuntimeCmd::Status(args) => {
                if let Err(e) = cmd_runtime_status(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            RuntimeCmd::Restore(args) => {
                if let Err(e) = cmd_runtime_restore(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Events { cmd }) => match cmd {
            EventsCmd::Journal(args) => {
                if let Err(e) = cmd_events_journal(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        None => {
            println!("arw-cli {} — bootstrap", env!("CARGO_PKG_VERSION"));
            hello_core();
            println!("{}", load_effective_paths());
        }
    }
}

fn cmd_backfill_ocr(args: &BackfillOcrArgs) -> Result<()> {
    const SUPPORTED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];

    let paths = load_effective_paths();
    let state_dir = paths
        .get("state_dir")
        .and_then(JsonValue::as_str)
        .map(PathBuf::from)
        .context("state_dir missing from effective paths")?;
    let screenshots_dir = state_dir.join("screenshots");
    if !screenshots_dir.exists() {
        println!(
            "No screenshots directory found at {}",
            screenshots_dir.display()
        );
        return Ok(());
    }

    let lang_fragment = sanitize_lang_fragment_cli(&args.lang);
    let (mut targets, stats) = collect_screenshot_targets(
        &screenshots_dir,
        &lang_fragment,
        args.force,
        args.limit,
        SUPPORTED_EXTENSIONS,
    )?;

    if targets.is_empty() {
        println!(
            "No screenshots required OCR (scanned: {}, skipped existing: {}, skipped other: {})",
            stats.scanned, stats.skipped_cached, stats.skipped_other
        );
        return Ok(());
    }

    if args.dry_run {
        for target in &targets {
            println!(
                "[dry-run] {} -> {}",
                target.path.display(),
                target
                    .sidecar
                    .file_name()
                    .map(|s| s.to_string_lossy())
                    .unwrap_or_default()
            );
        }
        println!(
            "Dry run only ({} candidates, scanned {}, skipped existing {}, skipped other {})",
            targets.len(),
            stats.scanned,
            stats.skipped_cached,
            stats.skipped_other
        );
        return Ok(());
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/tools/run", base);

    let mut processed = 0usize;
    let mut failures: Vec<(PathBuf, String)> = Vec::new();

    for target in targets.drain(..) {
        let path = target.path;
        if args.verbose {
            println!("Running OCR for {}", path.display());
        }
        let path_str = path.to_string_lossy().to_string();
        let mut payload = json!({
            "id": "ui.screenshot.ocr",
            "input": {
                "path": path_str,
                "lang": args.lang,
            }
        });
        if args.force {
            if let Some(input) = payload.get_mut("input") {
                input["force"] = JsonValue::Bool(true);
            }
        }
        let mut req = client.post(&url).json(&payload);
        if let Some(ref tok) = token {
            req = req.header("X-ARW-Admin", tok);
            req = req.bearer_auth(tok);
        }
        let resp = match req.send() {
            Ok(resp) => resp,
            Err(err) => {
                failures.push((path.clone(), err.to_string()));
                if args.verbose {
                    eprintln!("  error: {}", err);
                }
                continue;
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            failures.push((path.clone(), format!("{}: {}", status, text)));
            if args.verbose {
                eprintln!("  server error: {} {}", status, text);
            }
            continue;
        }
        match resp.json::<JsonValue>() {
            Ok(body) => {
                processed += 1;
                if args.verbose {
                    let cached = body
                        .get("cached")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let lang = body
                        .get("lang")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&args.lang);
                    println!("  ok (lang={}, cached={})", lang, cached);
                }
                // capture alt text for markdown by storing text snippet in local cache file? Not needed; UI listens to event.
            }
            Err(err) => {
                failures.push((path.clone(), err.to_string()));
                if args.verbose {
                    eprintln!("  parse error: {}", err);
                }
            }
        }
    }

    println!(
        "OCR backfill complete: processed {}, failures {}, scanned {}, skipped existing {} (other {})",
        processed,
        failures.len(),
        stats.scanned,
        stats.skipped_cached,
        stats.skipped_other
    );
    if !failures.is_empty() {
        eprintln!("Failures:");
        for (path, err) in failures {
            eprintln!("  {} => {}", path.display(), err);
        }
        return Err(anyhow::anyhow!("some OCR requests failed"));
    }
    Ok(())
}

fn cmd_runtime_status(args: &RuntimeStatusArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base.trim_end_matches('/');
    let url = format!("{}/state/runtime_supervisor", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .context("requesting runtime supervisor snapshot")?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime supervisor response")?;
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

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
            );
        } else {
            println!("{}", body);
        }
        return Ok(());
    }

    println!("{}", render_runtime_summary(&body));
    Ok(())
}

fn cmd_runtime_restore(args: &RuntimeRestoreArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base.trim_end_matches('/');
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

fn cmd_events_journal(args: &EventsJournalArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let after_time = if let Some(ref cursor) = args.after_cursor {
        match chrono::DateTime::parse_from_rfc3339(cursor) {
            Ok(dt) => Some(dt.with_timezone(&chrono::Utc)),
            Err(_) => {
                anyhow::bail!("--after must be an RFC3339 timestamp (e.g. 2025-10-02T17:15:00Z)");
            }
        }
    } else {
        None
    };

    let mut body = fetch_journal_snapshot(
        &client,
        base,
        token.as_deref(),
        args.limit,
        args.prefix.as_deref(),
    )?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string())
            );
        } else {
            println!("{}", body);
        }
        return Ok(());
    }

    let mut first_pass = true;
    let mut state = if args.follow {
        Some(JournalPrintState::new(args.limit.max(512)))
    } else {
        None
    };

    loop {
        let apply_after = if first_pass {
            after_time.as_ref()
        } else {
            None
        };
        let _printed = render_journal_text(
            &body,
            args.show_sources,
            first_pass,
            apply_after,
            state.as_mut(),
        );
        if !args.follow {
            return Ok(());
        }
        first_pass = false;
        std::thread::sleep(Duration::from_secs(args.interval.max(1)));
        body = fetch_journal_snapshot(
            &client,
            base,
            token.as_deref(),
            args.limit,
            args.prefix.as_deref(),
        )?;
    }
}

fn fetch_journal_snapshot(
    client: &Client,
    base: &str,
    token: Option<&str>,
    limit: usize,
    prefix: Option<&str>,
) -> Result<JsonValue> {
    let url = format!("{}/admin/events/journal", base);
    let mut params: Vec<(String, String)> = vec![("limit".into(), limit.to_string())];
    if let Some(pref) = prefix {
        let trimmed = pref.trim();
        if !trimmed.is_empty() {
            params.push(("prefix".into(), trimmed.to_string()));
        }
    }
    let mut req = client.get(&url);
    if !params.is_empty() {
        req = req.query(&params);
    }
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("journal disabled: set ARW_EVENTS_JOURNAL on the server and restart");
    }
    let body: JsonValue = resp.json().context("parsing journal response")?;
    if !status.is_success() {
        anyhow::bail!("journal request failed: {} {}", status, body);
    }
    Ok(body)
}

fn render_journal_text(
    body: &JsonValue,
    show_sources: bool,
    first_pass: bool,
    after: Option<&chrono::DateTime<chrono::Utc>>,
    mut state: Option<&mut JournalPrintState>,
) -> usize {
    let limit = body
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let total = body
        .get("total_matched")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let truncated = body
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let skipped = body
        .get("skipped_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let prefixes: Vec<String> = body
        .get("prefixes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let entries = body
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let source_files: Vec<String> = body
        .get("source_files")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut printable: Vec<(JsonValue, String)> = Vec::new();
    for entry in entries {
        let key = entry_identity(&entry);
        if let Some(st) = state.as_ref() {
            if st.seen(&key) {
                continue;
            }
        }
        if let Some(after_ts) = after {
            if let Some(entry_ts) = entry_timestamp(&entry) {
                if entry_ts <= *after_ts {
                    if let Some(st) = state.as_mut() {
                        st.record(key);
                    }
                    continue;
                }
            }
        }
        printable.push((entry, key));
    }

    if first_pass {
        let prefix_label = if prefixes.is_empty() {
            "(none)".to_string()
        } else {
            prefixes.join(", ")
        };
        println!(
            "Journal entries: returned {} (limit {}), total matches {}, truncated: {}, skipped lines {}",
            printable.len(),
            limit,
            total,
            truncated,
            skipped
        );
        println!("Prefixes: {}", prefix_label);
        if show_sources && !source_files.is_empty() {
            println!("Sources:");
            for path in source_files {
                println!("  {}", path);
            }
        }
        if printable.is_empty() {
            println!("No journal entries matched the query.");
            return 0;
        }
    } else if printable.is_empty() {
        return 0;
    } else {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        println!("-- poll @ {}: {} new entries --", now, printable.len());
    }

    let mut new_count = 0usize;
    for (entry, key) in printable {
        let time = entry.get("time").and_then(|v| v.as_str()).unwrap_or("-");
        let kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let payload = entry.get("payload").cloned().unwrap_or(JsonValue::Null);
        let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "null".into());
        let preview = truncate_payload(&payload_str, 160);
        println!("[{}] {}", time, kind);
        println!("  payload: {}", preview);
        if let Some(policy) = entry.get("policy") {
            if !policy.is_null() {
                let policy_str = serde_json::to_string(policy).unwrap_or_else(|_| "{}".into());
                println!("  policy: {}", truncate_payload(&policy_str, 120));
            }
        }
        if let Some(ce) = entry.get("ce") {
            if !ce.is_null() {
                let ce_str = serde_json::to_string(ce).unwrap_or_else(|_| "{}".into());
                println!("  ce: {}", truncate_payload(&ce_str, 120));
            }
        }
        if let Some(st) = state.as_mut() {
            st.record(key);
        }
        new_count += 1;
    }

    new_count
}

struct JournalPrintState {
    seen: HashSet<String>,
    order: VecDeque<String>,
    cap: usize,
}

impl JournalPrintState {
    fn new(cap: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            cap: cap.max(64),
        }
    }

    fn seen(&self, key: &str) -> bool {
        self.seen.contains(key)
    }

    fn record(&mut self, key: String) {
        if self.seen.insert(key.clone()) {
            self.order.push_back(key);
            if self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.seen.remove(&old);
                }
            }
        }
    }
}

fn entry_identity(entry: &JsonValue) -> String {
    let payload = entry.get("payload").cloned().unwrap_or(JsonValue::Null);
    let policy = entry.get("policy").cloned().unwrap_or(JsonValue::Null);
    let ce = entry.get("ce").cloned().unwrap_or(JsonValue::Null);
    format!(
        "{}|{}|{}|{}|{}",
        entry.get("time").and_then(|v| v.as_str()).unwrap_or(""),
        entry.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
        serde_json::to_string(&payload).unwrap_or_default(),
        serde_json::to_string(&policy).unwrap_or_default(),
        serde_json::to_string(&ce).unwrap_or_default()
    )
}

fn entry_timestamp(entry: &JsonValue) -> Option<chrono::DateTime<chrono::Utc>> {
    let time_str = entry.get("time")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(time_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
}

fn truncate_payload(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn sanitize_lang_fragment_cli(lang: &str) -> String {
    let mut out = String::new();
    for c in lang.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if matches!(c, '+' | '-' | '_') {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "eng".into()
    } else {
        out
    }
}

fn collect_screenshot_targets(
    screenshots_dir: &Path,
    lang_fragment: &str,
    force: bool,
    limit: Option<usize>,
    extensions: &[&str],
) -> Result<(Vec<ScreenshotTarget>, ScanStats)> {
    let mut stats = ScanStats::default();
    let mut targets = Vec::new();

    for entry in WalkDir::new(screenshots_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let ext = match path.extension().and_then(|s| s.to_str()) {
            Some(ext) => ext.to_ascii_lowercase(),
            None => {
                stats.skipped_other += 1;
                continue;
            }
        };
        if !extensions.contains(&ext.as_str()) {
            stats.skipped_other += 1;
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(stem) => stem,
            None => {
                stats.skipped_other += 1;
                continue;
            }
        };
        if stem.ends_with(".ann") || stem.contains(".ocr.") {
            stats.skipped_other += 1;
            continue;
        }

        stats.scanned += 1;
        let parent = match path.parent() {
            Some(parent) => parent,
            None => continue,
        };
        let sidecar = parent.join(format!("{}.ocr.{}.json", stem, lang_fragment));
        if !force && sidecar.exists() {
            stats.skipped_cached += 1;
            continue;
        }

        targets.push(ScreenshotTarget {
            path: path.to_path_buf(),
            sidecar,
        });

        if let Some(limit) = limit {
            if targets.len() >= limit {
                break;
            }
        }
    }

    Ok((targets, stats))
}

fn cmd_gen_ed25519(
    out_pub: Option<&str>,
    out_priv: Option<&str>,
    issuer: Option<&str>,
) -> Result<()> {
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
        let state = status
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_lowercase();
        let severity = status
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_lowercase();
        let summary = status
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("(no summary)");
        if state == "ready" {
            ready += 1;
        }
        if state == "error" || severity == "error" || state == "offline" {
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
            "- {} ({}) [{}] — {} ({})",
            name, adapter, id, summary, state
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

fn resolve_admin_token(opt: &Option<String>) -> Option<String> {
    opt.clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok())
        .filter(|s| !s.trim().is_empty())
}

fn with_admin_headers(
    mut req: reqwest::blocking::RequestBuilder,
    token: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    if let Some(tok) = token {
        req = req.header("X-ARW-Admin", tok);
        req = req.bearer_auth(tok);
    }
    req
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

fn format_local_timestamp(ms: u64) -> String {
    match Utc.timestamp_millis_opt(ms as i64).single() {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S %Z")
            .to_string(),
        None => "(invalid timestamp)".to_string(),
    }
}

fn format_relative_from_now(target_ms: u64, now_ms: u64) -> String {
    let diff = target_ms as i128 - now_ms as i128;
    let future = diff >= 0;
    let abs = if future {
        diff as u128
    } else {
        (-diff) as u128
    };
    let seconds = (abs / 1_000) as u64;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let label = if days > 0 {
        format!("{}d", days)
    } else if hours > 0 {
        format!("{}h", hours)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        format!("{}s", seconds)
    };
    if future {
        format!("in {}", label)
    } else {
        format!("{} ago", label)
    }
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

fn cmd_completions(shell: clap_complete::Shell, out_dir: Option<&str>) -> Result<()> {
    use clap_complete::{generate, generate_to};
    use std::io::stdout;
    let mut cmd = Cli::command();
    let bin = "arw-cli";
    if let Some(dir) = out_dir {
        let dir_path = std::path::Path::new(dir);
        std::fs::create_dir_all(dir_path).ok();
        let _path = generate_to(shell, &mut cmd, bin, dir_path)?;
    } else {
        generate(shell, &mut cmd, bin, &mut stdout());
    }
    Ok(())
}

fn cmd_ping(args: &PingArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(args.timeout))
        .build()?;
    let mut headers = reqwest::header::HeaderMap::new();
    let tok = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());
    if let Some(t) = tok.as_deref() {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)).unwrap(),
        );
    }
    let h = client
        .get(format!("{}/healthz", base))
        .headers(headers.clone())
        .send()?;
    let ok_health = h.status().is_success();
    let a = client
        .get(format!("{}/about", base))
        .headers(headers)
        .send()?;
    let about_json: serde_json::Value = a.json().unwrap_or_else(|_| serde_json::json!({}));
    let out = serde_json::json!({
        "base": base,
        "healthz": {"status": h.status().as_u16()},
        "about": about_json,
        "ok": ok_health,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_spec_health(args: &SpecHealthArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/spec/health", base);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let resp = client.get(url).send()?;
    let txt = resp.text()?;
    if args.pretty {
        let v: serde_json::Value =
            serde_json::from_str(&txt).unwrap_or_else(|_| serde_json::json!({}));
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("{}", txt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn sanitize_lang_fragment_cli_normalizes() {
        assert_eq!(sanitize_lang_fragment_cli("ENg"), "eng");
        assert_eq!(sanitize_lang_fragment_cli("fr+best"), "fr+best");
        assert_eq!(sanitize_lang_fragment_cli(" zh - Hans "), "zh_-_hans");
        assert_eq!(sanitize_lang_fragment_cli(""), "eng");
        assert_eq!(sanitize_lang_fragment_cli("@!#"), "___");
    }

    #[test]
    fn collect_screenshot_targets_skips_sidecars_and_respects_limit() -> Result<()> {
        const EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp"];
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("2025/09/30");
        fs::create_dir_all(&root)?;

        let shot1 = root.join("one.png");
        fs::write(&shot1, b"fake")?;
        let shot2 = root.join("two.png");
        fs::write(&shot2, b"fake")?;
        let shot3 = root.join("three.ann.png");
        fs::write(&shot3, b"fake")?;
        let not_image = root.join("note.txt");
        fs::write(&not_image, b"text")?;

        let sidecar1 = root.join("one.ocr.eng.json");
        let mut f = fs::File::create(&sidecar1)?;
        writeln!(f, "{{}}")?;

        let (targets, stats) = collect_screenshot_targets(tmp.path(), "eng", false, Some(5), EXT)?;

        assert_eq!(stats.scanned, 2); // one.png and two.png
        assert_eq!(stats.skipped_cached, 1);
        assert!(stats.skipped_other >= 2); // .ann + text + sidecar pattern
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, shot2);
        assert!(targets[0].sidecar.ends_with("two.ocr.eng.json"));

        let (targets_force, _stats_force) =
            collect_screenshot_targets(tmp.path(), "eng", true, Some(1), EXT)?;
        assert_eq!(targets_force.len(), 1);

        Ok(())
    }
}
