use anyhow::{anyhow, bail, ensure, Context, Result};
use arw_core::{
    effective_paths, gating, gating_keys, hello_core, introspect_tools, load_effective_paths,
    resolve_config_path, runtime_bundles,
};
use arw_runtime::{RuntimeAccelerator, RuntimeModality, RuntimeSeverity, RuntimeState};
use base64::Engine;
use chrono::{DateTime, Local, SecondsFormat, TimeZone, Utc};
use clap::CommandFactory;
use clap::{Args, Parser, Subcommand, ValueEnum};
use csv::WriterBuilder;
use json_patch::{patch as apply_json_patch, Patch as JsonPatch};
use rand::RngCore;
use reqwest::{blocking::Client, header::ACCEPT, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::env;
use std::fmt::Write as _;
use std::fs::{create_dir_all, OpenOptions};
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
    /// Emergency teardown for active policy capsules
    Teardown(CapsuleTeardownArgs),
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
    /// Egress helper commands
    Egress {
        #[command(subcommand)]
        cmd: AdminEgressCmd,
    },
    /// Memory review helpers
    Review {
        #[command(subcommand)]
        cmd: AdminReviewCmd,
    },
    /// Identity registry helpers
    Identity {
        #[command(subcommand)]
        cmd: AdminIdentityCmd,
    },
}

#[derive(Subcommand)]
enum AdminEgressCmd {
    /// Show configured egress scopes from /state/egress/settings
    Scopes(AdminEgressScopesArgs),
    /// Manage individual egress scopes
    Scope {
        #[command(subcommand)]
        cmd: AdminEgressScopeCmd,
    },
}

#[derive(Subcommand)]
enum AdminEgressScopeCmd {
    /// Create a new scope or fail if the id already exists
    Add(AdminEgressScopeAddArgs),
    /// Update an existing scope by id
    Update(AdminEgressScopeUpdateArgs),
    /// Remove a scope by id
    Remove(AdminEgressScopeRemoveArgs),
}

#[derive(Subcommand)]
enum AdminTokenCmd {
    /// Hash an admin token for ARW_ADMIN_TOKEN_SHA256
    Hash(AdminTokenHashArgs),
    /// Generate a random admin token
    Generate(AdminTokenGenerateArgs),
    /// Persist an admin token (and optional hash) to an env file
    Persist(AdminTokenPersistArgs),
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

#[derive(Args, Clone)]
struct AdminTokenPersistArgs {
    /// Path to the env file that should hold the admin token (default: ./.env)
    #[arg(long, default_value = ".env")]
    path: PathBuf,
    /// Reuse this token instead of generating a new one
    #[arg(long, conflicts_with = "read_env")]
    token: Option<String>,
    /// Read token from an environment variable (conflicts with --token)
    #[arg(long = "read-env", conflicts_with = "token")]
    read_env: Option<String>,
    /// Number of random bytes when generating a new token (default: 32)
    #[arg(long, default_value_t = 32)]
    length: usize,
    /// Output format for generated tokens
    #[arg(long, value_enum, default_value_t = TokenFormat::Hex)]
    format: TokenFormat,
    /// Upper-case hex output when format=hex
    #[arg(long)]
    uppercase: bool,
    /// Also persist ARW_ADMIN_TOKEN_SHA256 alongside the token
    #[arg(long)]
    hash: bool,
    /// Print the token to stdout after persisting
    #[arg(long = "print-token")]
    print_token: bool,
    /// Print the SHA-256 hash to stdout (computed automatically)
    #[arg(long = "print-hash")]
    print_hash: bool,
}

#[derive(Args, Clone)]
struct AdminIdentityCommonArgs {
    /// Tenants manifest path; defaults to ARW_TENANTS_FILE or configs/security/tenants.toml
    #[arg(long, value_name = "PATH")]
    tenants_file: Option<PathBuf>,
}

#[derive(Args, Clone)]
struct AdminIdentityAddArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier (letters, numbers, '-', '_', '.', '@')
    #[arg(long)]
    id: String,
    /// Optional display name
    #[arg(long)]
    display_name: Option<String>,
    /// Assign role (repeatable)
    #[arg(long = "role", value_name = "ROLE")]
    roles: Vec<String>,
    /// Assign scope (repeatable)
    #[arg(long = "scope", value_name = "SCOPE")]
    scopes: Vec<String>,
    /// Plain token to hash and store (repeatable)
    #[arg(long = "token", value_name = "TOKEN")]
    tokens: Vec<String>,
    /// Pre-hashed token (SHA-256 hex, repeatable)
    #[arg(long = "token-sha256", value_name = "HASH")]
    token_sha256: Vec<String>,
    /// Disable the principal
    #[arg(long, conflicts_with = "enable")]
    disable: bool,
    /// Ensure principal is enabled
    #[arg(long, conflicts_with = "disable")]
    enable: bool,
    /// Fail if principal already exists
    #[arg(long)]
    fail_if_exists: bool,
    /// Merge roles/scopes/tokens instead of replacing when the principal exists
    #[arg(long)]
    merge: bool,
}

#[derive(Args, Clone)]
struct AdminIdentityRemoveArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to remove
    #[arg(long)]
    id: String,
    /// Succeed without error when the principal does not exist
    #[arg(long)]
    ignore_missing: bool,
}

#[derive(Args, Clone)]
struct AdminIdentityEnableArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to enable
    #[arg(long)]
    id: String,
}

#[derive(Args, Clone)]
struct AdminIdentityDisableArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to disable
    #[arg(long)]
    id: String,
}

#[derive(Args, Clone)]
struct AdminIdentityRotateArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to rotate tokens for
    #[arg(long)]
    id: String,
    /// Provide a precomputed token instead of generating one
    #[arg(long)]
    token: Option<String>,
    /// Number of random bytes when generating a token
    #[arg(long, default_value_t = 32)]
    length: usize,
    /// Output format for generated tokens
    #[arg(long, value_enum, default_value_t = TokenFormat::Hex)]
    format: TokenFormat,
    /// Upper-case hex output when format=hex
    #[arg(long)]
    uppercase: bool,
    /// Keep existing token hashes and append the new one
    #[arg(long)]
    append: bool,
    /// Suppress printing the new token to stdout
    #[arg(long)]
    quiet: bool,
    /// Also print the SHA-256 hash
    #[arg(long)]
    print_hash: bool,
}

#[derive(Args, Clone)]
struct AdminIdentityShowArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Emit JSON instead of an aligned table
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Subcommand)]
enum AdminIdentityCmd {
    /// Create or update a principal in the tenants manifest
    Add(AdminIdentityAddArgs),
    /// Remove a principal from the tenants manifest
    Remove(AdminIdentityRemoveArgs),
    /// Enable a principal
    Enable(AdminIdentityEnableArgs),
    /// Disable a principal
    Disable(AdminIdentityDisableArgs),
    /// Rotate or append a token for a principal
    Rotate(AdminIdentityRotateArgs),
    /// Show the tenants manifest content
    Show(AdminIdentityShowArgs),
}

#[derive(Args, Clone)]
struct AdminEgressScopesArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
struct AdminEgressScopeBaseArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
}

impl AdminEgressScopeBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args, Clone)]
struct AdminEgressScopeAddArgs {
    #[command(flatten)]
    base: AdminEgressScopeBaseArgs,
    /// Scope identifier (unique)
    #[arg(long)]
    id: String,
    /// Optional description
    #[arg(long)]
    description: Option<String>,
    /// Domains/hosts allowed by this scope (repeatable)
    #[arg(long = "host", value_name = "HOST", num_args = 1..)]
    hosts: Vec<String>,
    /// CIDR ranges allowed by this scope (repeatable)
    #[arg(long = "cidr", value_name = "CIDR", num_args = 1..)]
    cidrs: Vec<String>,
    /// Allowed ports (repeatable)
    #[arg(long = "port", value_name = "PORT", value_parser = clap::value_parser!(u16), num_args = 1..)]
    ports: Vec<u16>,
    /// Allowed protocols (http|https|tcp)
    #[arg(long = "protocol", value_name = "PROTOCOL", num_args = 1..)]
    protocols: Vec<String>,
    /// Capabilities to mint when scope grants access (repeatable)
    #[arg(long = "lease-cap", value_name = "CAP", num_args = 1..)]
    lease_capabilities: Vec<String>,
    /// Expiry timestamp (RFC3339)
    #[arg(long = "expires-at")]
    expires_at: Option<String>,
}

#[derive(Args, Clone)]
struct AdminEgressScopeUpdateArgs {
    #[command(flatten)]
    base: AdminEgressScopeBaseArgs,
    /// Scope identifier to update
    #[arg(long)]
    id: String,
    /// Optional description
    #[arg(long)]
    description: Option<String>,
    /// Replace hosts list (repeatable)
    #[arg(long = "host", value_name = "HOST", num_args = 1..)]
    hosts: Vec<String>,
    /// Clear hosts entirely
    #[arg(long)]
    clear_hosts: bool,
    /// Replace CIDR list (repeatable)
    #[arg(long = "cidr", value_name = "CIDR", num_args = 1..)]
    cidrs: Vec<String>,
    /// Clear CIDRs entirely
    #[arg(long)]
    clear_cidrs: bool,
    /// Replace ports list (repeatable)
    #[arg(long = "port", value_name = "PORT", value_parser = clap::value_parser!(u16), num_args = 1..)]
    ports: Vec<u16>,
    /// Clear ports entirely
    #[arg(long)]
    clear_ports: bool,
    /// Replace protocols list (repeatable)
    #[arg(long = "protocol", value_name = "PROTOCOL", num_args = 1..)]
    protocols: Vec<String>,
    /// Clear protocols entirely
    #[arg(long)]
    clear_protocols: bool,
    /// Replace lease capability list (repeatable)
    #[arg(long = "lease-cap", value_name = "CAP", num_args = 1..)]
    lease_capabilities: Vec<String>,
    /// Clear lease capabilities entirely
    #[arg(long)]
    clear_lease_caps: bool,
    /// Update expiry timestamp (use empty string to clear)
    #[arg(long = "expires-at")]
    expires_at: Option<String>,
    /// Remove expiry timestamp
    #[arg(long)]
    clear_expires: bool,
}

#[derive(Args, Clone)]
struct AdminEgressScopeRemoveArgs {
    #[command(flatten)]
    base: AdminEgressScopeBaseArgs,
    /// Scope identifier to remove
    id: String,
}

#[derive(Subcommand)]
enum AdminReviewCmd {
    /// Memory quarantine helpers
    Quarantine {
        #[command(subcommand)]
        cmd: AdminReviewQuarantineCmd,
    },
}

#[derive(Subcommand)]
enum AdminReviewQuarantineCmd {
    /// List memory quarantine entries
    List(AdminReviewQuarantineListArgs),
    /// Admit, reject, or requeue a quarantine entry
    Admit(AdminReviewQuarantineAdmitArgs),
    /// Show a specific quarantine entry
    Show(AdminReviewQuarantineShowArgs),
}

#[derive(Args, Clone)]
struct AdminReviewBaseArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
}

impl AdminReviewBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args, Clone)]
struct AdminReviewQuarantineListArgs {
    #[command(flatten)]
    base: AdminReviewBaseArgs,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Emit newline-delimited JSON (conflicts with --json/--csv)
    #[arg(long, conflicts_with_all = ["json", "pretty", "csv"])]
    ndjson: bool,
    /// Emit CSV (conflicts with --json/--ndjson)
    #[arg(long, conflicts_with_all = ["json", "pretty", "ndjson"])]
    csv: bool,
    /// Filter by state (repeatable)
    #[arg(long = "state", value_enum, num_args = 1..)]
    states: Vec<AdminReviewStateFilter>,
    /// Filter by project identifier
    #[arg(long)]
    project: Option<String>,
    /// Filter by source (tool|ingest|world_diff|manual)
    #[arg(long)]
    source: Option<String>,
    /// Limit the number of entries returned
    #[arg(long)]
    limit: Option<usize>,
    /// Include (truncated) content preview in output
    #[arg(long = "show-preview")]
    show_preview: bool,
}

#[derive(Args, Clone)]
struct AdminReviewQuarantineAdmitArgs {
    #[command(flatten)]
    base: AdminReviewBaseArgs,
    /// Quarantine entry identifier(s)
    #[arg(long = "id", value_name = "ID", num_args = 1..)]
    ids: Vec<String>,
    /// Decision to apply
    #[arg(long, value_enum, default_value_t = AdminReviewDecision::Admit)]
    decision: AdminReviewDecision,
    /// Optional reviewer note
    #[arg(long)]
    note: Option<String>,
    /// Reviewer identifier (email or handle)
    #[arg(long = "by")]
    reviewed_by: Option<String>,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Include (truncated) content preview when rendering the updated entry
    #[arg(long = "show-preview")]
    show_preview: bool,
}

#[derive(Args, Clone)]
struct AdminReviewQuarantineShowArgs {
    #[command(flatten)]
    base: AdminReviewBaseArgs,
    /// Quarantine entry identifier to display
    #[arg(long)]
    id: String,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Include (truncated) content preview and review metadata
    #[arg(long = "show-preview")]
    show_preview: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum AdminReviewStateFilter {
    Queued,
    NeedsExtractor,
    Admitted,
    Rejected,
}

impl AdminReviewStateFilter {
    fn as_str(&self) -> &'static str {
        match self {
            AdminReviewStateFilter::Queued => "queued",
            AdminReviewStateFilter::NeedsExtractor => "needs_extractor",
            AdminReviewStateFilter::Admitted => "admitted",
            AdminReviewStateFilter::Rejected => "rejected",
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum AdminReviewDecision {
    Admit,
    Reject,
    ExtractAgain,
}

impl AdminReviewDecision {
    fn as_str(&self) -> &'static str {
        match self {
            AdminReviewDecision::Admit => "admit",
            AdminReviewDecision::Reject => "reject",
            AdminReviewDecision::ExtractAgain => "extract_again",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ToolCacheSnapshot {
    hit: u64,
    miss: u64,
    coalesced: u64,
    errors: u64,
    bypass: u64,
    payload_too_large: u64,
    capacity: u64,
    ttl_secs: u64,
    entries: u64,
    max_payload_bytes: Option<u64>,
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

#[derive(Args, Clone)]
struct CapsuleTeardownArgs {
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
    /// Inspect managed runtime bundle catalogs
    Bundles {
        #[command(subcommand)]
        cmd: RuntimeBundlesCmd,
    },
}

#[derive(Subcommand)]
enum RuntimeBundlesCmd {
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
) {
    if !roots.is_empty() {
        println!("Roots: {}", roots.join(", "));
    }
    if let Some(ts) = generated {
        println!("Generated: {}", ts);
    }
    println!(
        "Found {} bundle catalog{}.",
        catalogs.len(),
        if catalogs.len() == 1 { "" } else { "s" }
    );
    if catalogs.is_empty() {
        println!("(no bundles declared)");
        return;
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
}

#[derive(Subcommand)]
enum StateCmd {
    /// Snapshot filtered actions via /state/actions
    Actions(StateActionsArgs),
    /// Inspect identity registry via /state/identity
    Identity(StateIdentityArgs),
    /// Inspect cluster registry via /state/cluster
    Cluster(StateClusterArgs),
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
    /// Tail modular events (modular.agent/tool accepted) with sensible defaults
    Modular(ModularTailArgs),
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

impl RuntimeBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
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
struct RuntimeBundlesListArgs {
    #[command(flatten)]
    base: RuntimeBaseArgs,
    /// Directory containing bundle catalogs (defaults to configs/runtime/)
    #[arg(long, value_name = "DIR")]
    dir: Option<PathBuf>,
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
struct RuntimeBundlesReloadArgs {
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
struct RuntimeBundlesInstallArgs {
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
struct RuntimeBundlesImportArgs {
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
struct RuntimeBundlesRollbackArgs {
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
    })
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
    #[arg(long, conflicts_with = "updated_relative")]
    updated_since: Option<String>,
    /// Relative lookback for action updates (e.g., 30m, 4h) converted to RFC3339
    #[arg(
        long = "updated-relative",
        value_name = "WINDOW",
        conflicts_with = "updated_since"
    )]
    updated_relative: Option<String>,
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
struct StateIdentityArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
struct StateClusterArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Emit raw JSON instead of formatted text
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliIdentitySnapshot {
    #[serde(default)]
    loaded_ms: Option<u64>,
    #[serde(default)]
    source_path: Option<String>,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    principals: Vec<CliIdentityPrincipal>,
    #[serde(default, rename = "env_principals")]
    env: Vec<CliIdentityPrincipal>,
    #[serde(default)]
    diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliClusterSnapshot {
    #[serde(default)]
    nodes: Vec<CliClusterNode>,
    #[serde(default)]
    generated: Option<String>,
    #[serde(default)]
    generated_ms: Option<u64>,
    #[serde(default)]
    ttl_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliClusterNode {
    id: String,
    role: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    health: Option<String>,
    #[serde(default)]
    capabilities: Option<JsonValue>,
    #[serde(default)]
    models: Option<JsonValue>,
    #[serde(default)]
    last_seen: Option<String>,
    #[serde(default)]
    last_seen_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliIdentityPrincipal {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    tokens: Option<usize>,
    #[serde(default)]
    notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CliTenantsFile {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    principals: Vec<CliTenantPrincipal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliTenantPrincipal {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default, rename = "token_sha256")]
    token_sha256: Vec<String>,
    #[serde(default)]
    disabled: bool,
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

#[derive(Args, Clone)]
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
    /// Skip entries older than this relative window on the first fetch (e.g. 15m, 2h30m)
    #[arg(
        long = "after-relative",
        value_name = "WINDOW",
        conflicts_with = "after_cursor"
    )]
    after_relative: Option<String>,
    /// Maximum characters to display for payload/policy lines (0 hides them)
    #[arg(long, default_value_t = 160)]
    payload_width: usize,
}

#[derive(Args, Clone)]
struct ModularTailArgs {
    #[command(flatten)]
    journal: EventsJournalArgs,
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
                AdminTokenCmd::Persist(args) => {
                    if let Err(e) = cmd_admin_token_persist(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            },
            AdminCmd::Egress { cmd } => match cmd {
                AdminEgressCmd::Scopes(args) => {
                    if let Err(e) = cmd_admin_egress_scopes(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminEgressCmd::Scope { cmd } => match cmd {
                    AdminEgressScopeCmd::Add(args) => {
                        if let Err(e) = cmd_admin_egress_scope_add(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                    AdminEgressScopeCmd::Update(args) => {
                        if let Err(e) = cmd_admin_egress_scope_update(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                    AdminEgressScopeCmd::Remove(args) => {
                        if let Err(e) = cmd_admin_egress_scope_remove(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                },
            },
            AdminCmd::Review { cmd } => match cmd {
                AdminReviewCmd::Quarantine { cmd } => match cmd {
                    AdminReviewQuarantineCmd::List(args) => {
                        if let Err(e) = cmd_admin_review_quarantine_list(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                    AdminReviewQuarantineCmd::Admit(args) => {
                        if let Err(e) = cmd_admin_review_quarantine_admit(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                    AdminReviewQuarantineCmd::Show(args) => {
                        if let Err(e) = cmd_admin_review_quarantine_show(&args) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                    }
                },
            },
            AdminCmd::Identity { cmd } => match cmd {
                AdminIdentityCmd::Add(args) => {
                    if let Err(e) = cmd_admin_identity_add(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminIdentityCmd::Remove(args) => {
                    if let Err(e) = cmd_admin_identity_remove(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminIdentityCmd::Enable(args) => {
                    if let Err(e) = cmd_admin_identity_enable(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminIdentityCmd::Disable(args) => {
                    if let Err(e) = cmd_admin_identity_disable(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminIdentityCmd::Rotate(args) => {
                    if let Err(e) = cmd_admin_identity_rotate(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                AdminIdentityCmd::Show(args) => {
                    if let Err(e) = cmd_admin_identity_show(&args) {
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
            CapCmd::Teardown(args) => {
                if let Err(e) = cmd_capsule_teardown(&args) {
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
            RuntimeCmd::Bundles { cmd } => match cmd {
                RuntimeBundlesCmd::List(args) => {
                    if let Err(e) = cmd_runtime_bundles_list(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeBundlesCmd::Reload(args) => {
                    if let Err(e) = cmd_runtime_bundles_reload(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeBundlesCmd::Install(args) => {
                    if let Err(e) = cmd_runtime_bundles_install(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeBundlesCmd::Import(args) => {
                    if let Err(e) = cmd_runtime_bundles_import(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeBundlesCmd::Rollback(args) => {
                    if let Err(e) = cmd_runtime_bundles_rollback(&args) {
                        eprintln!("{}", e);
                        std::process::exit(1);
                    }
                }
            },
        },
        Some(Commands::State { cmd }) => match cmd {
            StateCmd::Actions(args) => {
                if let Err(e) = cmd_state_actions(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            StateCmd::Identity(args) => {
                if let Err(e) = cmd_state_identity(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            StateCmd::Cluster(args) => {
                if let Err(e) = cmd_state_cluster(&args) {
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
            EventsCmd::Modular(args) => {
                if let Err(e) = cmd_events_modular(&args) {
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

fn generate_admin_token_string(
    length: usize,
    format: TokenFormat,
    uppercase: bool,
) -> Result<String> {
    if length == 0 {
        anyhow::bail!("--length must be greater than zero");
    }
    let mut bytes = vec![0u8; length];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);
    let token = match format {
        TokenFormat::Hex => encode_hex(&bytes, uppercase),
        TokenFormat::Base64 => base64::engine::general_purpose::STANDARD_NO_PAD.encode(&bytes),
    };
    Ok(token)
}

fn cmd_admin_token_generate(args: &AdminTokenGenerateArgs) -> Result<()> {
    let token = if args.length == 0 {
        anyhow::bail!("--length must be greater than zero");
    } else {
        generate_admin_token_string(args.length, args.format, args.uppercase)?
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

fn cmd_admin_token_persist(args: &AdminTokenPersistArgs) -> Result<()> {
    let (token, generated) = resolve_or_generate_admin_token(args)?;
    let hash_value = if args.hash || args.print_hash {
        Some(hash_admin_token(&token))
    } else {
        None
    };

    let hash_for_file = if args.hash {
        hash_value.as_deref()
    } else {
        None
    };

    let existed = persist_admin_token_file(&args.path, &token, hash_for_file)?;

    if args.print_token {
        println!("{}", token);
    }
    if args.print_hash {
        if let Some(hash) = &hash_value {
            println!("{}", hash);
        }
    }

    let descriptor = if args.hash {
        "ARW_ADMIN_TOKEN and ARW_ADMIN_TOKEN_SHA256"
    } else {
        "ARW_ADMIN_TOKEN"
    };
    let mut message = format!(
        "{} {} in {}",
        if existed { "Updated" } else { "Created" },
        descriptor,
        args.path.display()
    );
    if generated {
        message.push_str(" (generated new token)");
    } else if args.token.is_some() || args.read_env.is_some() {
        message.push_str(" (reused provided token)");
    }
    println!("{}", message);

    Ok(())
}

fn cmd_admin_identity_add(args: &AdminIdentityAddArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let mut file = load_tenants_file(&path)?;

    let id = sanitize_identity_id(&args.id)
        .ok_or_else(|| anyhow!("invalid principal id `{}`", args.id))?;
    let display_name = args
        .display_name
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let roles_input = normalize_labels(&args.roles);
    let scopes_input = normalize_labels(&args.scopes);

    let mut token_hashes: Vec<String> = Vec::new();
    for token in &args.tokens {
        let trimmed = token.trim();
        ensure!(!trimmed.is_empty(), "--token values cannot be empty");
        token_hashes.push(hash_admin_token(trimmed));
    }
    for hash in &args.token_sha256 {
        let trimmed = hash.trim();
        ensure!(
            is_valid_sha256(trimmed),
            "token-sha256 value `{}` must be a 64-character hex digest",
            hash
        );
        token_hashes.push(trimmed.to_ascii_lowercase());
    }
    dedup_sorted(&mut token_hashes);

    let disable_override = if args.disable {
        Some(true)
    } else if args.enable {
        Some(false)
    } else {
        None
    };

    if let Some(existing) = file.principals.iter_mut().find(|p| p.id == id) {
        if args.fail_if_exists {
            bail!("principal `{}` already exists in {}", id, path.display());
        }
        if let Some(name) = display_name.as_ref() {
            existing.display_name = Some(name.clone());
        }
        if !roles_input.is_empty() {
            if args.merge {
                existing.roles.extend(roles_input.clone());
                dedup_sorted(&mut existing.roles);
            } else {
                existing.roles = roles_input.clone();
            }
        }
        if !scopes_input.is_empty() {
            if args.merge {
                existing.scopes.extend(scopes_input.clone());
                dedup_sorted(&mut existing.scopes);
            } else {
                existing.scopes = scopes_input.clone();
            }
        }
        if !token_hashes.is_empty() {
            if args.merge {
                existing.token_sha256.extend(token_hashes.clone());
                dedup_sorted(&mut existing.token_sha256);
            } else {
                existing.token_sha256 = token_hashes.clone();
            }
        } else if existing.token_sha256.is_empty() {
            bail!(
                "principal `{}` has no tokens; provide --token or --token-sha256",
                id
            );
        }
        if let Some(flag) = disable_override {
            existing.disabled = flag;
        }
        save_tenants_file(&path, &mut file)?;
        println!("principal `{}` updated in {}", id, path.display());
        return Ok(());
    }

    ensure!(
        !token_hashes.is_empty(),
        "provide at least one --token or --token-sha256 when creating a principal"
    );

    let mut principal = CliTenantPrincipal {
        id: id.clone(),
        display_name: display_name.clone(),
        roles: roles_input.clone(),
        scopes: scopes_input.clone(),
        token_sha256: token_hashes.clone(),
        disabled: disable_override.unwrap_or(false),
    };
    dedup_sorted(&mut principal.roles);
    dedup_sorted(&mut principal.scopes);
    dedup_sorted(&mut principal.token_sha256);

    file.principals.push(principal);
    save_tenants_file(&path, &mut file)?;
    println!("principal `{}` added to {}", id, path.display());
    Ok(())
}

fn cmd_admin_identity_remove(args: &AdminIdentityRemoveArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let mut file = load_tenants_file(&path)?;
    let id = sanitize_identity_id(&args.id)
        .ok_or_else(|| anyhow!("invalid principal id `{}`", args.id))?;

    let initial_len = file.principals.len();
    file.principals.retain(|p| p.id != id);
    if file.principals.len() == initial_len {
        if args.ignore_missing {
            println!(
                "principal `{}` not present in {}; nothing to do",
                id,
                path.display()
            );
            return Ok(());
        }
        bail!("principal `{}` not found in {}", id, path.display());
    }

    save_tenants_file(&path, &mut file)?;
    println!("principal `{}` removed from {}", id, path.display());
    Ok(())
}

fn cmd_admin_identity_enable(args: &AdminIdentityEnableArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let mut file = load_tenants_file(&path)?;
    let id = sanitize_identity_id(&args.id)
        .ok_or_else(|| anyhow!("invalid principal id `{}`", args.id))?;
    let principal = file
        .principals
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow!("principal `{}` not found in {}", id, path.display()))?;
    if principal.disabled {
        principal.disabled = false;
        save_tenants_file(&path, &mut file)?;
        println!("principal `{}` enabled in {}", id, path.display());
    } else {
        println!("principal `{}` already enabled in {}", id, path.display());
    }
    Ok(())
}

fn cmd_admin_identity_disable(args: &AdminIdentityDisableArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let mut file = load_tenants_file(&path)?;
    let id = sanitize_identity_id(&args.id)
        .ok_or_else(|| anyhow!("invalid principal id `{}`", args.id))?;
    let principal = file
        .principals
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow!("principal `{}` not found in {}", id, path.display()))?;
    if !principal.disabled {
        principal.disabled = true;
        save_tenants_file(&path, &mut file)?;
        println!("principal `{}` disabled in {}", id, path.display());
    } else {
        println!("principal `{}` already disabled in {}", id, path.display());
    }
    Ok(())
}

fn cmd_admin_identity_rotate(args: &AdminIdentityRotateArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let mut file = load_tenants_file(&path)?;
    let id = sanitize_identity_id(&args.id)
        .ok_or_else(|| anyhow!("invalid principal id `{}`", args.id))?;
    let principal = file
        .principals
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| anyhow!("principal `{}` not found in {}", id, path.display()))?;

    let token = if let Some(raw) = &args.token {
        let trimmed = raw.trim();
        ensure!(!trimmed.is_empty(), "--token cannot be empty");
        trimmed.to_string()
    } else {
        generate_admin_token_string(args.length, args.format, args.uppercase)?
    };
    let digest = hash_admin_token(&token);

    if args.append {
        principal.token_sha256.push(digest.clone());
    } else {
        principal.token_sha256.clear();
        principal.token_sha256.push(digest.clone());
    }
    dedup_sorted(&mut principal.token_sha256);

    save_tenants_file(&path, &mut file)?;

    if !args.quiet {
        println!("token: {}", token);
    }
    if args.print_hash {
        println!("sha256: {}", digest);
    }
    println!("principal `{}` updated in {}", id, path.display());
    Ok(())
}

fn cmd_admin_identity_show(args: &AdminIdentityShowArgs) -> Result<()> {
    let path = resolve_tenants_path(&args.common.tenants_file)?;
    let file = load_tenants_file(&path)?;

    if args.json {
        let value = serde_json::to_value(&file).context("serializing tenants manifest")?;
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!("{}", value);
        }
        return Ok(());
    }

    let principals: Vec<CliIdentityPrincipal> = file
        .principals
        .iter()
        .map(|p| CliIdentityPrincipal {
            id: p.id.clone(),
            display_name: p.display_name.clone(),
            roles: p.roles.clone(),
            scopes: p.scopes.clone(),
            tokens: Some(p.token_sha256.len()),
            notes: if p.disabled {
                Some("disabled".into())
            } else {
                None
            },
        })
        .collect();

    let snapshot = CliIdentitySnapshot {
        loaded_ms: None,
        source_path: Some(path.to_string_lossy().to_string()),
        version: file.version,
        principals,
        env: Vec::new(),
        diagnostics: Vec::new(),
    };
    render_identity_snapshot(&snapshot);
    Ok(())
}

fn cmd_admin_egress_scopes(args: &AdminEgressScopesArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let snapshot = fetch_egress_settings(&client, base, token.as_deref())?;
    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string())
            );
        } else {
            println!("{}", snapshot);
        }
        return Ok(());
    }

    render_egress_scopes_text(&snapshot)?;
    Ok(())
}

fn cmd_admin_egress_scope_add(args: &AdminEgressScopeAddArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    let snapshot = fetch_egress_settings(&client, base, token.as_deref())?;
    let mut scopes = extract_scopes(&snapshot)?;
    let scope_id = args.id.trim();
    ensure!(!scope_id.is_empty(), "--id cannot be empty");
    if scopes
        .iter()
        .any(|scope| scope.get("id").and_then(|v| v.as_str()).unwrap_or("") == scope_id)
    {
        bail!("scope '{}' already exists", scope_id);
    }

    let hosts = sanitize_hosts(&args.hosts);
    let cidrs = sanitize_list(&args.cidrs);
    if hosts.is_empty() && cidrs.is_empty() {
        bail!("provide at least one --host or --cidr");
    }
    let ports = sanitize_ports(&args.ports);
    let protocols = normalize_protocols(&args.protocols)?;
    let lease_caps = sanitize_list(&args.lease_capabilities);

    let mut scope = serde_json::Map::new();
    scope.insert("id".into(), json!(scope_id));
    if let Some(desc) = args
        .description
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        scope.insert("description".into(), json!(desc));
    }
    scope.insert("hosts".into(), json!(hosts));
    scope.insert("cidrs".into(), json!(cidrs));
    if !ports.is_empty() {
        scope.insert("ports".into(), json!(ports));
    }
    if !protocols.is_empty() {
        scope.insert("protocols".into(), json!(protocols));
    }
    if !lease_caps.is_empty() {
        scope.insert("lease_capabilities".into(), json!(lease_caps));
    }
    if let Some(expires) = args
        .expires_at
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        scope.insert("expires_at".into(), json!(expires));
    }

    scopes.push(JsonValue::Object(scope));
    let payload = json!({"scopes": scopes});
    let updated = post_egress_settings(&client, base, token.as_deref(), &payload)?;
    println!("Scope '{}' created.", scope_id);
    render_egress_scopes_text(&updated)?;
    Ok(())
}

fn cmd_admin_egress_scope_update(args: &AdminEgressScopeUpdateArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    let snapshot = fetch_egress_settings(&client, base, token.as_deref())?;
    let mut scopes = extract_scopes(&snapshot)?;
    let scope_id = args.id.trim();
    ensure!(!scope_id.is_empty(), "--id cannot be empty");
    let position = scopes
        .iter()
        .position(|scope| scope.get("id").and_then(|v| v.as_str()).unwrap_or("") == scope_id)
        .ok_or_else(|| anyhow!("scope '{}' not found", scope_id))?;

    let mut scope_obj = scopes[position]
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("invalid scope payload"))?;

    if let Some(desc) = args.description.as_ref().map(|s| s.trim()) {
        if desc.is_empty() {
            scope_obj.remove("description");
        } else {
            scope_obj.insert("description".into(), json!(desc));
        }
    }

    if args.clear_hosts {
        scope_obj.insert("hosts".into(), json!([]));
    } else if !args.hosts.is_empty() {
        scope_obj.insert("hosts".into(), json!(sanitize_hosts(&args.hosts)));
    }

    if args.clear_cidrs {
        scope_obj.insert("cidrs".into(), json!([]));
    } else if !args.cidrs.is_empty() {
        scope_obj.insert("cidrs".into(), json!(sanitize_list(&args.cidrs)));
    }

    if args.clear_ports {
        scope_obj.remove("ports");
    } else if !args.ports.is_empty() {
        scope_obj.insert("ports".into(), json!(sanitize_ports(&args.ports)));
    }

    if args.clear_protocols {
        scope_obj.remove("protocols");
    } else if !args.protocols.is_empty() {
        scope_obj.insert(
            "protocols".into(),
            json!(normalize_protocols(&args.protocols)?),
        );
    }

    if args.clear_lease_caps {
        scope_obj.remove("lease_capabilities");
    } else if !args.lease_capabilities.is_empty() {
        scope_obj.insert(
            "lease_capabilities".into(),
            json!(sanitize_list(&args.lease_capabilities)),
        );
    }

    if args.clear_expires {
        scope_obj.remove("expires_at");
    } else if let Some(expires) = args.expires_at.as_ref() {
        let trimmed = expires.trim();
        if trimmed.is_empty() {
            scope_obj.remove("expires_at");
        } else {
            scope_obj.insert("expires_at".into(), json!(trimmed));
        }
    }

    let hosts = scope_obj
        .get("hosts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let cidrs = scope_obj
        .get("cidrs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if hosts.is_empty() && cidrs.is_empty() {
        bail!("scope '{}' must have at least one host or cidr", scope_id);
    }

    scopes[position] = JsonValue::Object(scope_obj);
    let payload = json!({"scopes": scopes});
    let updated = post_egress_settings(&client, base, token.as_deref(), &payload)?;
    println!("Scope '{}' updated.", scope_id);
    render_egress_scopes_text(&updated)?;
    Ok(())
}

fn cmd_admin_egress_scope_remove(args: &AdminEgressScopeRemoveArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    let snapshot = fetch_egress_settings(&client, base, token.as_deref())?;
    let mut scopes = extract_scopes(&snapshot)?;
    let scope_id = args.id.trim();
    let orig_len = scopes.len();
    scopes.retain(|scope| scope.get("id").and_then(|v| v.as_str()).unwrap_or("") != scope_id);
    if scopes.len() == orig_len {
        bail!("scope '{}' not found", scope_id);
    }

    let payload = json!({"scopes": scopes});
    let updated = post_egress_settings(&client, base, token.as_deref(), &payload)?;
    println!("Scope '{}' removed.", scope_id);
    render_egress_scopes_text(&updated)?;
    Ok(())
}

fn cmd_admin_review_quarantine_list(args: &AdminReviewQuarantineListArgs) -> Result<()> {
    if let Some(limit) = args.limit {
        ensure!(limit > 0, "--limit must be greater than zero");
    }

    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    let entries = fetch_memory_quarantine_entries(&client, base, token.as_deref())?;

    let state_filter: Option<HashSet<&'static str>> = if args.states.is_empty() {
        None
    } else {
        Some(args.states.iter().map(|s| s.as_str()).collect())
    };
    let project_filter = args
        .project
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let source_filter = args
        .source
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let mut filtered: Vec<JsonValue> = entries
        .into_iter()
        .filter(|entry| {
            if let Some(states) = &state_filter {
                let state = entry.get("state").and_then(|v| v.as_str()).unwrap_or("");
                if !states.contains(state) {
                    return false;
                }
            }
            if let Some(project) = &project_filter {
                let project_id = entry
                    .get("project_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if project_id != project {
                    return false;
                }
            }
            if let Some(source) = &source_filter {
                let entry_source = entry.get("source").and_then(|v| v.as_str()).unwrap_or("");
                if entry_source.is_empty() || !entry_source.eq_ignore_ascii_case(source) {
                    return false;
                }
            }
            true
        })
        .collect();

    filtered.sort_by_key(|entry| Reverse(parse_quarantine_time(entry)));
    if let Some(limit) = args.limit {
        if filtered.len() > limit {
            filtered.truncate(limit);
        }
    }

    if args.ndjson {
        render_quarantine_entries_ndjson(&filtered)?;
        return Ok(());
    }

    if args.csv {
        render_quarantine_entries_csv(&filtered)?;
        return Ok(());
    }

    if args.json {
        let payload = JsonValue::Array(filtered.clone());
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string())
            );
        }
        return Ok(());
    }

    render_quarantine_entries_text(&filtered, args.show_preview)?;
    Ok(())
}

fn cmd_admin_review_quarantine_admit(args: &AdminReviewQuarantineAdmitArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    ensure!(!args.ids.is_empty(), "provide at least one --id");

    let mut id_list: Vec<String> = Vec::new();
    for raw in &args.ids {
        let trimmed = raw.trim();
        ensure!(!trimmed.is_empty(), "--id cannot be empty");
        id_list.push(trimmed.to_string());
    }

    let mut json_responses: Vec<JsonValue> = Vec::new();

    for id in id_list {
        let mut body = serde_json::Map::new();
        body.insert("id".into(), JsonValue::String(id.clone()));
        body.insert(
            "decision".into(),
            JsonValue::String(args.decision.as_str().to_string()),
        );
        if let Some(note) = args
            .note
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            body.insert("note".into(), JsonValue::String(note.to_string()));
        }
        if let Some(by) = args
            .reviewed_by
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            body.insert("reviewed_by".into(), JsonValue::String(by.to_string()));
        }
        let payload = JsonValue::Object(body);

        let response = post_memory_quarantine_admit(&client, base, token.as_deref(), &payload)?;

        if args.json {
            json_responses.push(response);
            continue;
        }

        let removed = response
            .get("removed")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if removed == 0 {
            println!("No quarantine entry with id '{}' was found.", id);
            continue;
        }

        println!(
            "Entry '{}' processed with decision '{}'.",
            id,
            args.decision.as_str()
        );

        if let Some(entry) = response.get("entry") {
            if !entry.is_null() {
                render_quarantine_entries_text(std::slice::from_ref(entry), args.show_preview)?;
            }
        }
    }

    if args.json {
        let payload = if json_responses.len() == 1 {
            json_responses.into_iter().next().unwrap()
        } else {
            JsonValue::Array(json_responses)
        };
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string())
            );
        }
    }

    Ok(())
}

fn cmd_admin_review_quarantine_show(args: &AdminReviewQuarantineShowArgs) -> Result<()> {
    let id = args.id.trim();
    ensure!(!id.is_empty(), "--id cannot be empty");

    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();

    let entries = fetch_memory_quarantine_entries(&client, base, token.as_deref())?;
    let matching: Vec<JsonValue> = entries
        .into_iter()
        .filter(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(|value| value == id)
                .unwrap_or(false)
        })
        .collect();

    if matching.is_empty() {
        println!("No quarantine entry with id '{}' found.", id);
        return Ok(());
    }

    if args.json {
        let payload = JsonValue::Array(matching.clone());
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&payload).unwrap_or_else(|_| payload.to_string())
            );
        }
        return Ok(());
    }

    render_quarantine_entries_text(&matching, args.show_preview)?;
    Ok(())
}

fn resolve_or_generate_admin_token(args: &AdminTokenPersistArgs) -> Result<(String, bool)> {
    if let Some(token) = &args.token {
        ensure!(!token.is_empty(), "--token cannot be empty");
        return Ok((token.clone(), false));
    }

    if let Some(var) = &args.read_env {
        match std::env::var(var) {
            Ok(value) if !value.is_empty() => return Ok((value, false)),
            Ok(_) => bail!("{} is set but empty", var),
            Err(_) => bail!("environment variable {} is not set", var),
        }
    }

    ensure!(args.length > 0, "--length must be greater than zero");
    let mut bytes = vec![0u8; args.length];
    let mut rng = rand::rng();
    rng.fill_bytes(&mut bytes);

    let token = match args.format {
        TokenFormat::Hex => encode_hex(&bytes, args.uppercase),
        TokenFormat::Base64 => base64::engine::general_purpose::STANDARD_NO_PAD.encode(&bytes),
    };

    Ok((token, true))
}

fn persist_admin_token_file(path: &Path, token: &str, hash: Option<&str>) -> Result<bool> {
    ensure!(!token.is_empty(), "token cannot be empty");

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_dir_all(parent)
                .with_context(|| format!("creating parent directory for {}", path.display()))?;
        }
    }

    let existed = path.exists();
    let existing = if existed {
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = existing.lines().map(|line| line.to_string()).collect();
    lines.retain(|line| {
        !is_assignment_for(line, "ARW_ADMIN_TOKEN")
            && !is_assignment_for(line, "ARW_ADMIN_TOKEN_SHA256")
    });

    lines.push(format!("ARW_ADMIN_TOKEN={}", token));
    if let Some(hash) = hash {
        lines.push(format!("ARW_ADMIN_TOKEN_SHA256={}", hash));
    }

    let mut output = lines.join("\n");
    if !output.ends_with('\n') {
        output.push('\n');
    }

    std::fs::write(path, output).with_context(|| format!("writing {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(err) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "warning: failed to set 0600 permissions on {}: {}",
                path.display(),
                err
            );
        }
    }

    Ok(existed)
}

fn is_assignment_for(line: &str, key: &str) -> bool {
    let trimmed = strip_assignment_prefix(line);
    if let Some((lhs, _)) = trimmed.split_once('=') {
        lhs.trim() == key
    } else {
        let mut parts = trimmed.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some(lhs), Some(_)) => lhs.trim() == key,
            _ => false,
        }
    }
}

fn strip_assignment_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    for prefix in ["export ", "EXPORT ", "set ", "SET ", "setx ", "SETX "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest;
        }
    }
    trimmed
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

fn cmd_runtime_bundles_list(args: &RuntimeBundlesListArgs) -> Result<()> {
    if args.remote {
        return runtime_bundles_list_remote(args);
    }

    let snapshot = load_runtime_bundle_snapshot_local(args.dir.clone())?;

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
        "Local runtime bundle inventory (root: {})",
        snapshot
            .roots
            .first()
            .map(|s| s.as_str())
            .unwrap_or("<unknown>")
    );
    print_bundle_summary(
        &snapshot.catalogs,
        &snapshot.roots,
        snapshot.generated.as_deref(),
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
                if let Some(summary_label) = entry
                    .summary
                    .as_ref()
                    .and_then(|s| format_history_summary(s))
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

fn cmd_context_telemetry(args: &ContextTelemetryArgs) -> Result<()> {
    if args.watch {
        eprintln!("watching context telemetry; press Ctrl-C to exit");
        return watch_context_telemetry(args);
    }
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let (status, body) = request_context_telemetry(&client, base, token.as_deref())?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        anyhow::bail!(
            "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access telemetry"
        );
    }
    if !status.is_success() {
        anyhow::bail!("training telemetry request failed: {} {}", status, body);
    }

    if args.json {
        if let Some(path) = args.output.as_ref() {
            append_json_output(path.as_path(), &body, args.pretty, args.output_rotate)?;
        }
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
    if let Some(path) = args.output.as_ref() {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        if let Some(limit) = args.output_rotate {
            append_text_output(path.as_path(), Some(stamp.as_str()), &summary, Some(limit))?;
        } else {
            append_context_summary(path, Some(stamp.as_str()), &summary)?;
        }
    }
    Ok(())
}

fn request_context_telemetry(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<(StatusCode, JsonValue)> {
    let url = format!("{}/state/training/telemetry", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("requesting training telemetry snapshot from {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing training telemetry response")?;
    Ok((status, body))
}

fn watch_context_telemetry(args: &ContextTelemetryArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let base_interval = args.interval.max(1);
    let max_backoff = base_interval.max(60);
    let mut sleep_secs = base_interval;

    loop {
        match request_context_telemetry(&client, base, token.as_deref()) {
            Ok((status, body)) => {
                if status == StatusCode::UNAUTHORIZED {
                    anyhow::bail!(
                        "unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN to access telemetry"
                    );
                }
                if !status.is_success() {
                    eprintln!("[context watch] request failed: {} {}", status, body);
                    sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
                } else {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
                    let summary = render_context_telemetry_summary(&body, now_ms);
                    let stamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                    println!("=== {} ===", stamp);
                    println!("{}", summary.trim_end());
                    println!();
                    io::stdout().flush().ok();
                    if let Some(path) = args.output.as_ref() {
                        if let Some(limit) = args.output_rotate {
                            append_text_output(
                                path.as_path(),
                                Some(stamp.as_str()),
                                &summary,
                                Some(limit),
                            )?;
                        } else {
                            append_context_summary(path, Some(stamp.as_str()), &summary)?;
                        }
                    }
                    sleep_secs = base_interval;
                }
            }
            Err(err) => {
                eprintln!("[context watch] error: {err:?}");
                sleep_secs = sleep_secs.saturating_mul(2).min(max_backoff);
            }
        }

        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

fn append_text_output(
    path: &Path,
    stamp: Option<&str>,
    text: &str,
    rotate_limit: Option<u64>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
        }
    }
    if let Some(limit) = rotate_limit {
        maybe_rotate_output(path, limit)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening output file {}", path.display()))?;
    if let Some(stamp_value) = stamp {
        writeln!(file, "=== {} ===", stamp_value)?;
    }
    writeln!(file, "{}", text.trim_end())?;
    writeln!(file)?;
    Ok(())
}

fn append_json_output(
    path: &Path,
    body: &JsonValue,
    pretty: bool,
    rotate_limit: Option<u64>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            create_dir_all(parent)
                .with_context(|| format!("creating output directory {}", parent.display()))?;
        }
    }
    if let Some(limit) = rotate_limit {
        maybe_rotate_output(path, limit)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening output file {}", path.display()))?;
    let text = if pretty {
        serde_json::to_string_pretty(body)?
    } else {
        body.to_string()
    };
    writeln!(file, "{}", text)?;
    Ok(())
}

fn append_context_summary(path: &Path, stamp: Option<&str>, summary: &str) -> Result<()> {
    append_text_output(path, stamp, summary, None)
}

#[cfg_attr(not(test), allow(dead_code))]
fn append_context_json(path: &Path, body: &JsonValue, pretty: bool) -> Result<()> {
    append_json_output(path, body, pretty, None)
}

fn maybe_rotate_output(path: &Path, max_bytes: u64) -> Result<()> {
    if max_bytes == 0 {
        return Ok(());
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.len() < max_bytes {
        return Ok(());
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    let mut rotated = path.to_path_buf();
    rotated.set_file_name(format!("{}.prev", file_name));
    if rotated.exists() {
        std::fs::remove_file(&rotated).ok();
    }
    std::fs::rename(path, &rotated)
        .with_context(|| format!("rotating output file {}", path.display()))?;
    Ok(())
}

fn parse_byte_limit_arg(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("rotate limit must not be empty".into());
    }
    if trimmed.eq_ignore_ascii_case("0") {
        return Ok(0);
    }
    let digit_count = trimmed
        .chars()
        .position(|c| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (num_part, suffix_part) = trimmed.split_at(digit_count);
    if num_part.is_empty() {
        return Err("rotate limit must start with digits".into());
    }
    let base = num_part
        .parse::<u64>()
        .map_err(|_| "rotate limit digits out of range".to_string())?;
    let suffix = suffix_part.trim().to_ascii_lowercase();
    let multiplier = match suffix.as_str() {
        "" => 1u64,
        "k" | "kb" => 1024,
        "m" | "mb" => 1024 * 1024,
        "g" | "gb" => 1024 * 1024 * 1024,
        "t" | "tb" => 1024u64.pow(4),
        _ => {
            return Err("unsupported rotate suffix (use K, M, G, or T with optional B)".to_string())
        }
    };
    let value = base
        .checked_mul(multiplier)
        .ok_or_else(|| "rotate limit overflow".to_string())?;
    if value != 0 && value < 64 * 1024 {
        Err("rotate limit must be at least 64KB; see CLI docs for details".to_string())
    } else {
        Ok(value)
    }
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

fn cmd_state_identity(args: &StateIdentityArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/identity", base);
    let response = with_admin_headers(
        client.get(&url).header(ACCEPT, "application/json"),
        token.as_deref(),
    )
    .send()
    .with_context(|| format!("requesting identity snapshot from {}", url))?;

    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!(
            "request to {} returned 401 Unauthorized; supply an admin token via --admin-token or ARW_ADMIN_TOKEN",
            url
        );
    }
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read body>".into());
        bail!("request to {} failed ({}): {}", url, status, body);
    }

    let raw: serde_json::Value = response
        .json()
        .context("parsing identity snapshot JSON payload")?;

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

    let snapshot: CliIdentitySnapshot =
        serde_json::from_value(raw).context("materialising identity snapshot structure")?;
    render_identity_snapshot(&snapshot);
    Ok(())
}

fn render_identity_snapshot(snapshot: &CliIdentitySnapshot) {
    println!("Identity registry snapshot");
    let loaded = snapshot
        .loaded_ms
        .map(format_local_timestamp)
        .unwrap_or_else(|| "—".to_string());
    let source = snapshot
        .source_path
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("—");
    let version = snapshot
        .version
        .map(|v| v.to_string())
        .unwrap_or_else(|| "—".into());

    println!("  Loaded : {}", loaded);
    println!("  Source : {}", source);
    println!("  Version: {}", version);
    println!(
        "  Principals: {} (file) · {} (env)",
        snapshot.principals.len(),
        snapshot.env.len()
    );

    if !snapshot.diagnostics.is_empty() {
        println!("\nDiagnostics:");
        for diag in &snapshot.diagnostics {
            println!("  - {}", diag);
        }
    }

    let mut entries: Vec<(&str, &CliIdentityPrincipal)> = Vec::new();
    for principal in &snapshot.principals {
        entries.push(("file", principal));
    }
    for principal in &snapshot.env {
        entries.push(("env", principal));
    }

    if entries.is_empty() {
        println!("\n(no principals loaded)");
        return;
    }

    entries.sort_by(|a, b| a.1.id.cmp(&b.1.id));

    println!(
        "\n{:<4} {:<24} {:<18} {:<28} {:<8} Name / Notes",
        "Src", "ID", "Roles", "Scopes", "Tokens"
    );
    for (source, principal) in entries {
        let id_display = ellipsize_str(&principal.id, 24);
        let roles_display = if principal.roles.is_empty() {
            "—".to_string()
        } else {
            ellipsize_str(&principal.roles.join(", "), 18)
        };
        let scopes_display = if principal.scopes.is_empty() {
            "—".to_string()
        } else {
            ellipsize_str(&principal.scopes.join(", "), 28)
        };
        let tokens_display = principal
            .tokens
            .filter(|count| *count > 0)
            .map(|count| count.to_string())
            .unwrap_or_else(|| "—".into());
        let name_notes = match (
            principal
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            principal
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            (Some(name), Some(note)) => format!("{} · {}", name, note),
            (Some(name), None) => name.to_string(),
            (None, Some(note)) => note.to_string(),
            (None, None) => "—".into(),
        };
        println!(
            "{:<4} {:<24} {:<18} {:<28} {:<8} {}",
            source, id_display, roles_display, scopes_display, tokens_display, name_notes
        );
    }
}

fn resolve_tenants_path(specified: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = specified {
        return Ok(path.clone());
    }
    if let Ok(env_path) = std::env::var("ARW_TENANTS_FILE") {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    if let Some(resolved) = resolve_config_path("configs/security/tenants.toml") {
        return Ok(resolved);
    }
    let paths = effective_paths();
    Ok(PathBuf::from(paths.state_dir).join("security/tenants.toml"))
}

fn load_tenants_file(path: &Path) -> Result<CliTenantsFile> {
    match std::fs::read(path) {
        Ok(bytes) if !bytes.is_empty() => {
            let text = String::from_utf8(bytes)
                .with_context(|| format!("decoding tenants manifest {}", path.display()))?;
            let mut file: CliTenantsFile = toml::from_str(&text)
                .with_context(|| format!("parsing tenants manifest {}", path.display()))?;
            file.version.get_or_insert(1);
            for principal in &mut file.principals {
                dedup_sorted(&mut principal.roles);
                dedup_sorted(&mut principal.scopes);
                dedup_sorted(&mut principal.token_sha256);
            }
            file.principals.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(file)
        }
        Ok(_) => Ok(CliTenantsFile::default()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(CliTenantsFile::default()),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

fn save_tenants_file(path: &Path, file: &mut CliTenantsFile) -> Result<()> {
    file.version.get_or_insert(1);
    for principal in &mut file.principals {
        dedup_sorted(&mut principal.roles);
        dedup_sorted(&mut principal.scopes);
        dedup_sorted(&mut principal.token_sha256);
    }
    file.principals.sort_by(|a, b| a.id.cmp(&b.id));

    if let Some(parent) = path.parent() {
        create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let body = toml::to_string_pretty(file)
        .with_context(|| format!("serializing tenants manifest {}", path.display()))?;
    std::fs::write(path, body)
        .with_context(|| format!("writing tenants manifest {}", path.display()))?;
    Ok(())
}

fn sanitize_identity_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return None;
    }
    if trimmed.starts_with('.') {
        return None;
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '@')))
    {
        return None;
    }
    Some(trimmed.to_string())
}

fn normalize_labels(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .collect();
    dedup_sorted(&mut out);
    out
}

fn is_valid_sha256(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn dedup_sorted(values: &mut Vec<String>) {
    values.sort();
    values.dedup();
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

fn fetch_egress_settings(client: &Client, base: &str, token: Option<&str>) -> Result<JsonValue> {
    let url = format!("{}/state/egress/settings", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing egress settings response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn render_egress_scopes_text(snapshot: &JsonValue) -> Result<()> {
    let egress = snapshot
        .get("egress")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("response missing 'egress' object"))?;

    let bool_field =
        |key: &str| -> bool { egress.get(key).and_then(|v| v.as_bool()).unwrap_or(false) };
    let posture = egress
        .get("posture")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    println!("Posture: {}", posture);
    println!(
        "Ledger: {} | Proxy: {} | DNS guard: {} | Block IP literals: {}",
        if bool_field("ledger_enable") {
            "enabled"
        } else {
            "disabled"
        },
        if bool_field("proxy_enable") {
            "enabled"
        } else {
            "disabled"
        },
        if bool_field("dns_guard_enable") {
            "enabled"
        } else {
            "disabled"
        },
        if bool_field("block_ip_literals") {
            "on"
        } else {
            "off"
        }
    );

    let scopes = egress
        .get("scopes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if scopes.is_empty() {
        println!("\nNo scopes configured.");
        return Ok(());
    }

    println!("\nScopes ({}):", scopes.len());
    for scope in scopes {
        let id = scope
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let description = scope
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let label = if !id.is_empty() {
            id.clone()
        } else if !description.is_empty() {
            description.clone()
        } else {
            "(unnamed)".to_string()
        };
        let expired = scope
            .get("expired")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let status = if expired { "expired" } else { "active" };
        println!("- {} [{}]", label, status);
        if !description.is_empty() && description != label {
            println!("    Description: {}", description);
        }
        let hosts = scope
            .get("hosts")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !hosts.is_empty() {
            println!("    Hosts: {}", hosts.join(", "));
        }
        let cidrs = scope
            .get("cidrs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !cidrs.is_empty() {
            println!("    CIDRs: {}", cidrs.join(", "));
        }
        let ports = scope
            .get("ports")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !ports.is_empty() {
            println!("    Ports: {}", ports.join(", "));
        }
        let protocols = scope
            .get("protocols")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !protocols.is_empty() {
            println!("    Protocols: {}", protocols.join(", "));
        }
        let lease_caps = scope
            .get("lease_capabilities")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !lease_caps.is_empty() {
            println!("    Lease capabilities: {}", lease_caps.join(", "));
        }
        if let Some(expires_at) = scope
            .get("expires_at")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            println!("    Expires at: {}", expires_at);
        }
    }

    Ok(())
}

fn fetch_memory_quarantine_entries(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<Vec<JsonValue>> {
    let url = format!("{}/admin/memory/quarantine", base);
    let resp = with_admin_headers(client.get(&url), token)
        .send()
        .with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp.json().context("parsing memory quarantine response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    match body {
        JsonValue::Array(entries) => Ok(entries),
        other => anyhow::bail!(
            "expected array from /admin/memory/quarantine, received {}",
            other
        ),
    }
}

fn post_memory_quarantine_admit(
    client: &Client,
    base: &str,
    token: Option<&str>,
    payload: &JsonValue,
) -> Result<JsonValue> {
    let url = format!("{}/admin/memory/quarantine/admit", base);
    let resp = with_admin_headers(client.post(&url).json(payload), token)
        .send()
        .with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp
        .json()
        .context("parsing memory quarantine admit response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn parse_quarantine_time(entry: &JsonValue) -> Option<chrono::DateTime<chrono::Utc>> {
    let time_str = entry.get("time")?.as_str()?;
    chrono::DateTime::parse_from_rfc3339(time_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
}

fn format_timestamp_local(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| raw.to_string())
}

fn render_quarantine_entries_text(entries: &[JsonValue], show_preview: bool) -> Result<()> {
    if entries.is_empty() {
        println!("No quarantine entries.");
        return Ok(());
    }

    let mut state_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for entry in entries {
        let state = entry
            .get("state")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        *state_counts.entry(state.to_string()).or_insert(0) += 1;
    }
    let mut state_summary: Vec<String> = state_counts
        .into_iter()
        .map(|(state, count)| format!("{} {}", state, count))
        .collect();
    state_summary.sort();
    println!(
        "Summary: total {} | {}",
        entries.len(),
        state_summary.join(" | ")
    );

    println!(
        "{:<32} {:<14} {:<12} {:>6} {:<19} {:<12} Markers",
        "ID", "State", "Source", "Score", "When", "Project"
    );

    for entry in entries {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("-");
        let state = entry.get("state").and_then(|v| v.as_str()).unwrap_or("-");
        let source = entry.get("source").and_then(|v| v.as_str()).unwrap_or("-");
        let score = entry
            .get("evidence_score")
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
            .map(|v| format!("{:.2}", v))
            .unwrap_or_else(|| "-".into());
        let time_display = entry
            .get("time")
            .and_then(|v| v.as_str())
            .map(format_timestamp_local)
            .unwrap_or_else(|| "-".into());
        let project = entry
            .get("project_id")
            .and_then(|v| v.as_str())
            .unwrap_or("-");
        let markers = entry
            .get("risk_markers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "-".into());

        println!(
            "{:<32} {:<14} {:<12} {:>6} {:<19} {:<12} {}",
            truncate_payload(id, 31),
            truncate_payload(state, 13),
            truncate_payload(source, 11),
            score,
            time_display,
            truncate_payload(project, 11),
            truncate_payload(&markers, 40)
        );

        let mut meta = Vec::new();
        if let Some(ep) = entry
            .get("episode_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            meta.push(format!("episode {}", ep));
        }
        if let Some(corr) = entry
            .get("corr_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            meta.push(format!("corr {}", corr));
        }
        if !meta.is_empty() {
            println!("    {}", meta.join(" | "));
        }
        if let Some(prov) = entry
            .get("provenance")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            println!("    provenance: {}", truncate_payload(prov, 120));
        }
        if show_preview {
            if let Some(preview) = entry
                .get("content_preview")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                println!("    preview: {}", truncate_payload(preview, 200));
            }
        }
        if let Some(review) = entry.get("review").and_then(|v| v.as_object()) {
            let mut review_parts = Vec::new();
            if let Some(decision) = review
                .get("decision")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                review_parts.push(decision.to_string());
            }
            if let Some(by) = review
                .get("by")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                review_parts.push(format!("by {}", by));
            }
            if let Some(time) = review
                .get("time")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                review_parts.push(format!("at {}", format_timestamp_local(time)));
            }
            if let Some(note) = review
                .get("note")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                review_parts.push(format!("note: {}", note));
            }
            if !review_parts.is_empty() {
                println!("    review: {}", review_parts.join(", "));
            }
        }
    }

    Ok(())
}

fn render_quarantine_entries_ndjson(entries: &[JsonValue]) -> Result<()> {
    for entry in entries {
        println!("{}", serde_json::to_string(entry)?);
    }
    Ok(())
}

fn render_quarantine_entries_csv(entries: &[JsonValue]) -> Result<()> {
    let mut writer = WriterBuilder::new().from_writer(io::stdout());
    writer.write_record([
        "id",
        "state",
        "source",
        "evidence_score",
        "time",
        "project_id",
        "episode_id",
        "corr_id",
        "risk_markers",
        "provenance",
        "review_decision",
        "review_by",
        "review_time",
        "review_note",
    ])?;

    for entry in entries {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let state = entry.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let source = entry.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let score = entry
            .get("evidence_score")
            .and_then(|v| v.as_f64())
            .map(|v| format!("{:.4}", v))
            .unwrap_or_default();
        let time = entry.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let project = entry
            .get("project_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let episode = entry
            .get("episode_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let corr = entry.get("corr_id").and_then(|v| v.as_str()).unwrap_or("");
        let markers = entry
            .get("risk_markers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(";")
            })
            .unwrap_or_default();
        let provenance = entry
            .get("provenance")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let review = entry.get("review").and_then(|v| v.as_object());
        let (review_decision, review_by, review_time, review_note) = review
            .map(|obj| {
                (
                    obj.get("decision")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    obj.get("by")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    obj.get("time")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    obj.get("note")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                )
            })
            .unwrap_or_else(|| (String::new(), String::new(), String::new(), String::new()));

        writer.write_record([
            id,
            state,
            source,
            &score,
            time,
            project,
            episode,
            corr,
            &markers,
            provenance,
            &review_decision,
            &review_by,
            &review_time,
            &review_note,
        ])?;
    }

    writer.flush()?;
    Ok(())
}

fn post_egress_settings(
    client: &Client,
    base: &str,
    token: Option<&str>,
    payload: &JsonValue,
) -> Result<JsonValue> {
    let url = format!("{}/egress/settings", base);
    let mut req = client.post(&url).json(payload);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("posting {}", url))?;
    let status = resp.status();
    let body: JsonValue = resp
        .json()
        .context("parsing egress settings update response")?;
    if status == StatusCode::UNAUTHORIZED {
        anyhow::bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        anyhow::bail!("server returned {}: {}", status, body);
    }
    Ok(body)
}

fn extract_scopes(snapshot: &JsonValue) -> Result<Vec<JsonValue>> {
    let scopes = snapshot
        .get("egress")
        .and_then(|v| v.get("scopes"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(scopes)
}

fn sanitize_hosts(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| s.trim().trim_end_matches('.').to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn sanitize_list(list: &[String]) -> Vec<String> {
    list.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn sanitize_ports(list: &[u16]) -> Vec<u16> {
    let mut ports: Vec<u16> = list.to_vec();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn normalize_protocols(list: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for item in list {
        let lower = item.trim().to_ascii_lowercase();
        if lower.is_empty() {
            continue;
        }
        match lower.as_str() {
            "http" | "https" | "tcp" => {
                if !out.contains(&lower) {
                    out.push(lower);
                }
            }
            other => bail!("invalid protocol '{}'; use http, https, or tcp", other),
        }
    }
    Ok(out)
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
        push_filter_str(&mut filters, "prefix=", args.kind_prefix.as_deref());
        push_filter_usize(&mut filters, "limit=", args.limit);
        if let Some(ref label) = since_resolution.relative_display {
            if !label.is_empty() {
                filters.push(label.clone());
            }
        }
        if let Some(ref label) = since_resolution.display {
            if !label.is_empty() {
                filters.push(label.clone());
            }
        }
        if args.payload_width == 0 {
            filters.push("payload hidden".to_string());
        } else if args.payload_width != 120 {
            filters.push(format!("payload_width={}", args.payload_width));
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

    println!("{:<28} {:<10} {:<36} Payload", "Time", "Age", "Kind");

    let now_utc = Utc::now();

    for item in items {
        let time_raw = item.get("time").and_then(|v| v.as_str()).unwrap_or("");
        let when = if time_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(time_raw)
        };
        let age_display = if time_raw.is_empty() {
            "-".to_string()
        } else {
            format_elapsed_since_with_now(time_raw, now_utc).unwrap_or_else(|| "-".to_string())
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
            "{:<28} {:<10} {:<36} {}{}",
            when, age_display, kind_display, payload_display, extra_str
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
        .get(format!("{}/events", base))
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

fn cmd_state_cluster(args: &StateClusterArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/state/cluster", base);
    let response = with_admin_headers(
        client.get(&url).header(ACCEPT, "application/json"),
        token.as_deref(),
    )
    .send()
    .with_context(|| format!("requesting cluster snapshot from {}", url))?;

    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        bail!(
            "request to {} returned 401 Unauthorized; supply an admin token via --admin-token or ARW_ADMIN_TOKEN",
            url
        );
    }
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<unable to read body>".into());
        bail!("request to {} failed ({}): {}", url, status, body);
    }

    let raw: serde_json::Value = response
        .json()
        .context("parsing cluster snapshot JSON payload")?;

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

    let snapshot: CliClusterSnapshot =
        serde_json::from_value(raw).context("materialising cluster snapshot structure")?;
    render_cluster_snapshot(&snapshot);
    Ok(())
}

fn render_cluster_snapshot(snapshot: &CliClusterSnapshot) {
    println!("Cluster snapshot");
    let generated = snapshot
        .generated_ms
        .map(format_local_timestamp)
        .or_else(|| snapshot.generated.clone())
        .unwrap_or_else(|| "—".to_string());
    let ttl = snapshot.ttl_seconds.unwrap_or(0);
    println!("  Generated : {}", generated);
    if ttl > 0 {
        println!("  TTL       : {}s", ttl);
    } else {
        println!("  TTL       : —");
    }
    println!("  Nodes     : {}", snapshot.nodes.len());

    if snapshot.nodes.is_empty() {
        println!("\n  (no nodes advertised)");
        return;
    }

    println!();
    println!(
        "{:<20} {:<10} {:<10} {:<32} {:<6} {:<8} {}",
        "ID", "Role", "Health", "Last Seen", "Stale", "Models", "Capabilities"
    );

    let now_raw = Utc::now().timestamp_millis();
    let now_ms = if now_raw < 0 { 0 } else { now_raw as u64 };
    let ttl_ms = snapshot.ttl_seconds.unwrap_or(0).saturating_mul(1_000);

    for node in &snapshot.nodes {
        let id_display = if let Some(name) = node.name.as_deref() {
            if !name.is_empty() {
                format!("{} ({})", node.id, name)
            } else {
                node.id.clone()
            }
        } else {
            node.id.clone()
        };
        let role = node.role.to_lowercase();
        let health = node.health.clone().unwrap_or_else(|| "—".into());
        let last_seen_ms = node.last_seen_ms.unwrap_or(0);
        let base_last = node
            .last_seen_ms
            .map(format_local_timestamp)
            .or_else(|| node.last_seen.clone())
            .unwrap_or_else(|| "—".to_string());
        let last_seen = if last_seen_ms > 0 && now_ms > 0 {
            format!(
                "{} ({})",
                base_last,
                format_relative_from_now(last_seen_ms, now_ms)
            )
        } else {
            base_last
        };
        let stale = if ttl_ms == 0 || last_seen_ms == 0 {
            "no"
        } else if now_ms > last_seen_ms {
            if now_ms - last_seen_ms > ttl_ms {
                "yes"
            } else {
                "no"
            }
        } else {
            "no"
        };
        let models = summarize_models_field(&node.models);
        let capabilities = summarize_capabilities_field(&node.capabilities);

        println!(
            "{:<20} {:<10} {:<10} {:<32} {:<6} {:<8} {}",
            truncate_pad(&id_display, 20),
            truncate_pad(&role, 10),
            truncate_pad(&health, 10),
            truncate_pad(&last_seen, 32),
            stale,
            truncate_pad(&models, 8),
            truncate_pad(&capabilities, 40)
        );
    }
}

fn summarize_capabilities_field(raw: &Option<JsonValue>) -> String {
    let Some(value) = raw else {
        return "—".into();
    };
    match value {
        JsonValue::Object(map) => {
            if map.is_empty() {
                return "—".into();
            }
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            let rendered: Vec<&str> = keys.into_iter().take(3).collect();
            let mut out = rendered.join(",");
            if map.len() > rendered.len() {
                if !out.is_empty() {
                    out.push_str(",…");
                } else {
                    out.push('…');
                }
            }
            if out.is_empty() {
                "—".into()
            } else {
                out
            }
        }
        JsonValue::Array(items) => {
            if items.is_empty() {
                return "—".into();
            }
            let mut chunks = Vec::new();
            for v in items.iter().take(3) {
                match v {
                    JsonValue::String(s) => chunks.push(s.as_str()),
                    _ => continue,
                }
            }
            if chunks.is_empty() {
                "—".into()
            } else {
                let mut out = chunks.join(",");
                if items.len() > chunks.len() {
                    out.push_str(",…");
                }
                out
            }
        }
        JsonValue::String(s) => {
            if s.is_empty() {
                "—".into()
            } else {
                s.clone()
            }
        }
        other => other.to_string(),
    }
}

fn summarize_models_field(raw: &Option<JsonValue>) -> String {
    let Some(value) = raw else {
        return "—".into();
    };
    match value {
        JsonValue::Object(map) => {
            if let Some(count) = map.get("count").and_then(JsonValue::as_u64) {
                let mut out = count.to_string();
                if let Some(preview) = map.get("preview").and_then(JsonValue::as_array) {
                    if !preview.is_empty() {
                        let mut tags = Vec::new();
                        for entry in preview.iter().take(2) {
                            if let Some(s) = entry.as_str() {
                                tags.push(shorten_hash(s));
                            }
                        }
                        if !tags.is_empty() {
                            out.push(' ');
                            out.push('(');
                            out.push_str(&tags.join(","));
                            if preview.len() > tags.len() {
                                out.push_str(",…");
                            }
                            out.push(')');
                        }
                    }
                }
                out
            } else {
                "—".into()
            }
        }
        JsonValue::Number(num) => num.to_string(),
        JsonValue::String(s) => {
            if s.is_empty() {
                "—".into()
            } else {
                s.clone()
            }
        }
        _ => "—".into(),
    }
}

fn shorten_hash(input: &str) -> String {
    if input.len() <= 8 {
        input.to_string()
    } else {
        input[..8].to_string()
    }
}

fn truncate_pad(input: &str, width: usize) -> String {
    if input.len() <= width {
        let mut s = input.to_string();
        if s.len() < width {
            s.push_str(&" ".repeat(width - s.len()));
        }
        s
    } else {
        let mut out = input[..width.saturating_sub(1)].to_string();
        out.push('…');
        out
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
        let updated_since = resolve_updated_since(args)?;
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
        let mut filters: Vec<String> = Vec::new();
        push_filter_str(&mut filters, "state=", args.state.as_deref());
        push_filter_str(&mut filters, "kind_prefix=", args.kind_prefix.as_deref());
        push_filter_usize(&mut filters, "limit=", args.limit);
        push_filter_str(
            &mut filters,
            "updated_relative=",
            args.updated_relative.as_deref(),
        );
        push_filter_str(&mut filters, "updated>", args.updated_since.as_deref());
        if !filters.is_empty() {
            println!("Filters: {}", filters.join(", "));
        }
    }

    if items.is_empty() {
        println!("(no actions matched the filters)");
        return Ok(());
    }

    let kind_width = args.kind_width.max(8);
    println!(
        "{:<28} {:<10} {:<10} {:<width$} Id",
        "Updated",
        "Age",
        "State",
        "Kind",
        width = kind_width
    );

    let now_utc = Utc::now();

    for item in items {
        let updated_raw = item.get("updated").and_then(|v| v.as_str()).unwrap_or("");
        let updated_display = if updated_raw.is_empty() {
            "-".to_string()
        } else {
            format_observation_timestamp(updated_raw)
        };
        let age_display = if updated_raw.is_empty() {
            "-".to_string()
        } else {
            format_elapsed_since_with_now(updated_raw, now_utc).unwrap_or_else(|| "-".to_string())
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
            "{:<28} {:<10} {:<10} {:<width$} {}",
            updated_display,
            age_display,
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
        .get(format!("{}/events", base))
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

fn resolve_after_timestamp(
    args: &EventsJournalArgs,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    resolve_after_timestamp_with_now(
        args.after_cursor.as_deref(),
        args.after_relative.as_deref(),
        chrono::Utc::now(),
    )
}

fn resolve_after_timestamp_with_now(
    absolute: Option<&str>,
    relative: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    if let Some(raw) = relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--after-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        return Ok(Some(now - duration));
    }

    if let Some(cursor) = absolute {
        let trimmed = cursor.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--after cannot be empty");
        }
        return match chrono::DateTime::parse_from_rfc3339(trimmed) {
            Ok(dt) => Ok(Some(dt.with_timezone(&chrono::Utc))),
            Err(_) => {
                anyhow::bail!("--after must be an RFC3339 timestamp (e.g. 2025-10-02T17:15:00Z)")
            }
        };
    }

    Ok(None)
}

fn resolve_updated_since(args: &StateActionsArgs) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    resolve_updated_since_with_now(
        args.updated_since.as_deref(),
        args.updated_relative.as_deref(),
        chrono::Utc::now(),
    )
}

fn resolve_updated_since_with_now(
    absolute: Option<&str>,
    relative: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    if let Some(raw) = relative {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--updated-relative requires a value such as 15m or 2h");
        }
        let duration = parse_relative_duration(trimmed)?;
        return Ok(Some(now - duration));
    }

    if let Some(raw) = absolute {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("--updated-since cannot be empty");
        }
        let parsed = chrono::DateTime::parse_from_rfc3339(trimmed)
            .with_context(|| format!("failed to parse updated_since='{}'", trimmed))?;
        return Ok(Some(parsed.with_timezone(&chrono::Utc)));
    }

    Ok(None)
}

fn cmd_events_journal(args: &EventsJournalArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');

    let after_time = resolve_after_timestamp(args)?;

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

    let mut filter_summaries: Vec<String> = Vec::new();
    push_filter_str(&mut filter_summaries, "prefix=", args.prefix.as_deref());
    push_filter_str(
        &mut filter_summaries,
        "after_relative=",
        args.after_relative.as_deref(),
    );
    push_filter_str(
        &mut filter_summaries,
        "after>",
        args.after_cursor.as_deref(),
    );
    if args.limit != 200 {
        filter_summaries.push(format!("limit={}", args.limit));
    }
    if args.payload_width == 0 {
        filter_summaries.push("payload hidden".to_string());
    } else if args.payload_width != 160 {
        filter_summaries.push(format!("payload_width={}", args.payload_width));
    }
    if args.after_relative.is_some() {
        if let Some(ref cursor) = after_time {
            filter_summaries.push(format!(
                "after>= {}",
                cursor.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            ));
        }
    }
    if !filter_summaries.is_empty() {
        println!("Filters: {}", filter_summaries.join(", "));
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
            args.payload_width,
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

fn cmd_events_modular(args: &ModularTailArgs) -> Result<()> {
    let mut journal = args.journal.clone();
    if journal
        .prefix
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        journal.prefix = Some("modular.".to_string());
    }
    if !journal.json {
        journal.follow = true;
        if journal.interval == 5 {
            journal.interval = 3;
        }
        if journal.payload_width == 160 {
            journal.payload_width = 200;
        }
    }
    if journal.limit == 200 {
        journal.limit = 100;
    }
    cmd_events_journal(&journal)
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
        anyhow::bail!("journal disabled: ensure the server runs with ARW_KERNEL_ENABLE=1");
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
    payload_width: usize,
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
    let now_utc = chrono::Utc::now();
    for (entry, key) in printable {
        let time_raw = entry.get("time").and_then(|v| v.as_str());
        let kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let (time_display, age_display) = if let Some(raw) = time_raw {
            let display = format_observation_timestamp(raw);
            let age =
                format_elapsed_since_with_now(raw, now_utc).unwrap_or_else(|| "-".to_string());
            (display, age)
        } else {
            ("-".to_string(), "-".to_string())
        };
        println!("[{} | {}] {}", time_display, age_display, kind);
        if payload_width > 0 {
            let payload = entry.get("payload").cloned().unwrap_or(JsonValue::Null);
            let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "null".into());
            println!(
                "  payload: {}",
                truncate_payload(&payload_str, payload_width)
            );
            if let Some(policy) = entry.get("policy") {
                if !policy.is_null() {
                    let policy_str = serde_json::to_string(policy).unwrap_or_else(|_| "{}".into());
                    println!("  policy: {}", truncate_payload(&policy_str, payload_width));
                }
            }
            if let Some(ce) = entry.get("ce") {
                if !ce.is_null() {
                    let ce_str = serde_json::to_string(ce).unwrap_or_else(|_| "{}".into());
                    println!("  ce: {}", truncate_payload(&ce_str, payload_width));
                }
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

fn render_tool_cache_summary(stats: &ToolCacheSnapshot, base: &str) -> String {
    let mut buf = String::new();
    let _ = writeln!(buf, "Tool cache @ {}", base);
    let limit_fragment = match stats.max_payload_bytes {
        Some(limit) => format!("limit {}", format_bytes(limit)),
        None => "limit off".to_string(),
    };
    if stats.capacity == 0 {
        let _ = writeln!(
            buf,
            "- status: disabled | capacity 0 | ttl {}s | entries {} | {}",
            stats.ttl_secs, stats.entries, limit_fragment
        );
    } else {
        let _ = writeln!(
            buf,
            "- status: enabled | capacity {} | ttl {}s | entries {} | {}",
            stats.capacity, stats.ttl_secs, stats.entries, limit_fragment
        );
    }

    let mut outcome_parts = vec![
        format!("hit {}", stats.hit),
        format!("miss {}", stats.miss),
        format!("coalesced {}", stats.coalesced),
        format!("bypass {}", stats.bypass),
        format!("errors {}", stats.errors),
    ];
    if stats.payload_too_large > 0 {
        outcome_parts.push(format!("payload>limit {}", stats.payload_too_large));
    }
    let mut outcomes = format!("- outcomes: {}", outcome_parts.join(" | "));
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

fn format_observation_timestamp(raw: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(raw) {
        Ok(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S%.3f %Z")
            .to_string(),
        Err(_) => raw.to_string(),
    }
}

fn format_elapsed_since_with_now(raw: &str, now: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw).ok()?;
    let delta = now - parsed.with_timezone(&Utc);
    let seconds = delta.num_seconds().max(0);
    Some(format_compact_duration(seconds))
}

fn format_compact_duration(total_seconds: i64) -> String {
    let seconds = total_seconds.max(0);
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        if hours > 0 {
            format!("{}d{:02}h", days, hours)
        } else {
            format!("{}d", days)
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{}h{:02}m", hours, minutes)
        } else {
            format!("{}h", hours)
        }
    } else if minutes > 0 {
        if secs > 0 {
            format!("{}m{:02}s", minutes, secs)
        } else {
            format!("{}m", minutes)
        }
    } else {
        format!("{}s", secs)
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

fn push_filter_str(filters: &mut Vec<String>, label: &str, value: Option<&str>) {
    if let Some(raw) = value {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            filters.push(format!("{label}{trimmed}"));
        }
    }
}

fn push_filter_usize(filters: &mut Vec<String>, label: &str, value: Option<usize>) {
    if let Some(v) = value {
        filters.push(format!("{label}{v}"));
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
    fn resolve_after_timestamp_handles_relative_window() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let resolved = resolve_after_timestamp_with_now(None, Some("15m"), now)
            .expect("relative after timestamp")
            .expect("timestamp");
        assert_eq!(resolved, now - chrono::Duration::minutes(15));
    }

    #[test]
    fn resolve_after_timestamp_handles_absolute_cursor() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let target = "2025-10-02T11:59:00Z";
        let resolved = resolve_after_timestamp_with_now(Some(target), None, now)
            .expect("absolute after timestamp")
            .expect("timestamp");
        let expected = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 11, 59, 0)
            .single()
            .expect("construct expected timestamp");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_after_timestamp_rejects_empty_inputs() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        assert!(resolve_after_timestamp_with_now(Some("  \t"), None, now).is_err());
        assert!(resolve_after_timestamp_with_now(None, Some(""), now).is_err());
    }

    #[test]
    fn resolve_updated_since_handles_relative_window() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let resolved = resolve_updated_since_with_now(None, Some("45m"), now)
            .expect("relative updated timestamp")
            .expect("timestamp");
        assert_eq!(resolved, now - chrono::Duration::minutes(45));
    }

    #[test]
    fn resolve_updated_since_handles_absolute_cursor() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let target = "2025-10-02T11:30:00Z";
        let resolved = resolve_updated_since_with_now(Some(target), None, now)
            .expect("absolute updated timestamp")
            .expect("timestamp");
        let expected = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 11, 30, 0)
            .single()
            .expect("construct expected timestamp");
        assert_eq!(resolved, expected);
    }

    #[test]
    fn resolve_updated_since_rejects_empty_inputs() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        assert!(resolve_updated_since_with_now(Some("   "), None, now).is_err());
        assert!(resolve_updated_since_with_now(None, Some(""), now).is_err());
    }

    #[test]
    fn format_compact_duration_handles_units() {
        assert_eq!(format_compact_duration(42), "42s");
        assert_eq!(format_compact_duration(125), "2m05s");
        assert_eq!(format_compact_duration(3600), "1h");
        assert_eq!(format_compact_duration(3700), "1h01m");
        assert_eq!(format_compact_duration(86_400), "1d");
        assert_eq!(format_compact_duration(86_400 + 7_200), "1d02h");
    }

    #[test]
    fn format_elapsed_since_with_now_clamps_future() {
        let now = chrono::Utc
            .with_ymd_and_hms(2025, 10, 2, 12, 0, 0)
            .single()
            .expect("construct chrono datetime");
        let future = "2025-10-02T12:05:00Z";
        let formatted = format_elapsed_since_with_now(future, now).expect("formatted");
        assert_eq!(formatted, "0s");
    }

    #[test]
    fn push_filter_str_trims_and_skips_empty() {
        let mut filters = Vec::new();
        push_filter_str(&mut filters, "state=", Some(" queued "));
        assert_eq!(filters, vec!["state=queued".to_string()]);

        push_filter_str(&mut filters, "state=", Some("   "));
        assert_eq!(filters, vec!["state=queued".to_string()]);

        push_filter_str(&mut filters, "state=", None);
        assert_eq!(filters, vec!["state=queued".to_string()]);
    }

    #[test]
    fn push_filter_usize_records_values() {
        let mut filters = Vec::new();
        push_filter_usize(&mut filters, "limit=", Some(25));
        assert_eq!(filters, vec!["limit=25".to_string()]);

        push_filter_usize(&mut filters, "limit=", None);
        assert_eq!(filters, vec!["limit=25".to_string()]);
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
    fn append_context_summary_creates_dirs_and_appends() {
        let dir = TempDir::new().expect("tempdir");
        let log_path = dir.path().join("logs/2025-10-02/context.log");

        append_context_summary(&log_path, Some("2025-10-02 12:00:00"), "First run").unwrap();
        append_context_summary(&log_path, None, "Second run").unwrap();

        let contents = fs::read_to_string(&log_path).expect("read log");
        assert_eq!(
            contents,
            "=== 2025-10-02 12:00:00 ===\nFirst run\n\nSecond run\n\n"
        );
    }

    #[test]
    fn append_context_json_respects_pretty_flag() {
        let dir = TempDir::new().expect("tempdir");
        let log_path = dir.path().join("logs/context.jsonl");
        let payload = json!({"hello": "world"});

        append_context_json(&log_path, &payload, false).unwrap();
        append_context_json(&log_path, &payload, true).unwrap();

        let contents = fs::read_to_string(&log_path).expect("read log");
        let mut lines = contents.lines();
        assert_eq!(lines.next().unwrap().trim(), "{\"hello\":\"world\"}");
        let pretty_block = lines.collect::<Vec<_>>().join("\n");
        assert!(pretty_block.contains("\"hello\": \"world\""));
    }

    #[test]
    fn append_text_output_rotates_when_limit_reached() -> Result<()> {
        let dir = TempDir::new()?;
        let log_path = dir.path().join("context.log");
        fs::write(&log_path, vec![b'x'; 16])?;

        append_text_output(&log_path, Some("2025-10-02 12:00:00"), "New entry", Some(8))?;

        let rotated = dir.path().join("context.log.prev");
        assert!(rotated.is_file());
        assert_eq!(fs::read(rotated)?, vec![b'x'; 16]);

        let fresh = fs::read_to_string(&log_path)?;
        assert!(fresh.contains("2025-10-02 12:00:00"));
        assert!(fresh.contains("New entry"));
        Ok(())
    }

    #[test]
    fn append_text_output_respects_no_rotation() -> Result<()> {
        let dir = TempDir::new()?;
        let log_path = dir.path().join("context.log");

        append_text_output(&log_path, Some("stamp"), "entry", None)?;
        append_text_output(&log_path, Some("stamp-2"), "entry-2", None)?;

        let updated = fs::read_to_string(&log_path)?;
        assert!(updated.contains("entry"));
        assert!(updated.contains("entry-2"));
        assert!(!dir.path().join("context.log.prev").exists());
        Ok(())
    }

    #[test]
    fn parse_byte_limit_arg_supports_suffixes() {
        assert_eq!(parse_byte_limit_arg("64KB").unwrap(), 64 * 1024);
        assert_eq!(parse_byte_limit_arg("3m").unwrap(), 3 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("4MB").unwrap(), 4 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("5G").unwrap(), 5 * 1024 * 1024 * 1024);
        assert_eq!(parse_byte_limit_arg("0").unwrap(), 0);
    }

    #[test]
    fn parse_byte_limit_arg_rejects_invalid() {
        assert!(parse_byte_limit_arg("").is_err());
        assert!(parse_byte_limit_arg("kb").is_err());
        assert!(parse_byte_limit_arg("2KB").is_err());
        assert!(parse_byte_limit_arg("63KB").is_err());
        assert!(parse_byte_limit_arg("10x").is_err());
        assert!(parse_byte_limit_arg("1000000000000000000000000000000").is_err());
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
    fn persist_admin_token_file_overwrites_existing_entries() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join(".env");
        fs::write(
            &path,
            "# sample env\nFOO=bar\nARW_ADMIN_TOKEN=old\nexport ARW_ADMIN_TOKEN_SHA256=oldhash\nBAR=baz\n",
        )
        .expect("seed env file");

        let existed =
            persist_admin_token_file(&path, "newtoken", Some("newhash")).expect("persist token");
        assert!(existed);

        let contents = fs::read_to_string(&path).expect("read env");
        assert_eq!(
            contents,
            "# sample env\nFOO=bar\nBAR=baz\nARW_ADMIN_TOKEN=newtoken\nARW_ADMIN_TOKEN_SHA256=newhash\n"
        );
    }

    #[test]
    fn persist_admin_token_file_creates_when_missing() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("config/.env");

        let existed = persist_admin_token_file(&path, "secret", None).expect("persist token");
        assert!(!existed);

        let contents = fs::read_to_string(&path).expect("read env");
        assert_eq!(contents, "ARW_ADMIN_TOKEN=secret\n");
    }

    #[test]
    fn is_assignment_for_handles_prefixes() {
        assert!(is_assignment_for(
            "export ARW_ADMIN_TOKEN=value",
            "ARW_ADMIN_TOKEN"
        ));
        assert!(is_assignment_for(
            " SET ARW_ADMIN_TOKEN = value ",
            "ARW_ADMIN_TOKEN"
        ));
        assert!(is_assignment_for(
            "setx ARW_ADMIN_TOKEN value",
            "ARW_ADMIN_TOKEN"
        ));
        assert!(!is_assignment_for("export OTHER=value", "ARW_ADMIN_TOKEN"));
        assert!(!is_assignment_for(
            "ARW_ADMIN_TOKEN_SUFFIX=1",
            "ARW_ADMIN_TOKEN"
        ));
    }

    #[test]
    fn tool_cache_summary_includes_key_metrics() {
        let snapshot = ToolCacheSnapshot {
            hit: 8,
            miss: 2,
            coalesced: 3,
            errors: 1,
            bypass: 4,
            payload_too_large: 2,
            capacity: 128,
            ttl_secs: 600,
            entries: 42,
            max_payload_bytes: Some(1_048_576),
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
        assert!(summary.contains("limit 1.0 MB"));
        assert!(summary.contains("payload>limit 2"));
    }
}
