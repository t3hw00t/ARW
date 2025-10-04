use anyhow::{anyhow, bail, ensure, Context, Result};
use arw_core::{gating, gating_keys, hello_core, introspect_tools, load_effective_paths};
use base64::Engine;
use chrono::{DateTime, Local, TimeZone, Utc};
use clap::CommandFactory;
use clap::{Args, Parser, Subcommand, ValueEnum};
use json_patch::{patch as apply_json_patch, Patch as JsonPatch};
use rand::RngCore;
use reqwest::{blocking::Client, header::ACCEPT, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value as JsonValue};
use sha2::Digest;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::env;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Read as _, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
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
    /// Tool helpers (list, cache stats)
    Tools {
        #[command(flatten)]
        list: ToolsListArgs,
        #[command(subcommand)]
        cmd: Option<ToolsSubcommand>,
    },
    /// Admin helpers
    Admin {
        #[command(subcommand)]
        cmd: AdminCmd,
    },
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
    /// State snapshots
    State {
        #[command(subcommand)]
        cmd: StateCmd,
    },
    /// Context and training telemetry helpers
    Context {
        #[command(subcommand)]
        cmd: ContextCmd,
    },
    /// Event journal helpers
    Events {
        #[command(subcommand)]
        cmd: EventsCmd,
    },
    /// Smoke checks for local validation
    Smoke {
        #[command(subcommand)]
        cmd: SmokeCmd,
    },
}

#[derive(Subcommand)]
enum SmokeCmd {
    /// Run action/state/event smoke checks
    Triad(SmokeTriadArgs),
    /// Run context telemetry smoke checks
    Context(SmokeContextArgs),
}

#[derive(Args, Clone)]
struct SmokeCommonArgs {
    /// Port to bind the temporary server on (default: 18181 / 18182)
    #[arg(long)]
    port: Option<u16>,
    /// Path to an existing arw-server binary; auto-detected when omitted
    #[arg(long)]
    server_bin: Option<PathBuf>,
    /// Seconds to wait for /healthz to become ready
    #[arg(long, default_value_t = 30)]
    wait_timeout_secs: u64,
    /// Preserve the temporary state/logs directory instead of deleting it
    #[arg(long)]
    keep_temp: bool,
}

#[derive(Args, Clone)]
struct SmokeTriadArgs {
    #[command(flatten)]
    common: SmokeCommonArgs,
    /// Admin token to use; defaults to an ephemeral value
    #[arg(long)]
    admin_token: Option<String>,
}

#[derive(Args, Clone)]
struct SmokeContextArgs {
    #[command(flatten)]
    common: SmokeCommonArgs,
    /// Admin token to use; defaults to an ephemeral value
    #[arg(long)]
    admin_token: Option<String>,
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

#[derive(Args, Default, Clone, Copy)]
struct ToolsListArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Subcommand)]
enum ToolsSubcommand {
    /// Print tool list (JSON)
    List(ToolsListArgs),
    /// Fetch tool cache statistics from the server
    Cache(ToolsCacheArgs),
}

#[derive(Args, Clone)]
struct ToolsCacheArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 10)]
    timeout: u64,
    /// Emit raw JSON instead of a human summary
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (only with --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Subcommand)]
enum AdminCmd {
    /// Admin token helpers
    Token {
        #[command(subcommand)]
        cmd: AdminTokenCmd,
    },
}

#[derive(Subcommand)]
enum AdminTokenCmd {
    /// Hash an admin token for ARW_ADMIN_TOKEN_SHA256
    Hash(AdminTokenHashArgs),
    /// Generate a random admin token
    Generate(AdminTokenGenerateArgs),
}

#[derive(Args, Clone)]
struct AdminTokenHashArgs {
    /// Plain admin token; omit to read from environment
    #[arg(long)]
    token: Option<String>,
    /// Read token from stdin (conflicts with --token)
    #[arg(long, conflicts_with = "token")]
    stdin: bool,
    /// Environment variable to read when --token is absent
    #[arg(long = "read-env", default_value = "ARW_ADMIN_TOKEN")]
    read_env: String,
    /// Print as ARW_ADMIN_TOKEN_SHA256=<hash>
    #[arg(long, conflicts_with = "export-shell")]
    env: bool,
    /// Print as export ARW_ADMIN_TOKEN_SHA256=<hash>
    #[arg(long = "export-shell", conflicts_with = "env")]
    export_shell: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum TokenFormat {
    Hex,
    Base64,
}

#[derive(Args, Clone)]
struct AdminTokenGenerateArgs {
    /// Number of random bytes to generate (default: 32)
    #[arg(long, default_value_t = 32)]
    length: usize,
    /// Output format
    #[arg(long, value_enum, default_value_t = TokenFormat::Hex)]
    format: TokenFormat,
    /// Upper-case hex output (ignored for base64)
    #[arg(long)]
    uppercase: bool,
    /// Print as export ARW_ADMIN_TOKEN=<token>
    #[arg(long = "export-shell", conflicts_with = "env")]
    export_shell: bool,
    /// Print as ARW_ADMIN_TOKEN=<token>
    #[arg(long, conflicts_with = "export-shell")]
    env: bool,
    /// Also print SHA-256 hash (raw)
    #[arg(long, conflicts_with_all = ["hash_env", "hash_export_shell"])]
    hash: bool,
    /// Print hash as ARW_ADMIN_TOKEN_SHA256=<hash>
    #[arg(long = "hash-env", conflicts_with = "hash_export_shell")]
    hash_env: bool,
    /// Print hash as export ARW_ADMIN_TOKEN_SHA256=<hash>
    #[arg(long = "hash-export-shell")]
    hash_export_shell: bool,
}

#[derive(Debug, Deserialize)]
struct ToolCacheSnapshot {
    hit: u64,
    miss: u64,
    coalesced: u64,
    errors: u64,
    bypass: u64,
    capacity: u64,
    ttl_secs: u64,
    entries: u64,
    latency_saved_ms_total: u64,
    latency_saved_samples: u64,
    avg_latency_saved_ms: f64,
    payload_bytes_saved_total: u64,
    payload_saved_samples: u64,
    avg_payload_bytes_saved: f64,
    avg_hit_age_secs: f64,
    hit_age_samples: u64,
    last_hit_age_secs: Option<u64>,
    max_hit_age_secs: Option<u64>,
    stampede_suppression_rate: f64,
    last_latency_saved_ms: Option<u64>,
    last_payload_bytes: Option<u64>,
    #[serde(flatten)]
    _extra: serde_json::Map<String, JsonValue>,
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
enum StateCmd {
    /// Snapshot filtered actions via /state/actions
    Actions(StateActionsArgs),
}

#[derive(Subcommand)]
enum ContextCmd {
    /// Fetch /state/training/telemetry and render a summary
    Telemetry(ContextTelemetryArgs),
}

#[derive(Subcommand)]
enum EventsCmd {
    /// Snapshot the observations read-model via /state/observations
    Observations(EventsObservationsArgs),
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
struct ContextTelemetryArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON snapshot
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
struct StateActionsArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Maximum number of items to request (server clamps 1-2000)
    #[arg(long)]
    limit: Option<usize>,
    /// Filter by action state (queued|running|completed|failed)
    #[arg(long)]
    state: Option<String>,
    /// Restrict kinds by prefix (e.g., chat.)
    #[arg(long)]
    kind_prefix: Option<String>,
    /// Only include actions updated at or after this RFC3339 timestamp
    #[arg(long)]
    updated_since: Option<String>,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Width for the rendered kind column in text output (ignored in JSON mode)
    #[arg(long, default_value_t = 36)]
    kind_width: usize,
    /// Stream live updates via state.read.model.patch SSE
    #[arg(long, conflicts_with = "json")]
    watch: bool,
}

#[derive(Args)]
struct EventsObservationsArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Maximum number of items to request (defaults to server window when omitted)
    #[arg(long)]
    limit: Option<usize>,
    /// Filter to observation kinds starting with this prefix (e.g., actions.)
    #[arg(long)]
    kind_prefix: Option<String>,
    /// Only include observations newer than this RFC3339 timestamp
    #[arg(long, conflicts_with = "since_relative")]
    since: Option<String>,
    /// Relative lookback (e.g., 15m, 2h30m) converted to an absolute `since`
    #[arg(long, value_name = "WINDOW", conflicts_with = "since")]
    since_relative: Option<String>,
    /// Emit raw JSON instead of a formatted summary
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Maximum characters of payload JSON to show per row (set 0 to hide)
    #[arg(long, default_value_t = 120)]
    payload_width: usize,
    /// Include policy metadata if present on events
    #[arg(long)]
    show_policy: bool,
    /// Stream live updates via state.read.model.patch SSE
    #[arg(long, conflicts_with = "json")]
    watch: bool,
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

fn print_tools_list(pretty: bool) {
    let list = introspect_tools();
    if pretty {
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
        Some(Commands::Tools { list, cmd }) => match cmd {
            Some(ToolsSubcommand::Cache(args)) => {
                if let Err(e) = cmd_tools_cache(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            Some(ToolsSubcommand::List(args)) => print_tools_list(args.pretty),
            None => print_tools_list(list.pretty),
        },
        Some(Commands::Admin { cmd }) => match cmd {
            AdminCmd::Token { cmd } => match cmd {
                AdminTokenCmd::Hash(args) => {
                    if let Err(e) = cmd_admin_token_hash(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminTokenCmd::Generate(args) => {
                    if let Err(e) = cmd_admin_token_generate(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            },
        },
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
                    let rendered = if args.pretty {
                        serde_json::to_string_pretty(&schema)
                    } else {
                        serde_json::to_string(&schema)
                    };
                    match rendered {
                        Ok(doc) => println!("{}", doc),
                        Err(err) => {
                            eprintln!("failed to render gating config schema: {err}");
                            println!("{{}}");
                        }
                    }
                }
                GateConfigCmd::Doc(_) => {
                    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
                    print!("{}", gating::render_config_markdown(&now));
                }
            },
        },
        Some(Commands::Smoke { cmd }) => match cmd {
            SmokeCmd::Triad(args) => {
                if let Err(e) = cmd_smoke_triad(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            SmokeCmd::Context(args) => {
                if let Err(e) = cmd_smoke_context(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Capsule { cmd }) => match cmd {
            CapCmd::Template(args) => {
                let duration = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO);
                let now = (duration.as_millis()).min(u128::from(u64::MAX)) as u64;
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
                let serialized = if args.compact {
                    serde_json::to_string(&tpl).map_err(|e| {
                        anyhow::anyhow!("failed to render capsule template JSON (compact): {e}")
                    })
                } else {
                    // default pretty unless explicitly compact
                    serde_json::to_string_pretty(&tpl).map_err(|e| {
                        anyhow::anyhow!("failed to render capsule template JSON (pretty): {e}")
                    })
                };
                match serialized {
                    Ok(output) => println!("{}", output),
                    Err(err) => {
                        eprintln!("{}", err);
                        std::process::exit(1);
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
        Some(Commands::State { cmd }) => match cmd {
            StateCmd::Actions(args) => {
                if let Err(e) = cmd_state_actions(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Context { cmd }) => match cmd {
            ContextCmd::Telemetry(args) => {
                if let Err(e) = cmd_context_telemetry(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Events { cmd }) => match cmd {
            EventsCmd::Observations(args) => {
                if let Err(e) = cmd_events_observations(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
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

fn cmd_tools_cache(args: &ToolsCacheArgs) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/admin/tools/cache_stats", base);
    let resp = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let text = resp.text().context("reading cache stats response")?;

    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow::anyhow!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN"
        ));
    }
    if !status.is_success() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("cache stats request failed: {}", status));
        }
        return Err(anyhow::anyhow!(
            "cache stats request failed: {} {}",
            status,
            trimmed
        ));
    }

    let raw: JsonValue = serde_json::from_str(&text).context("parsing cache stats JSON")?;
    let snapshot: ToolCacheSnapshot =
        serde_json::from_value(raw.clone()).context("deserializing cache stats snapshot")?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&raw).unwrap_or_else(|_| raw.to_string())
            );
        } else {
            println!("{}", raw);
        }
        return Ok(());
    }

    println!("{}", render_tool_cache_summary(&snapshot, base));
    Ok(())
}

fn cmd_admin_token_hash(args: &AdminTokenHashArgs) -> Result<()> {
    let token = if let Some(token) = &args.token {
        if token.is_empty() {
            anyhow::bail!("--token cannot be empty");
        }
        token.clone()
    } else if args.stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading token from stdin")?;
        let trimmed = buf.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            anyhow::bail!("stdin provided no token bytes");
        }
        trimmed.to_string()
    } else {
        let var = args.read_env.as_str();
        match std::env::var(var) {
            Ok(value) if !value.is_empty() => value,
            Ok(_) => anyhow::bail!("{} is set but empty", var),
            Err(_) => anyhow::bail!("provide --token or set {}", var),
        }
    };

    let digest = hash_admin_token(&token);
    if args.export_shell {
        println!("export ARW_ADMIN_TOKEN_SHA256={}", digest);
    } else if args.env {
        println!("ARW_ADMIN_TOKEN_SHA256={}", digest);
    } else {
        println!("{}", digest);
    }

    Ok(())
}

fn cmd_admin_token_generate(args: &AdminTokenGenerateArgs) -> Result<()> {
    if args.length == 0 {
        anyhow::bail!("--length must be greater than zero");
    }
    let mut bytes = vec![0u8; args.length];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);

    let token = match args.format {
        TokenFormat::Hex => encode_hex(&bytes, args.uppercase),
        TokenFormat::Base64 => base64::engine::general_purpose::STANDARD_NO_PAD.encode(&bytes),
    };

    if args.export_shell {
        println!("export ARW_ADMIN_TOKEN={}", token);
    } else if args.env {
        println!("ARW_ADMIN_TOKEN={}", token);
    } else {
        println!("{}", token);
    }

    if args.hash || args.hash_env || args.hash_export_shell {
        let digest = hash_admin_token(&token);
        if args.hash_export_shell {
            println!("export ARW_ADMIN_TOKEN_SHA256={}", digest);
        } else if args.hash_env {
            println!("ARW_ADMIN_TOKEN_SHA256={}", digest);
        } else {
            println!("{}", digest);
        }
    }

    Ok(())
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
        let json = combine_runtime_snapshots(&body, matrix_snapshot);
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string())
            );
        } else {
            println!("{}", json);
        }
        return Ok(());
    }

    println!("{}", render_runtime_summary(&body));
    if let Some(matrix) = matrix_snapshot {
        println!();
        println!("{}", render_runtime_matrix_summary(&matrix));
    }
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

fn cmd_context_telemetry(args: &ContextTelemetryArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/training/telemetry", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token.as_deref());
    let resp = req
        .send()
        .context("requesting training telemetry snapshot")?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing training telemetry response")?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access telemetry"
        );
    }
    if !status.is_success() {
        anyhow::bail!("training telemetry request failed: {} {}", status, body);
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

    let now_ms = chrono::Utc::now().timestamp_millis();
    let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
    let summary = render_context_telemetry_summary(&body, now_ms);
    println!("{}", summary.trim_end());
    Ok(())
}

fn fetch_runtime_matrix(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<Option<JsonValue>> {
    let url = format!("{}/state/runtime_matrix", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("requesting runtime matrix snapshot from {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing runtime matrix response")?;
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

fn combine_runtime_snapshots(supervisor: &JsonValue, matrix: Option<JsonValue>) -> JsonValue {
    let mut wrapper = serde_json::Map::new();
    wrapper.insert("supervisor".to_string(), supervisor.clone());
    wrapper.insert("matrix".to_string(), matrix.unwrap_or(JsonValue::Null));
    JsonValue::Object(wrapper)
}

fn render_context_telemetry_summary(snapshot: &JsonValue, now_ms: u64) -> String {
    let mut out = String::new();
    if let Some(ms) = snapshot.get("generated_ms").and_then(JsonValue::as_u64) {
        let _ = writeln!(
            out,
            "Generated: {} ({})",
            format_local_timestamp(ms),
            format_relative_from_now(ms, now_ms)
        );
    } else if let Some(ts) = snapshot.get("generated").and_then(JsonValue::as_str) {
        let _ = writeln!(out, "Generated: {}", clean_text(ts));
    } else {
        let _ = writeln!(out, "Generated: unknown");
    }

    let Some(context) = snapshot.get("context").and_then(JsonValue::as_object) else {
        out.push('\n');
        let _ = writeln!(out, "Coverage:");
        let _ = writeln!(out, "  (no context telemetry)");
        return out;
    };

    out.push('\n');
    summarize_coverage_section(&mut out, context.get("coverage"));
    out.push('\n');
    summarize_recall_section(&mut out, context.get("recall_risk"));
    out.push('\n');
    summarize_working_set_section(&mut out, context.get("assembled"));

    out
}

fn summarize_coverage_section(out: &mut String, coverage: Option<&JsonValue>) {
    let _ = writeln!(out, "Coverage:");
    let Some(obj) = coverage.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no coverage data)");
        return;
    };

    if let Some(latest) = obj.get("latest").and_then(JsonValue::as_object) {
        let needs_more = latest
            .get("needs_more")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let verdict = if needs_more {
            "needs more coverage"
        } else {
            "coverage satisfied"
        };
        let _ = writeln!(out, "  Latest verdict: {}", verdict);

        if let Some(scope) = render_scope(latest.get("project"), latest.get("query")) {
            let _ = writeln!(out, "  Scope: {}", scope);
        }

        if let Some(reasons) = latest.get("reasons").and_then(JsonValue::as_array) {
            let mut labels: Vec<String> = reasons
                .iter()
                .filter_map(JsonValue::as_str)
                .map(format_coverage_reason)
                .collect();
            if !labels.is_empty() {
                labels.sort();
                labels.dedup();
                let _ = writeln!(out, "  Reasons: {}", labels.join(", "));
            }
        }

        if let Some(summary) = latest.get("summary").and_then(JsonValue::as_object) {
            if let Some(slots) = summary.get("slots").and_then(JsonValue::as_object) {
                if let Some(counts) = slots.get("counts").and_then(JsonValue::as_object) {
                    let mut entries: Vec<String> = counts
                        .iter()
                        .filter_map(|(slot, value)| value.as_u64().map(|count| (slot, count)))
                        .map(|(slot, count)| format!("{}={}", format_slot_name(slot), count))
                        .collect();
                    if !entries.is_empty() {
                        entries.sort();
                        let _ = writeln!(out, "  Slot counts: {}", entries.join(", "));
                    }
                }
                if let Some(budgets) = slots.get("budgets").and_then(JsonValue::as_object) {
                    let mut entries: Vec<String> = budgets
                        .iter()
                        .filter_map(|(slot, value)| value.as_u64().map(|count| (slot, count)))
                        .map(|(slot, count)| format!("{}≤{}", format_slot_name(slot), count))
                        .collect();
                    if !entries.is_empty() {
                        entries.sort();
                        let _ = writeln!(out, "  Budgets: {}", entries.join(", "));
                    }
                }
            }
        }
    } else {
        let _ = writeln!(out, "  Latest verdict unavailable");
    }

    if let Some(ratio) = obj.get("needs_more_ratio").and_then(JsonValue::as_f64) {
        let sample = obj
            .get("recent")
            .and_then(JsonValue::as_array)
            .map(|arr| arr.len())
            .unwrap_or(0);
        let window = if sample > 0 {
            format!("last {}", sample)
        } else {
            "recent".to_string()
        };
        let _ = writeln!(
            out,
            "  Needs-more ratio ({}): {}",
            window,
            format_percent(ratio, 0)
        );
    }

    if let Some(reasons) = obj.get("top_reasons").and_then(JsonValue::as_array) {
        let lines: Vec<String> = reasons
            .iter()
            .filter_map(|item| {
                let reason = item.get("reason").and_then(JsonValue::as_str)?;
                let count = item.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                Some(format!(
                    "{} · {}",
                    format_coverage_reason(reason),
                    format_count_label(count, "event")
                ))
            })
            .collect();
        if !lines.is_empty() {
            let _ = writeln!(out, "  Top gaps:");
            for line in lines.iter().take(3) {
                let _ = writeln!(out, "    - {}", line);
            }
        }
    }

    if let Some(slots) = obj.get("top_slots").and_then(JsonValue::as_array) {
        if !slots.is_empty() {
            let _ = writeln!(out, "  Top slot gaps:");
            for slot in slots.iter().take(3) {
                let name = slot
                    .get("slot")
                    .and_then(JsonValue::as_str)
                    .map(format_slot_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let count = slot.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                let _ = writeln!(out, "    - {} · {}", name, format_count_label(count, "gap"));
            }
        }
    }
}

fn summarize_recall_section(out: &mut String, recall: Option<&JsonValue>) {
    let _ = writeln!(out, "Recall risk:");
    let Some(obj) = recall.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no recall telemetry)");
        return;
    };

    if let Some(latest) = obj.get("latest").and_then(JsonValue::as_object) {
        let level = latest
            .get("level")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        let score = latest.get("score").and_then(JsonValue::as_f64);
        let at_risk = latest
            .get("at_risk")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let status = if at_risk { "at risk" } else { "stable" };
        let _ = writeln!(
            out,
            "  Latest level: {} ({}){}",
            level,
            percent_or_dash(score, 0),
            if at_risk { " · investigate" } else { "" }
        );
        let _ = writeln!(out, "  Status: {}", status);

        if let Some(components) = latest.get("components").and_then(JsonValue::as_object) {
            if let Some(value) = components
                .get("coverage_shortfall")
                .and_then(JsonValue::as_f64)
            {
                let _ = writeln!(out, "  Coverage shortfall: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("lane_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Lane gap: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("slot_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Slot gap: {}", format_percent(value, 0));
            }
            if let Some(value) = components.get("quality_gap").and_then(JsonValue::as_f64) {
                let _ = writeln!(out, "  Quality gap: {}", format_percent(value, 0));
            }
            if let Some(slots) = components.get("slots").and_then(JsonValue::as_object) {
                let mut entries: Vec<(String, f64)> = slots
                    .iter()
                    .filter_map(|(slot, value)| value.as_f64().map(|gap| (slot.clone(), gap)))
                    .collect();
                entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                entries.retain(|(_, gap)| *gap > 0.0);
                if !entries.is_empty() {
                    let _ = writeln!(out, "  Slot gaps:");
                    for (slot, gap) in entries.into_iter().take(3) {
                        let _ = writeln!(
                            out,
                            "    - {} · {}",
                            format_slot_name(&slot),
                            format_percent(gap, 0)
                        );
                    }
                }
            }
        }
    } else {
        let _ = writeln!(out, "  Latest level unavailable");
    }

    if let Some(avg) = obj.get("avg_score").and_then(JsonValue::as_f64) {
        let samples = obj.get("sampled").and_then(JsonValue::as_u64).unwrap_or(0);
        let label = if samples > 0 {
            format!("avg score ({} samples)", samples)
        } else {
            "avg score".to_string()
        };
        let _ = writeln!(out, "  {}: {}", label, format_percent(avg, 0));
    }
    if let Some(ratio) = obj.get("at_risk_ratio").and_then(JsonValue::as_f64) {
        let _ = writeln!(out, "  At-risk ratio: {}", format_percent(ratio, 0));
    }
    if let Some(levels) = obj.get("levels").and_then(JsonValue::as_array) {
        if !levels.is_empty() {
            let entries: Vec<String> = levels
                .iter()
                .filter_map(|level| {
                    let name = level.get("level").and_then(JsonValue::as_str)?;
                    let count = level.get("count").and_then(JsonValue::as_u64).unwrap_or(0);
                    Some(format!("{} {}", name, format_count_label(count, "sample")))
                })
                .collect();
            if !entries.is_empty() {
                let _ = writeln!(out, "  Level distribution: {}", entries.join(", "));
            }
        }
    }
    if let Some(slots) = obj.get("top_slots").and_then(JsonValue::as_array) {
        if !slots.is_empty() {
            let _ = writeln!(out, "  Top slot gaps (avg / max):");
            for entry in slots.iter().take(3) {
                let slot = entry
                    .get("slot")
                    .and_then(JsonValue::as_str)
                    .map(format_slot_name)
                    .unwrap_or_else(|| "unknown".to_string());
                let avg = percent_or_dash(entry.get("avg_gap").and_then(JsonValue::as_f64), 0);
                let max = percent_or_dash(entry.get("max_gap").and_then(JsonValue::as_f64), 0);
                let samples = entry
                    .get("samples")
                    .and_then(JsonValue::as_u64)
                    .unwrap_or(0);
                let _ = writeln!(
                    out,
                    "    - {} · avg {} · max {} · {}",
                    slot,
                    avg,
                    max,
                    format_count_label(samples, "sample")
                );
            }
        }
    }
}

fn summarize_working_set_section(out: &mut String, assembled: Option<&JsonValue>) {
    let _ = writeln!(out, "Working set:");
    let Some(obj) = assembled.and_then(JsonValue::as_object) else {
        let _ = writeln!(out, "  (no assembled snapshot)");
        return;
    };

    if let Some(scope) = render_scope(obj.get("project"), obj.get("query")) {
        let _ = writeln!(out, "  Scope: {}", scope);
    }

    if let Some(working) = obj.get("working_set").and_then(JsonValue::as_object) {
        if let Some(counts) = working.get("counts").and_then(JsonValue::as_object) {
            let items = counts.get("items").and_then(JsonValue::as_u64).unwrap_or(0);
            let seeds = counts.get("seeds").and_then(JsonValue::as_u64).unwrap_or(0);
            let expanded = counts
                .get("expanded")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0);
            let _ = writeln!(
                out,
                "  Counts: items {} · seeds {} · expanded {}",
                items, seeds, expanded
            );
        }
        if let Some(spec) = working
            .get("final_spec")
            .or_else(|| obj.get("spec"))
            .and_then(JsonValue::as_object)
        {
            if let Some(lanes) = spec.get("lanes").and_then(JsonValue::as_array) {
                let labels: Vec<String> = lanes
                    .iter()
                    .filter_map(JsonValue::as_str)
                    .map(clean_text)
                    .collect();
                if !labels.is_empty() {
                    let _ = writeln!(out, "  Lanes: {}", labels.join(", "));
                }
            }
            if let Some(slots) = spec.get("slot_budgets").and_then(JsonValue::as_object) {
                let mut entries: Vec<String> = slots
                    .iter()
                    .filter_map(|(slot, value)| value.as_u64().map(|budget| (slot, budget)))
                    .map(|(slot, budget)| format!("{}≤{}", format_slot_name(slot), budget))
                    .collect();
                if !entries.is_empty() {
                    entries.sort();
                    let _ = writeln!(out, "  Slot budgets: {}", entries.join(", "));
                }
            }
        }
    } else {
        let _ = writeln!(out, "  (working set summary unavailable)");
    }
}

fn render_scope(project: Option<&JsonValue>, query: Option<&JsonValue>) -> Option<String> {
    let project = project.and_then(JsonValue::as_str).map(clean_text);
    let query = query.and_then(JsonValue::as_str).map(clean_text);
    match (project, query) {
        (Some(p), Some(q)) if !p.is_empty() && !q.is_empty() => {
            Some(format!("project {} · query {}", p, q))
        }
        (Some(p), _) if !p.is_empty() => Some(format!("project {}", p)),
        (_, Some(q)) if !q.is_empty() => Some(format!("query {}", q)),
        _ => None,
    }
}

fn format_coverage_reason(reason: &str) -> String {
    if let Some(slot) = reason.strip_prefix("slot_underfilled:") {
        format!("Slot underfilled · {}", format_slot_name(slot))
    } else {
        clean_text(&reason.replace('_', " "))
    }
}

fn format_slot_name(slot: &str) -> String {
    clean_text(&slot.replace(['_', '-'], " "))
}

fn format_percent(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    let clamped = value.clamp(0.0, 1.0);
    format!("{:.*}%", digits, clamped * 100.0)
}

fn percent_or_dash(value: Option<f64>, digits: usize) -> String {
    match value {
        Some(v) if v.is_finite() => format_percent(v, digits),
        _ => "—".to_string(),
    }
}

fn format_count_label(count: u64, singular: &str) -> String {
    if count == 1 {
        format!("1 {}", singular)
    } else {
        format!("{} {}s", count, singular)
    }
}

fn clean_text(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    cleaned.trim().to_string()
}

fn cmd_state_actions(args: &StateActionsArgs) -> Result<()> {
    if args.watch && args.json {
        anyhow::bail!("--watch cannot be combined with --json output");
    }

    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let filters = ActionFilters::from_args(args)?;

    let mut full_snapshot = fetch_full_actions(&client, base, token.as_deref())?;
    let view = build_filtered_actions_view(&full_snapshot, &filters)?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&view).unwrap_or_else(|_| view.to_string())
            );
        } else {
            println!("{}", view);
        }
        return Ok(());
    }

    render_actions_text(&view, args, None)?;

    if args.watch {
        eprintln!("watching actions; press Ctrl-C to exit");
        watch_actions(base, token.as_deref(), &filters, args, &mut full_snapshot)?;
    }

    Ok(())
}

fn cmd_events_observations(args: &EventsObservationsArgs) -> Result<()> {
    if args.watch && args.json {
        anyhow::bail!("--watch cannot be combined with --json output");
    }

    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let since_resolution = resolve_since_param(args)?;
    let filters = ObservationFilters::from_args(args, &since_resolution)?;

    let mut full_snapshot = fetch_full_observations(&client, base, token.as_deref())?;
    let view = build_filtered_observations_view(&full_snapshot, &filters)?;

    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&view).unwrap_or_else(|_| view.to_string())
            );
        } else {
            println!("{}", view);
        }
        return Ok(());
    }

    render_observations_text(&view, args, &since_resolution, None)?;

    if args.watch {
        eprintln!("watching observations; press Ctrl-C to exit");
        watch_observations(
            base,
            token.as_deref(),
            &filters,
            args,
            &since_resolution,
            &mut full_snapshot,
        )?;
    }

    Ok(())
}

#[derive(Clone)]
struct ObservationFilters {
    limit: Option<usize>,
    kind_prefix: Option<String>,
    since_cutoff: Option<DateTime<Utc>>,
}

impl ObservationFilters {
    fn from_args(args: &EventsObservationsArgs, since: &SinceResolution) -> Result<Self> {
        let kind_prefix = args
            .kind_prefix
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let since_cutoff = match since.query {
            Some(ref iso) => {
                let parsed = DateTime::parse_from_rfc3339(iso)
                    .with_context(|| format!("failed to parse since='{}'", iso))?;
                Some(parsed.with_timezone(&Utc))
            }
            None => None,
        };
        Ok(Self {
            limit: args.limit,
            kind_prefix,
            since_cutoff,
        })
    }
}

fn fetch_full_observations(client: &Client, base: &str, token: Option<&str>) -> Result<JsonValue> {
    let url = format!("{}/state/observations", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing observations response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn build_filtered_observations_view(
    snapshot: &JsonValue,
    filters: &ObservationFilters,
) -> Result<JsonValue> {
    let version = snapshot
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let items = snapshot
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut selected: Vec<JsonValue> = Vec::new();
    for item in items.iter().rev() {
        if let Some(prefix) = filters.kind_prefix.as_deref() {
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !kind.starts_with(prefix) {
                continue;
            }
        }
        if let Some(cutoff) = filters.since_cutoff {
            if let Some(time_raw) = item.get("time").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(time_raw) {
                    if ts.with_timezone(&Utc) <= cutoff {
                        continue;
                    }
                }
            }
        }
        selected.push(item.clone());
        if let Some(limit) = filters.limit {
            if selected.len() >= limit {
                break;
            }
        }
    }
    selected.reverse();

    Ok(json!({
        "version": version,
        "items": selected,
    }))
}

fn render_observations_text(
    body: &JsonValue,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
    update_note: Option<&str>,
) -> Result<()> {
    let version = body.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(note) = update_note {
        println!();
        println!(
            "[{}] Observations update ({} items, version {})",
            note,
            items.len(),
            version
        );
    } else {
        println!(
            "Observations snapshot ({} items, version {})",
            items.len(),
            version
        );
        let mut filters: Vec<String> = Vec::new();
        if let Some(ref prefix) = args.kind_prefix {
            if !prefix.trim().is_empty() {
                filters.push(format!("prefix={}", prefix.trim()));
            }
        }
        if let Some(ref label) = since_resolution.relative_display {
            filters.push(label.clone());
        }
        if let Some(ref label) = since_resolution.display {
            filters.push(label.clone());
        }
        if !filters.is_empty() {
            println!("Filters: {}", filters.join(", "));
        }
    }

    if items.is_empty() {
        if update_note.is_some() {
            println!("(no observations matched filters)");
        }
        return Ok(());
    }

    for item in items {
        let time_raw = item.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let when = if time_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(time_raw)
        };
        let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("-");
        let kind_display = ellipsize_str(kind, 36);

        let payload_display = if args.payload_width == 0 {
            "-".to_string()
        } else if let Some(payload) = item.get("payload") {
            format_payload_snippet(payload, args.payload_width)
        } else {
            "-".to_string()
        };

        let mut extras: Vec<String> = Vec::new();
        if args.show_policy {
            if let Some(policy) = item.get("policy") {
                let snippet = format_payload_snippet(policy, args.payload_width.max(48));
                if snippet != "-" {
                    extras.push(format!("policy={}", snippet));
                }
            }
            if let Some(ce) = item.get("ce") {
                let snippet = format_payload_snippet(ce, args.payload_width.max(48));
                if snippet != "-" {
                    extras.push(format!("ce={}", snippet));
                }
            }
        }
        let extra_str = if extras.is_empty() {
            String::new()
        } else {
            format!(" {}", extras.join(" "))
        };

        println!(
            "{:<28} {:<36} {}{}",
            when, kind_display, payload_display, extra_str
        );
    }

    io::stdout().flush().ok();
    Ok(())
}

fn watch_observations(
    base: &str,
    token: Option<&str>,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
    snapshot: &mut JsonValue,
) -> Result<()> {
    let mut last_event_id: Option<String> = None;
    let mut backoff_secs = 1u64;
    loop {
        match stream_observations_once(
            base,
            token,
            last_event_id.as_deref(),
            snapshot,
            filters,
            args,
            since_resolution,
        ) {
            Ok(next_id) => {
                if let Some(id) = next_id {
                    last_event_id = Some(id);
                }
                backoff_secs = 1;
            }
            Err(err) => {
                eprintln!("watch error: {err:?}");
                backoff_secs = (backoff_secs * 2).min(30);
            }
        }
        thread::sleep(Duration::from_secs(backoff_secs));
    }
}

fn stream_observations_once(
    base: &str,
    token: Option<&str>,
    last_event_id: Option<&str>,
    snapshot: &mut JsonValue,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
) -> Result<Option<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .context("building SSE client")?;

    let mut req = client
        .get(&format!("{}/events", base))
        .query(&[("prefix", "state.read.model.patch"), ("replay", "0")])
        .header(ACCEPT, "text/event-stream");
    if let Some(id) = last_event_id {
        req = req.header("Last-Event-ID", id);
    }
    req = with_admin_headers(req, token);

    let resp = req.send().context("connecting to /events stream")?;
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("events stream failed with status {}", status);
    }

    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut event_name = String::new();
    let mut data_buf = String::new();
    let mut event_id_line: Option<String> = None;
    let mut latest_id = last_event_id.map(|s| s.to_string());

    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(latest_id);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if line.is_empty() {
            if event_name == "state.read.model.patch" && !data_buf.is_empty() {
                if let Err(err) =
                    handle_observations_patch(&data_buf, snapshot, filters, args, since_resolution)
                {
                    eprintln!("failed to process patch: {err:?}");
                } else if let Some(id_val) = event_id_line.as_ref() {
                    latest_id = Some(id_val.clone());
                }
            }
            event_name.clear();
            data_buf.clear();
            event_id_line = None;
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = line.strip_prefix("id:") {
            event_id_line = Some(rest.trim().to_string());
            continue;
        }
    }
}

fn handle_observations_patch(
    data: &str,
    snapshot: &mut JsonValue,
    filters: &ObservationFilters,
    args: &EventsObservationsArgs,
    since_resolution: &SinceResolution,
) -> Result<()> {
    let env: JsonValue = serde_json::from_str(data).context("decoding SSE payload")?;
    let payload = env.get("payload").cloned().unwrap_or(env.clone());
    let rm = payload.get("payload").cloned().unwrap_or(payload.clone());
    let read_model_id = rm
        .get("id")
        .or_else(|| rm.get("read_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if read_model_id != "observations" {
        return Ok(());
    }
    let patch_value = match rm.get("patch") {
        Some(v) if v.is_array() => v.clone(),
        _ => return Ok(()),
    };
    let patch: JsonPatch =
        serde_json::from_value(patch_value).context("decoding JSON Patch for observations")?;
    apply_json_patch(snapshot, &patch).context("applying observations patch")?;
    let view = build_filtered_observations_view(snapshot, filters)?;
    let version = view.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let note = format!("{} (version {})", Local::now().format("%H:%M:%S"), version);
    render_observations_text(&view, args, since_resolution, Some(&note))?;
    Ok(())
}

#[derive(Clone)]
struct ActionFilters {
    limit: Option<usize>,
    state: Option<String>,
    kind_prefix: Option<String>,
    updated_since: Option<DateTime<Utc>>,
}

impl ActionFilters {
    fn from_args(args: &StateActionsArgs) -> Result<Self> {
        let state = args
            .state
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let kind_prefix = args
            .kind_prefix
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let updated_since = if let Some(ref raw) = args.updated_since {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                anyhow::bail!("--updated-since cannot be empty");
            }
            let parsed = DateTime::parse_from_rfc3339(trimmed)
                .with_context(|| format!("failed to parse updated_since='{}'", trimmed))?;
            Some(parsed.with_timezone(&Utc))
        } else {
            None
        };
        Ok(Self {
            limit: args.limit.map(|v| v.clamp(1, 2000)),
            state,
            kind_prefix,
            updated_since,
        })
    }
}

fn fetch_full_actions(client: &Client, base: &str, token: Option<&str>) -> Result<JsonValue> {
    let url = format!("{}/state/actions", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing actions response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn build_filtered_actions_view(snapshot: &JsonValue, filters: &ActionFilters) -> Result<JsonValue> {
    let version = snapshot
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let items = snapshot
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut selected: Vec<JsonValue> = Vec::new();
    for item in items.iter() {
        if let Some(state) = filters.state.as_deref() {
            let current_state = item.get("state").and_then(|v| v.as_str()).unwrap_or("");
            if !current_state.eq_ignore_ascii_case(state) {
                continue;
            }
        }
        if let Some(prefix) = filters.kind_prefix.as_deref() {
            let kind = item.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !kind.starts_with(prefix) {
                continue;
            }
        }
        if let Some(cutoff) = filters.updated_since {
            if let Some(updated_raw) = item.get("updated").and_then(|v| v.as_str()) {
                if let Ok(ts) = DateTime::parse_from_rfc3339(updated_raw) {
                    if ts.with_timezone(&Utc) <= cutoff {
                        continue;
                    }
                }
            }
        }
        selected.push(item.clone());
        if let Some(limit) = filters.limit {
            if selected.len() >= limit {
                break;
            }
        }
    }

    Ok(json!({
        "version": version,
        "items": selected,
    }))
}

fn render_actions_text(
    body: &JsonValue,
    args: &StateActionsArgs,
    update_note: Option<&str>,
) -> Result<()> {
    let version = body.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(note) = update_note {
        println!();
        println!(
            "[{}] Actions update ({} items, version {})",
            note,
            items.len(),
            version
        );
    } else {
        println!(
            "Actions snapshot ({} items, version {})",
            items.len(),
            version
        );
    }

    if items.is_empty() {
        println!("(no actions matched the filters)");
        return Ok(());
    }

    let kind_width = args.kind_width.max(8);
    println!(
        "{:<28} {:<10} {:<width$} Id",
        "Updated",
        "State",
        "Kind",
        width = kind_width
    );

    for item in items {
        let updated_raw = item.get("updated").and_then(|v| v.as_str()).unwrap_or("");
        let updated_display = if updated_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(updated_raw)
        };
        let state_display = item
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string();
        let kind_display = item
            .get("kind")
            .and_then(|v| v.as_str())
            .map(|k| ellipsize_str(k, kind_width))
            .unwrap_or_else(|| "-".to_string());
        let id_display = item.get("id").and_then(|v| v.as_str()).unwrap_or("-");

        println!(
            "{:<28} {:<10} {:<width$} {}",
            updated_display,
            state_display,
            kind_display,
            id_display,
            width = kind_width
        );
    }

    io::stdout().flush().ok();
    Ok(())
}

fn watch_actions(
    base: &str,
    token: Option<&str>,
    filters: &ActionFilters,
    args: &StateActionsArgs,
    snapshot: &mut JsonValue,
) -> Result<()> {
    let mut last_event_id: Option<String> = None;
    let mut backoff_secs = 1u64;
    loop {
        match stream_actions_once(
            base,
            token,
            last_event_id.as_deref(),
            snapshot,
            filters,
            args,
        ) {
            Ok(next_id) => {
                if let Some(id) = next_id {
                    last_event_id = Some(id);
                }
                backoff_secs = 1;
            }
            Err(err) => {
                eprintln!("watch error: {err:?}");
                backoff_secs = (backoff_secs * 2).min(30);
            }
        }
        thread::sleep(Duration::from_secs(backoff_secs));
    }
}

fn stream_actions_once(
    base: &str,
    token: Option<&str>,
    last_event_id: Option<&str>,
    snapshot: &mut JsonValue,
    filters: &ActionFilters,
    args: &StateActionsArgs,
) -> Result<Option<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .context("building SSE client")?;

    let mut req = client
        .get(&format!("{}/events", base))
        .query(&[("prefix", "state.read.model.patch"), ("replay", "0")])
        .header(ACCEPT, "text/event-stream");
    if let Some(id) = last_event_id {
        req = req.header("Last-Event-ID", id);
    }
    req = with_admin_headers(req, token);

    let resp = req.send().context("connecting to /events stream")?;
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("events stream failed with status {}", status);
    }

    let mut reader = BufReader::new(resp);
    let mut line = String::new();
    let mut event_name = String::new();
    let mut data_buf = String::new();
    let mut event_id_line: Option<String> = None;
    let mut latest_id = last_event_id.map(|s| s.to_string());

    loop {
        line.clear();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(latest_id);
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if line.is_empty() {
            if event_name == "state.read.model.patch" && !data_buf.is_empty() {
                if let Err(err) = handle_actions_patch(&data_buf, snapshot, filters, args) {
                    eprintln!("failed to process patch: {err:?}");
                } else if let Some(id_val) = event_id_line.as_ref() {
                    latest_id = Some(id_val.clone());
                }
            }
            event_name.clear();
            data_buf.clear();
            event_id_line = None;
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event_name = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(rest.trim_start());
            continue;
        }
        if let Some(rest) = line.strip_prefix("id:") {
            event_id_line = Some(rest.trim().to_string());
            continue;
        }
    }
}

fn handle_actions_patch(
    data: &str,
    snapshot: &mut JsonValue,
    filters: &ActionFilters,
    args: &StateActionsArgs,
) -> Result<()> {
    let env: JsonValue = serde_json::from_str(data).context("decoding SSE payload")?;
    let payload = env.get("payload").cloned().unwrap_or(env.clone());
    let rm = payload.get("payload").cloned().unwrap_or(payload.clone());
    let read_model_id = rm
        .get("id")
        .or_else(|| rm.get("read_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if read_model_id != "actions" {
        return Ok(());
    }
    let patch_value = match rm.get("patch") {
        Some(v) if v.is_array() => v.clone(),
        _ => return Ok(()),
    };
    let patch: JsonPatch =
        serde_json::from_value(patch_value).context("decoding JSON Patch for actions")?;
    apply_json_patch(snapshot, &patch).context("applying actions patch")?;
    let view = build_filtered_actions_view(snapshot, filters)?;
    let version = view.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    let note = format!("{} (version {})", Local::now().format("%H:%M:%S"), version);
    render_actions_text(&view, args, Some(&note))?;
    Ok(())
}

struct SinceResolution {
    query: Option<String>,
    display: Option<String>,
    relative_display: Option<String>,
}

fn resolve_since_param(args: &EventsObservationsArgs) -> Result<SinceResolution> {
    if let Some(ref raw) = args.since_relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--since-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        let ts = (Utc::now() - duration).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        return Ok(SinceResolution {
            query: Some(ts.clone()),
            display: Some(format!("since>{}", ts)),
            relative_display: Some(format!("since_relative={}", trimmed)),
        });
    }

    if let Some(ref raw) = args.since {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--since cannot be empty");
        }
        return Ok(SinceResolution {
            query: Some(trimmed.to_string()),
            display: Some(format!("since>{}", trimmed)),
            relative_display: None,
        });
    }

    Ok(SinceResolution {
        query: None,
        display: None,
        relative_display: None,
    })
}

fn parse_relative_duration(input: &str) -> Result<chrono::Duration> {
    let mut total_seconds: i64 = 0;
    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_ascii_digit() {
            current.push(ch);
            continue;
        }
        if current.is_empty() {
            anyhow::bail!("expected digits before unit in '{}'", input);
        }
        let value: i64 = current
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid number in '{}'", input))?;
        current.clear();
        let unit = ch.to_ascii_lowercase();
        let component = match unit {
            's' => value,
            'm' => value
                .checked_mul(60)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            'h' => value
                .checked_mul(3600)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            'd' => value
                .checked_mul(86400)
                .ok_or_else(|| anyhow::anyhow!("duration overflow"))?,
            _ => anyhow::bail!("unsupported unit '{}' in '{}'", ch, input),
        };
        total_seconds = total_seconds
            .checked_add(component)
            .ok_or_else(|| anyhow::anyhow!("duration overflow"))?;
    }

    if !current.is_empty() {
        anyhow::bail!("missing unit after '{}' in '{}'", current, input);
    }
    if total_seconds <= 0 {
        anyhow::bail!("relative duration must be greater than zero");
    }
    Ok(chrono::Duration::seconds(total_seconds))
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
        let severity = status
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("info");
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
            "- {}: {} (severity {}) — runtimes total {}{}{}",
            node, label, severity, total, states_fragment, detail_fragment
        ));
    }

    lines.join("\n")
}

fn render_tool_cache_summary(stats: &ToolCacheSnapshot, base: &str) -> String {
    let mut buf = String::new();
    let _ = writeln!(buf, "Tool cache @ {}", base);
    if stats.capacity == 0 {
        let _ = writeln!(
            buf,
            "- status: disabled | capacity 0 | ttl {}s | entries {}",
            stats.ttl_secs, stats.entries
        );
    } else {
        let _ = writeln!(
            buf,
            "- status: enabled | capacity {} | ttl {}s | entries {}",
            stats.capacity, stats.ttl_secs, stats.entries
        );
    }

    let mut outcomes = format!(
        "- outcomes: hit {} | miss {} | coalesced {} | bypass {} | errors {}",
        stats.hit, stats.miss, stats.coalesced, stats.bypass, stats.errors
    );
    let total = stats.hit + stats.miss;
    if total > 0 {
        let hit_rate = stats.hit as f64 / total as f64 * 100.0;
        let suppression = stats.stampede_suppression_rate * 100.0;
        outcomes.push_str(&format!(
            " (hit {:.1}%, suppression {:.1}%)",
            hit_rate, suppression
        ));
    }
    let _ = writeln!(buf, "{}", outcomes);

    if stats.latency_saved_samples > 0 {
        let mut line = format!(
            "- latency saved: avg {:.1} ms (samples {}, total {})",
            stats.avg_latency_saved_ms,
            stats.latency_saved_samples,
            format_duration_ms(stats.latency_saved_ms_total)
        );
        if let Some(last) = stats.last_latency_saved_ms {
            line.push_str(&format!(", last {} ms", last));
        }
        let _ = writeln!(buf, "{}", line);
    }

    if stats.payload_saved_samples > 0 {
        let mut line = format!(
            "- payload saved: avg {} (samples {}, total {})",
            format_bytes_f64(stats.avg_payload_bytes_saved),
            stats.payload_saved_samples,
            format_bytes(stats.payload_bytes_saved_total)
        );
        if let Some(last) = stats.last_payload_bytes {
            line.push_str(&format!(", last {}", format_bytes(last)));
        }
        let _ = writeln!(buf, "{}", line);
    }

    if stats.hit_age_samples > 0 {
        let mut line = format!(
            "- hit age: avg {} (samples {})",
            format_seconds_f64(stats.avg_hit_age_secs),
            stats.hit_age_samples
        );
        if let Some(last) = stats.last_hit_age_secs {
            line.push_str(&format!(", last {}", format_seconds(last)));
        }
        if let Some(max) = stats.max_hit_age_secs {
            line.push_str(&format!(", max {}", format_seconds(max)));
        }
        let _ = writeln!(buf, "{}", line);
    }

    buf.trim_end().to_string()
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

fn format_observation_timestamp(raw: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S%.3f %Z")
            .to_string(),
        Err(_) => raw.to_string(),
    }
}

fn format_payload_snippet(value: &JsonValue, width: usize) -> String {
    if width == 0 {
        return "-".to_string();
    }
    let raw = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    let cleaned = raw.replace(['\n', '\r'], " ");
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        "-".to_string()
    } else {
        ellipsize_str(trimmed, width)
    }
}

fn ellipsize_str(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut chars = input.chars();
    let mut collected: Vec<char> = Vec::new();
    while let Some(ch) = chars.next() {
        collected.push(ch);
        if collected.len() == max_chars {
            if !chars.as_str().is_empty() {
                if max_chars == 1 {
                    return "…".to_string();
                }
                collected.pop();
                collected.push('…');
            }
            return collected.iter().collect();
        }
    }
    collected.iter().collect()
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

fn encode_hex(bytes: &[u8], uppercase: bool) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        if uppercase {
            let _ = write!(out, "{:02X}", byte);
        } else {
            let _ = write!(out, "{:02x}", byte);
        }
    }
    out
}

fn hash_admin_token(token: &str) -> String {
    encode_hex(&sha2::Sha256::digest(token.as_bytes()), false)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

fn format_bytes_f64(bytes: f64) -> String {
    if bytes <= 0.0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    if unit_index == 0 {
        format!("{:.0} {}", value.round(), UNITS[unit_index])
    } else {
        format!("{:.1} {}", value, UNITS[unit_index])
    }
}

fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{} ms", ms);
    }
    if ms < 60_000 {
        return format!("{:.1} s", ms as f64 / 1_000.0);
    }
    let total_secs = ms / 1_000;
    if total_secs < 3_600 {
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        return format!("{}m{:02}s", minutes, seconds);
    }
    let hours = total_secs / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    let seconds = total_secs % 60;
    format!("{}h{:02}m{:02}s", hours, minutes, seconds)
}

fn format_seconds(secs: u64) -> String {
    if secs < 60 {
        format!("{} s", secs)
    } else if secs < 3_600 {
        let minutes = secs / 60;
        let seconds = secs % 60;
        format!("{}m{:02}s", minutes, seconds)
    } else {
        let hours = secs / 3_600;
        let minutes = (secs % 3_600) / 60;
        let seconds = secs % 60;
        format!("{}h{:02}m{:02}s", hours, minutes, seconds)
    }
}

fn format_seconds_f64(secs: f64) -> String {
    if secs < 1.0 {
        return format!("{:.0} ms", secs * 1_000.0);
    }
    if secs < 60.0 {
        return format!("{:.1} s", secs);
    }
    let total_secs = secs.floor() as u64;
    let remainder = secs - total_secs as f64;
    let base = format_seconds(total_secs);
    if remainder >= 0.5 {
        // include ~0.5s remainder to avoid discarding observable fractional seconds
        format!("{} (~{:.1}s)", base, secs)
    } else {
        base
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
        let auth_value = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t))
            .context("invalid bearer token for Authorization header")?;
        headers.insert(reqwest::header::AUTHORIZATION, auth_value);
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

fn cmd_smoke_triad(args: &SmokeTriadArgs) -> Result<()> {
    let port = args.common.port.unwrap_or(18181);
    let admin_token = args
        .admin_token
        .clone()
        .unwrap_or_else(|| "triad-smoke-token".to_string());
    let server_bin = ensure_server_binary(args.common.server_bin.as_deref())?;
    let mut server = spawn_server(&server_bin, port, Some(&admin_token), Vec::new())?;
    server.set_keep_temp(args.common.keep_temp);

    let base = format!("http://127.0.0.1:{port}");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build HTTP client")?;

    let log_path = server.log_path().to_path_buf();
    wait_for_health(
        &client,
        &base,
        server.child_mut(),
        &log_path,
        Duration::from_secs(args.common.wait_timeout_secs),
    )?;

    let action_id = submit_echo_action(&client, &base, Some(&admin_token), "triad-smoke")?;
    let status_doc = wait_for_action(
        &client,
        &base,
        Some(&admin_token),
        &action_id,
        Duration::from_secs(20),
    )?;
    validate_echo_payload(&status_doc)?;
    ensure_projects_snapshot(&client, &base, Some(&admin_token))?;
    ensure_sse_connected(&client, &base, Some(&admin_token), None)?;

    println!("triad smoke OK");
    if args.common.keep_temp {
        println!(
            "Temporary state preserved at {}",
            server.state_path().display()
        );
        println!("Server log: {}", server.log_path().display());
        server.persist();
    }
    Ok(())
}

fn cmd_smoke_context(args: &SmokeContextArgs) -> Result<()> {
    let port = args.common.port.unwrap_or(18182);
    let admin_token = args
        .admin_token
        .clone()
        .unwrap_or_else(|| "context-ci-token".to_string());
    let token_sha = format!("{:x}", sha2::Sha256::digest(admin_token.as_bytes()));
    let extra_env = vec![
        ("ARW_ADMIN_TOKEN_SHA256".to_string(), token_sha),
        ("ARW_CONTEXT_CI_TOKEN".to_string(), admin_token.clone()),
    ];

    let server_bin = ensure_server_binary(args.common.server_bin.as_deref())?;
    let mut server = spawn_server(&server_bin, port, Some(&admin_token), extra_env)?;
    server.set_keep_temp(args.common.keep_temp);

    let base = format!("http://127.0.0.1:{port}");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("build HTTP client")?;

    let log_path = server.log_path().to_path_buf();
    wait_for_health(
        &client,
        &base,
        server.child_mut(),
        &log_path,
        Duration::from_secs(args.common.wait_timeout_secs),
    )?;

    let mut action_ids = Vec::new();
    for idx in 0..2 {
        let msg = format!("context-ci-{idx}");
        let action_id = submit_echo_action(&client, &base, Some(&admin_token), &msg)?;
        action_ids.push(action_id);
    }
    for action_id in &action_ids {
        let _ = wait_for_action(
            &client,
            &base,
            Some(&admin_token),
            action_id,
            Duration::from_secs(20),
        )?;
    }

    ensure_context_telemetry(&client, &base, Some(&admin_token))?;
    println!("context telemetry smoke OK");
    if args.common.keep_temp {
        println!(
            "Temporary state preserved at {}",
            server.state_path().display()
        );
        println!("Server log: {}", server.log_path().display());
        server.persist();
    }
    Ok(())
}

struct ServerHandle {
    child: Child,
    state_dir: Option<TempDir>,
    state_path: PathBuf,
    log_path: PathBuf,
    keep_temp: bool,
}

impl ServerHandle {
    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn log_path(&self) -> &Path {
        &self.log_path
    }

    fn state_path(&self) -> &Path {
        &self.state_path
    }

    fn set_keep_temp(&mut self, keep: bool) {
        self.keep_temp = keep;
    }

    fn persist(&mut self) {
        self.keep_temp = true;
        if let Some(dir) = self.state_dir.take() {
            let _ = dir.keep();
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            // child already exited
        } else {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }

        if !self.keep_temp {
            // TempDir drop removes the directory when still present
        }
    }
}

fn spawn_server(
    server_bin: &Path,
    port: u16,
    admin_token: Option<&str>,
    extra_env: Vec<(String, String)>,
) -> Result<ServerHandle> {
    let state_dir = TempDir::new().context("create temporary state directory")?;
    let state_path = state_dir.path().to_path_buf();
    let log_path = state_path.join("arw-server.log");
    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .context("create server log file")?;
    let stdout = log_file
        .try_clone()
        .context("clone log handle for stdout")?;
    let stderr = log_file
        .try_clone()
        .context("clone log handle for stderr")?;

    let mut cmd = Command::new(server_bin);
    cmd.env("ARW_PORT", port.to_string());
    cmd.env("ARW_STATE_DIR", &state_path);
    cmd.env("ARW_DEBUG", "0");
    if let Some(token) = admin_token {
        cmd.env("ARW_ADMIN_TOKEN", token);
    }
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::from(stdout));
    cmd.stderr(Stdio::from(stderr));

    let child = cmd
        .spawn()
        .with_context(|| format!("failed to launch {}", server_bin.display()))?;

    Ok(ServerHandle {
        child,
        state_dir: Some(state_dir),
        state_path,
        log_path,
        keep_temp: false,
    })
}

fn wait_for_health(
    client: &Client,
    base: &str,
    child: &mut Child,
    log_path: &Path,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let url = format!("{}/healthz", base);
    while Instant::now() < deadline {
        if let Ok(resp) = client.get(&url).send() {
            if resp.status().is_success() {
                return Ok(());
            }
        }

        if let Some(status) = child.try_wait().context("check server health status")? {
            let log = read_log_tail(log_path, 8192);
            bail!("arw-server exited before health check (status {status:?})\n{log}");
        }

        std::thread::sleep(Duration::from_millis(400));
    }

    let log = read_log_tail(log_path, 8192);
    bail!("timed out waiting for {url}\n{log}");
}

fn submit_echo_action(
    client: &Client,
    base: &str,
    admin_token: Option<&str>,
    message: &str,
) -> Result<String> {
    let payload = json!({
        "kind": "demo.echo",
        "input": { "msg": message }
    });

    let mut request = client.post(format!("{}/actions", base));
    if let Some(token) = admin_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .json(&payload)
        .send()
        .context("submit action")?
        .error_for_status()
        .context("submit action status")?;

    let body: JsonValue = response
        .json()
        .context("parse action submission response")?;
    if let Some(id) = body.get("id").and_then(|v| v.as_str()) {
        return Ok(id.to_string());
    }
    if let Some(action) = body.get("action").and_then(|v| v.as_object()) {
        if let Some(id) = action.get("id").and_then(|v| v.as_str()) {
            return Ok(id.to_string());
        }
    }
    bail!("action submission response missing id: {}", body);
}

fn wait_for_action(
    client: &Client,
    base: &str,
    admin_token: Option<&str>,
    action_id: &str,
    timeout: Duration,
) -> Result<JsonValue> {
    let deadline = Instant::now() + timeout;
    let url = format!("{}/actions/{}", base, action_id);
    loop {
        let mut request = client.get(&url);
        if let Some(token) = admin_token {
            request = request.bearer_auth(token);
        }
        match request.send() {
            Ok(resp) => {
                if resp.status() == StatusCode::NOT_FOUND {
                    if Instant::now() >= deadline {
                        bail!("action {action_id} not found before timeout");
                    }
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
                let resp = resp.error_for_status().context("action status request")?;
                let doc: JsonValue = resp.json().context("parse action status response")?;
                match doc.get("state").and_then(|v| v.as_str()).unwrap_or("") {
                    "completed" => return Ok(doc),
                    "queued" | "running" => {
                        if Instant::now() >= deadline {
                            bail!(
                                "action {action_id} did not complete in time (last state {})",
                                doc.get("state")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown"),
                            );
                        }
                        std::thread::sleep(Duration::from_millis(400));
                    }
                    other => bail!("unexpected action state {other}: {doc}"),
                }
            }
            Err(err) => {
                if Instant::now() >= deadline {
                    bail!("failed to fetch action {action_id} status: {err}");
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    }
}

fn validate_echo_payload(doc: &JsonValue) -> Result<()> {
    let state = doc.get("state").and_then(|v| v.as_str()).unwrap_or("");
    ensure!(state == "completed", "unexpected action state: {doc}");

    if let Some(created) = doc.get("created").and_then(|v| v.as_str()) {
        parse_timestamp(created).context("invalid action created timestamp")?;
    }

    let output = doc
        .get("output")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("action output missing: {doc}"))?;
    ensure!(
        output.contains_key("echo"),
        "action output missing echo payload"
    );
    Ok(())
}

fn ensure_projects_snapshot(client: &Client, base: &str, admin_token: Option<&str>) -> Result<()> {
    let mut request = client.get(format!("{}/state/projects", base));
    if let Some(token) = admin_token {
        request = request.bearer_auth(token);
    }
    let resp = request
        .send()
        .context("fetch /state/projects")?
        .error_for_status()
        .context("/state/projects status")?;
    let doc: JsonValue = resp.json().context("parse /state/projects response")?;
    let obj = doc
        .as_object()
        .ok_or_else(|| anyhow!("unexpected /state/projects payload: {doc}"))?;
    ensure!(
        obj.contains_key("generated"),
        "/state/projects missing generated timestamp"
    );
    ensure!(
        obj.contains_key("items"),
        "/state/projects missing items array"
    );
    Ok(())
}

fn ensure_sse_connected(
    client: &Client,
    base: &str,
    admin_token: Option<&str>,
    last_event_id: Option<&str>,
) -> Result<()> {
    let mut request = client
        .get(format!("{}/events?replay=1", base))
        .header(ACCEPT, "text/event-stream");
    if let Some(token) = admin_token {
        request = request.bearer_auth(token);
    }
    if let Some(id) = last_event_id {
        request = request.header("Last-Event-ID", id);
    }
    let response = request
        .timeout(Duration::from_secs(6))
        .send()
        .context("open SSE stream")?
        .error_for_status()
        .context("SSE handshake status")?;

    let mut reader = BufReader::new(response);
    let mut buf = String::new();
    for _ in 0..32 {
        buf.clear();
        let bytes = reader.read_line(&mut buf).context("read SSE line")?;
        if bytes == 0 {
            break;
        }
        if buf.contains("event: service.connected") {
            return Ok(());
        }
    }
    bail!("did not observe service.connected during SSE handshake");
}

fn ensure_context_telemetry(client: &Client, base: &str, admin_token: Option<&str>) -> Result<()> {
    let mut request = client.get(format!("{}/state/training/telemetry", base));
    if let Some(token) = admin_token {
        request = request.bearer_auth(token);
    }
    let resp = request
        .send()
        .context("fetch context telemetry")?
        .error_for_status()
        .context("context telemetry status")?;
    let doc: JsonValue = resp.json().context("parse context telemetry response")?;
    let obj = doc
        .as_object()
        .ok_or_else(|| anyhow!("context telemetry payload is not an object: {doc}"))?;

    let generated = obj
        .get("generated")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("context telemetry missing generated timestamp"))?;
    parse_timestamp(generated).context("invalid telemetry generated timestamp")?;

    let events = obj
        .get("events")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("context telemetry missing events section"))?;
    let total = events.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
    ensure!(
        total >= 2,
        "telemetry events total below expected threshold ({total})"
    );

    let routes = obj
        .get("routes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("context telemetry missing routes array"))?;
    ensure!(
        !routes.is_empty(),
        "context telemetry routes array is empty"
    );

    let bus = obj
        .get("bus")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("context telemetry missing bus metrics"))?;
    ensure!(
        bus.contains_key("published"),
        "context telemetry bus missing published metric"
    );

    let tools = obj
        .get("tools")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("context telemetry missing tools metrics"))?;
    let completed = tools.get("completed").and_then(|v| v.as_u64()).unwrap_or(0);
    ensure!(
        completed >= 2,
        "context telemetry tools completions below expected threshold ({completed})"
    );

    Ok(())
}

fn read_log_tail(log_path: &Path, max_bytes: usize) -> String {
    match std::fs::read_to_string(log_path) {
        Ok(contents) => {
            let tail = if contents.len() > max_bytes {
                let start = contents.len() - max_bytes;
                &contents[start..]
            } else {
                &contents
            };
            format!(
                "----- server log tail -----\n{}\n---------------------------",
                tail
            )
        }
        Err(err) => format!("(unable to read log {}: {})", log_path.display(), err),
    }
}

fn ensure_server_binary(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        bail!("specified server binary {:?} does not exist", path);
    }

    let exe_name = if cfg!(windows) {
        "arw-server.exe"
    } else {
        "arw-server"
    };
    let root = workspace_root()?;
    let candidates = [
        root.join("target").join("release").join(exe_name),
        root.join("target").join("debug").join(exe_name),
    ];

    for cand in &candidates {
        if cand.exists() {
            return Ok(cand.clone());
        }
    }

    build_server_binary(&root)?;

    for cand in &candidates {
        if cand.exists() {
            return Ok(cand.clone());
        }
    }

    bail!(
        "unable to locate arw-server binary; use --server-bin or run `cargo build -p arw-server`"
    );
}

fn workspace_root() -> Result<PathBuf> {
    let mut dir = env::current_dir().context("determine current directory")?;
    loop {
        if dir.join("Cargo.toml").is_file() {
            return Ok(dir);
        }
        if !dir.pop() {
            break;
        }
    }

    let mut exe = env::current_exe().context("locate current executable")?;
    while exe.pop() {
        if exe.join("Cargo.toml").is_file() {
            return Ok(exe);
        }
    }

    bail!("unable to locate workspace root; run from repository root or use --server-bin");
}

fn build_server_binary(root: &Path) -> Result<()> {
    let status = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("arw-server")
        .current_dir(root)
        .status()
        .context("invoke cargo build -p arw-server")?;
    if !status.success() {
        bail!("cargo build -p arw-server failed with status {status}");
    }
    Ok(())
}

fn parse_timestamp(raw: &str) -> Result<()> {
    let normalized = normalize_rfc3339(raw);
    chrono::DateTime::parse_from_rfc3339(&normalized)
        .map(|_| ())
        .map_err(|err| anyhow!("invalid timestamp {raw}: {err}"))
}

fn normalize_rfc3339(raw: &str) -> String {
    if raw.ends_with('Z') {
        raw.trim_end_matches('Z').to_string() + "+00:00"
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn parse_relative_duration_supports_composites() {
        let duration = parse_relative_duration("1h30m").expect("duration");
        assert_eq!(duration.num_seconds(), 5400);
    }

    #[test]
    fn parse_relative_duration_rejects_invalid_input() {
        assert!(parse_relative_duration("abc").is_err());
        assert!(parse_relative_duration("15").is_err());
        assert!(parse_relative_duration("0s").is_err());
    }

    #[test]
    fn combine_snapshots_includes_matrix_and_supervisor() {
        let supervisor = json!({"runtimes":[], "updated_at":"2025-10-02T05:30:00Z"});
        let matrix = json!({"ttl_seconds": 120, "items": {}});
        let combined = combine_runtime_snapshots(&supervisor, Some(matrix.clone()));
        assert_eq!(combined["supervisor"], supervisor);
        assert_eq!(combined["matrix"], matrix);
        assert_eq!(combined["matrix"]["ttl_seconds"].as_u64(), Some(120));
    }

    #[test]
    fn combine_snapshots_defaults_matrix_to_null() {
        let supervisor = json!({"runtimes": [json!({"descriptor": {"id": "rt"}})]});
        let combined = combine_runtime_snapshots(&supervisor, None);
        assert!(combined["matrix"].is_null());
        assert_eq!(combined["supervisor"], supervisor);
    }

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
        let tmp = TempDir::new()?;
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

    #[test]
    fn context_summary_renders_core_metrics() {
        let snapshot = json!({
            "generated_ms": 1_700_000_000_000u64,
            "context": {
                "coverage": {
                    "latest": {
                        "needs_more": true,
                        "reasons": ["slot_underfilled:seeds", "insufficient_evidence"],
                        "project": "alpha",
                        "query": "sprint review",
                        "summary": {
                            "slots": {
                                "counts": {"seeds": 2, "drafts": 1},
                                "budgets": {"seeds": 4, "drafts": 2}
                            }
                        }
                    },
                    "needs_more_ratio": 0.5,
                    "recent": [{}, {}],
                    "top_reasons": [
                        {"reason": "slot_underfilled:seeds", "count": 3}
                    ],
                    "top_slots": [
                        {"slot": "seeds", "count": 3},
                        {"slot": "drafts", "count": 1}
                    ]
                },
                "recall_risk": {
                    "latest": {
                        "level": "medium",
                        "score": 0.42,
                        "at_risk": true,
                        "components": {
                            "coverage_shortfall": 0.6,
                            "lane_gap": 0.0,
                            "slot_gap": 0.25,
                            "quality_gap": 0.05,
                            "slots": {"seeds": 0.7, "drafts": 0.2}
                        }
                    },
                    "avg_score": 0.33,
                    "at_risk_ratio": 0.4,
                    "sampled": 5,
                    "levels": [
                        {"level": "high", "count": 2},
                        {"level": "medium", "count": 3}
                    ],
                    "top_slots": [
                        {"slot": "seeds", "avg_gap": 0.7, "max_gap": 0.9, "samples": 3}
                    ]
                },
                "assembled": {
                    "project": "alpha",
                    "query": "sprint review",
                    "working_set": {
                        "counts": {"items": 8, "seeds": 3, "expanded": 9},
                        "final_spec": {
                            "lanes": ["research", "analysis"],
                            "slot_budgets": {"seeds": 4, "drafts": 2}
                        }
                    }
                }
            }
        });
        let summary = render_context_telemetry_summary(&snapshot, 1_700_000_005_000);
        assert!(summary.contains("Coverage:"));
        assert!(summary.contains("Latest verdict"));
        assert!(summary.contains("Slot underfilled"));
        assert!(summary.contains("Recall risk:"));
        assert!(summary.contains("avg score"));
        assert!(summary.contains("Working set:"));
        assert!(summary.contains("Counts: items"));
    }

    #[test]
    fn context_summary_handles_missing_sections() {
        let snapshot = json!({
            "generated": "2025-10-02T17:15:00Z"
        });
        let summary = render_context_telemetry_summary(&snapshot, 1_700_000_000_000);
        assert!(summary.contains("Generated"));
        assert!(summary.contains("no context telemetry"));
    }

    #[test]
    fn hash_admin_token_matches_sha256() {
        assert_eq!(
            hash_admin_token("secret"),
            "2bb80d537b1da3e38bd30361aa855686bde0eacd7162fef6a25fe97bf527a25b"
        );
    }

    #[test]
    fn encode_hex_respects_case() {
        let bytes = [0xde, 0xad, 0xbe, 0xef];
        assert_eq!(encode_hex(&bytes, false), "deadbeef");
        assert_eq!(encode_hex(&bytes, true), "DEADBEEF");
    }

    #[test]
    fn tool_cache_summary_includes_key_metrics() {
        let snapshot = ToolCacheSnapshot {
            hit: 8,
            miss: 2,
            coalesced: 3,
            errors: 1,
            bypass: 4,
            capacity: 128,
            ttl_secs: 600,
            entries: 42,
            latency_saved_ms_total: 12_500,
            latency_saved_samples: 5,
            avg_latency_saved_ms: 250.0,
            payload_bytes_saved_total: 512_000,
            payload_saved_samples: 5,
            avg_payload_bytes_saved: 102_400.0,
            avg_hit_age_secs: 18.5,
            hit_age_samples: 3,
            last_hit_age_secs: Some(12),
            max_hit_age_secs: Some(45),
            stampede_suppression_rate: 0.4,
            last_latency_saved_ms: Some(200),
            last_payload_bytes: Some(204_800),
            _extra: serde_json::Map::new(),
        };
        let summary = render_tool_cache_summary(&snapshot, "http://127.0.0.1:8091");
        assert!(summary.contains("Tool cache"));
        assert!(summary.contains("hit 8 | miss 2"));
        assert!(summary.contains("avg 250.0 ms"));
        assert!(summary.contains("avg 100.0 KB"));
        assert!(summary.contains("max 45 s"));
    }
}
