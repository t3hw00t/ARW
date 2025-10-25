use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::fs::{create_dir_all, read_to_string, File};
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::Engine;
use chrono::Duration as ChronoDuration;
use chrono::{DateTime, Local, TimeZone, Utc};
use clap::{Args, Subcommand, ValueEnum};
use csv::WriterBuilder;
use rand::RngCore;
use reqwest::{blocking::Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::Digest;

use super::util::{resolve_admin_token, resolve_persona_id, truncate_payload, with_admin_headers};
use crate::commands::state::{render_identity_snapshot, CliIdentityPrincipal, CliIdentitySnapshot};
use arw_core::{effective_paths, resolve_config_path};
use arw_kernel::{Kernel, PersonaEntry, PersonaEntryUpsert};

#[derive(Subcommand)]
pub enum AdminCmd {
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
    /// Autonomy lane controls
    Autonomy {
        #[command(subcommand)]
        cmd: AdminAutonomyCmd,
    },
    /// Persona helpers
    Persona {
        #[command(subcommand)]
        cmd: AdminPersonaCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum AdminAutonomyCmd {
    /// List autonomy lanes
    Lanes(AdminAutonomyListArgs),
    /// Show details for a specific lane
    Lane(AdminAutonomyShowArgs),
    /// Pause a lane (halts scheduling and running jobs)
    Pause(AdminAutonomyActionArgs),
    /// Resume a lane (guided or autonomous mode)
    Resume(AdminAutonomyResumeArgs),
    /// Stop a lane and flush queued + in-flight jobs
    Stop(AdminAutonomyActionArgs),
    /// Flush queued or running jobs without changing mode
    Flush(AdminAutonomyFlushArgs),
    /// Update or clear lane budgets
    Budgets(AdminAutonomyBudgetsArgs),
    /// Reset the engagement ledger for a lane
    EngagementReset(AdminAutonomyEngagementArgs),
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum AdminPersonaCmd {
    /// Grant a persona:manage lease
    Grant(AdminPersonaGrantArgs),
    /// Seed or update a persona entry directly in the state store
    Seed(AdminPersonaSeedArgs),
}

#[derive(Args, Clone)]
pub(crate) struct AdminPersonaGrantArgs {
    /// Base URL (e.g. http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Lease scope (optional)
    #[arg(long)]
    scope: Option<String>,
    /// Lease budget (optional)
    #[arg(long)]
    budget: Option<f64>,
    /// Lease lifetime in seconds (default: 1h)
    #[arg(long, default_value_t = 3600)]
    ttl_secs: u64,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
pub(crate) struct AdminPersonaSeedArgs {
    /// Persona identifier (default: persona.alpha)
    #[arg(long, default_value = "persona.alpha")]
    id: String,
    /// Persona owner kind (workspace | project | agent)
    #[arg(long, default_value = "workspace")]
    owner_kind: String,
    /// Persona owner reference (workspace/project/agent id). Fetched from /state/identity when omitted.
    #[arg(long)]
    owner_ref: Option<String>,
    /// Optional display name.
    #[arg(long)]
    name: Option<String>,
    /// Optional archetype label.
    #[arg(long)]
    archetype: Option<String>,
    /// Traits JSON (inline or @file path). Defaults to {}.
    #[arg(long)]
    traits: Option<String>,
    /// Preferences JSON (inline or @file path). Defaults to {}.
    #[arg(long)]
    preferences: Option<String>,
    /// Worldview JSON (inline or @file path). Defaults to {}.
    #[arg(long)]
    worldview: Option<String>,
    /// Vibe profile JSON (inline or @file path). Defaults to {}.
    #[arg(long)]
    vibe_profile: Option<String>,
    /// Calibration JSON (inline or @file path). Defaults to {}.
    #[arg(long)]
    calibration: Option<String>,
    /// Set telemetry.vibe.enabled = true (optional scope with --telemetry-scope)
    #[arg(long)]
    enable_telemetry: bool,
    /// Scope applied when --enable-telemetry is set (default: owner kind)
    #[arg(long)]
    telemetry_scope: Option<String>,
    /// Base URL used to resolve owner_ref when --owner-ref is omitted (default: local server)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token for /state/identity fallback (optional).
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Override the state directory (defaults to effective state path)
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// Emit the resulting persona entry as JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Subcommand)]
pub(crate) enum AdminEgressCmd {
    /// Show configured egress scopes from /state/egress/settings
    Scopes(AdminEgressScopesArgs),
    /// Manage individual egress scopes
    Scope {
        #[command(subcommand)]
        cmd: AdminEgressScopeCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum AdminEgressScopeCmd {
    /// Create a new scope or fail if the id already exists
    Add(AdminEgressScopeAddArgs),
    /// Update an existing scope by id
    Update(AdminEgressScopeUpdateArgs),
    /// Remove a scope by id
    Remove(AdminEgressScopeRemoveArgs),
}

#[derive(Subcommand)]
pub(crate) enum AdminTokenCmd {
    /// Hash an admin token for ARW_ADMIN_TOKEN_SHA256
    Hash(AdminTokenHashArgs),
    /// Generate a random admin token
    Generate(AdminTokenGenerateArgs),
    /// Persist an admin token (and optional hash) to an env file
    Persist(AdminTokenPersistArgs),
}

#[derive(Args, Clone)]
pub(crate) struct AdminTokenHashArgs {
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
pub(crate) struct AdminTokenGenerateArgs {
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
pub(crate) struct AdminTokenPersistArgs {
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
pub(crate) struct AdminIdentityCommonArgs {
    /// Tenants manifest path; defaults to ARW_TENANTS_FILE or configs/security/tenants.toml
    #[arg(long, value_name = "PATH")]
    tenants_file: Option<PathBuf>,
}

#[derive(Args, Clone)]
pub(crate) struct AdminIdentityAddArgs {
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
pub(crate) struct AdminIdentityRemoveArgs {
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
pub(crate) struct AdminIdentityEnableArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to enable
    #[arg(long)]
    id: String,
}

#[derive(Args, Clone)]
pub(crate) struct AdminIdentityDisableArgs {
    #[command(flatten)]
    common: AdminIdentityCommonArgs,
    /// Principal identifier to disable
    #[arg(long)]
    id: String,
}

#[derive(Args, Clone)]
pub(crate) struct AdminIdentityRotateArgs {
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
pub(crate) struct AdminIdentityShowArgs {
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
pub(crate) enum AdminIdentityCmd {
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
pub(crate) struct AdminAutonomyBaseArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds when calling the API
    #[arg(long, default_value_t = 5)]
    timeout: u64,
    /// Persona id to tag autonomy control actions (falls back to ARW_PERSONA_ID)
    #[arg(long)]
    persona_id: Option<String>,
}

impl AdminAutonomyBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }

    fn persona_id(&self) -> Option<String> {
        resolve_persona_id(&self.persona_id)
    }
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyListArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyShowArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Lane identifier
    #[arg(long)]
    lane: String,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyActionArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Lane identifier
    #[arg(long)]
    lane: String,
    /// Operator identifier to record
    #[arg(long)]
    operator: Option<String>,
    /// Reason for the action
    #[arg(long)]
    reason: Option<String>,
}

#[derive(Copy, Clone, ValueEnum)]
enum AutonomyResumeMode {
    Guided,
    Autonomous,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyResumeArgs {
    #[command(flatten)]
    action: AdminAutonomyActionArgs,
    /// Target mode after resuming
    #[arg(long, value_enum, default_value_t = AutonomyResumeMode::Guided)]
    mode: AutonomyResumeMode,
}

#[derive(Copy, Clone, ValueEnum)]
enum AutonomyFlushState {
    All,
    Queued,
    #[value(name = "in_flight")]
    InFlight,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyFlushArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Lane identifier
    #[arg(long)]
    lane: String,
    /// Which jobs to flush
    #[arg(long, value_enum, default_value_t = AutonomyFlushState::All)]
    state: AutonomyFlushState,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyBudgetsArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Lane identifier
    #[arg(long)]
    lane: String,
    /// Remaining wall clock seconds budget
    #[arg(long)]
    wall_clock_secs: Option<u64>,
    /// Remaining tokens budget
    #[arg(long)]
    tokens: Option<u64>,
    /// Remaining spend budget (in cents)
    #[arg(long)]
    spend_cents: Option<u64>,
    /// Clear budgets instead of updating them
    #[arg(long)]
    clear: bool,
    /// Preview without persisting
    #[arg(long)]
    dry_run: bool,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args, Clone)]
pub(crate) struct AdminAutonomyEngagementArgs {
    #[command(flatten)]
    base: AdminAutonomyBaseArgs,
    /// Lane identifier
    #[arg(long)]
    lane: String,
    /// Emit raw JSON
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Show recent audit entries after reset
    #[arg(long)]
    show_audit: bool,
    /// Number of audit entries to display when --show-audit is set (default: 3)
    #[arg(long, default_value_t = 3, requires = "show_audit")]
    audit_entries: usize,
}

#[derive(Args, Clone)]
pub(crate) struct AdminEgressScopesArgs {
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
pub(crate) struct AdminEgressScopeBaseArgs {
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
pub(crate) struct AdminEgressScopeAddArgs {
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
pub(crate) struct AdminEgressScopeUpdateArgs {
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
pub(crate) struct AdminEgressScopeRemoveArgs {
    #[command(flatten)]
    base: AdminEgressScopeBaseArgs,
    /// Scope identifier to remove
    id: String,
}

#[derive(Subcommand)]
pub(crate) enum AdminReviewCmd {
    /// Memory quarantine helpers
    Quarantine {
        #[command(subcommand)]
        cmd: AdminReviewQuarantineCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum AdminReviewQuarantineCmd {
    /// List memory quarantine entries
    List(AdminReviewQuarantineListArgs),
    /// Admit, reject, or requeue a quarantine entry
    Admit(AdminReviewQuarantineAdmitArgs),
    /// Show a specific quarantine entry
    Show(AdminReviewQuarantineShowArgs),
}

#[derive(Args, Clone)]
pub(crate) struct AdminReviewBaseArgs {
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
pub(crate) struct AdminReviewQuarantineListArgs {
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
pub(crate) struct AdminReviewQuarantineAdmitArgs {
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
pub(crate) struct AdminReviewQuarantineShowArgs {
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
pub(crate) enum AdminReviewStateFilter {
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
pub(crate) enum AdminReviewDecision {
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

pub fn execute(cmd: AdminCmd) -> Result<()> {
    match cmd {
        AdminCmd::Token { cmd } => match cmd {
            AdminTokenCmd::Hash(args) => cmd_admin_token_hash(&args),
            AdminTokenCmd::Generate(args) => cmd_admin_token_generate(&args),
            AdminTokenCmd::Persist(args) => cmd_admin_token_persist(&args),
        },
        AdminCmd::Egress { cmd } => match cmd {
            AdminEgressCmd::Scopes(args) => cmd_admin_egress_scopes(&args),
            AdminEgressCmd::Scope { cmd } => match cmd {
                AdminEgressScopeCmd::Add(args) => cmd_admin_egress_scope_add(&args),
                AdminEgressScopeCmd::Update(args) => cmd_admin_egress_scope_update(&args),
                AdminEgressScopeCmd::Remove(args) => cmd_admin_egress_scope_remove(&args),
            },
        },
        AdminCmd::Review { cmd } => match cmd {
            AdminReviewCmd::Quarantine { cmd } => match cmd {
                AdminReviewQuarantineCmd::List(args) => cmd_admin_review_quarantine_list(&args),
                AdminReviewQuarantineCmd::Admit(args) => cmd_admin_review_quarantine_admit(&args),
                AdminReviewQuarantineCmd::Show(args) => cmd_admin_review_quarantine_show(&args),
            },
        },
        AdminCmd::Identity { cmd } => match cmd {
            AdminIdentityCmd::Add(args) => cmd_admin_identity_add(&args),
            AdminIdentityCmd::Remove(args) => cmd_admin_identity_remove(&args),
            AdminIdentityCmd::Enable(args) => cmd_admin_identity_enable(&args),
            AdminIdentityCmd::Disable(args) => cmd_admin_identity_disable(&args),
            AdminIdentityCmd::Rotate(args) => cmd_admin_identity_rotate(&args),
            AdminIdentityCmd::Show(args) => cmd_admin_identity_show(&args),
        },
        AdminCmd::Autonomy { cmd } => match cmd {
            AdminAutonomyCmd::Lanes(args) => cmd_admin_autonomy_lanes(&args),
            AdminAutonomyCmd::Lane(args) => cmd_admin_autonomy_lane(&args),
            AdminAutonomyCmd::Pause(args) => cmd_admin_autonomy_pause(&args),
            AdminAutonomyCmd::Resume(args) => cmd_admin_autonomy_resume(&args),
            AdminAutonomyCmd::Stop(args) => cmd_admin_autonomy_stop(&args),
            AdminAutonomyCmd::Flush(args) => cmd_admin_autonomy_flush(&args),
            AdminAutonomyCmd::Budgets(args) => cmd_admin_autonomy_budgets(&args),
            AdminAutonomyCmd::EngagementReset(args) => cmd_admin_autonomy_engagement_reset(&args),
        },
        AdminCmd::Persona { cmd } => match cmd {
            AdminPersonaCmd::Grant(args) => cmd_admin_persona_grant(&args),
            AdminPersonaCmd::Seed(args) => cmd_admin_persona_seed(&args),
        },
    }
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

fn cmd_admin_autonomy_lanes(args: &AdminAutonomyListArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let lanes = fetch_autonomy_lanes(&client, base, token.as_deref())?;
    if args.json {
        let value = serde_json::to_value(&lanes).context("serializing autonomy lanes to JSON")?;
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
            );
        }
        return Ok(());
    }
    render_autonomy_lane_list(&lanes.lanes);
    Ok(())
}

fn cmd_admin_autonomy_lane(args: &AdminAutonomyShowArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let lane = fetch_autonomy_lane(&client, base, token.as_deref(), &args.lane)?;
    if args.json {
        let value = serde_json::to_value(&lane).context("serializing autonomy lane to JSON")?;
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
            );
        }
        return Ok(());
    }
    render_autonomy_lane_detail(&lane.lane);
    Ok(())
}

fn cmd_admin_autonomy_pause(args: &AdminAutonomyActionArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let persona = args.base.persona_id();
    let mut payload = JsonMap::new();
    if let Some(operator) = sanitize_option_string(&args.operator) {
        payload.insert("operator".into(), JsonValue::String(operator));
    }
    if let Some(reason) = sanitize_option_string(&args.reason) {
        payload.insert("reason".into(), JsonValue::String(reason));
    }
    let lane = post_autonomy_action(
        &client,
        base,
        token.as_deref(),
        &args.lane,
        "pause",
        payload,
        persona.as_deref(),
    )?;
    println!("Lane '{}' paused.", lane.lane_id);
    render_autonomy_lane_detail(&lane);
    Ok(())
}

fn cmd_admin_autonomy_resume(args: &AdminAutonomyResumeArgs) -> Result<()> {
    let token = resolve_admin_token(&args.action.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.action.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.action.base.base_url();
    let persona = args.action.base.persona_id();
    let mut payload = JsonMap::new();
    if let Some(operator) = sanitize_option_string(&args.action.operator) {
        payload.insert("operator".into(), JsonValue::String(operator));
    }
    if let Some(reason) = sanitize_option_string(&args.action.reason) {
        payload.insert("reason".into(), JsonValue::String(reason));
    }
    let mode_str = match args.mode {
        AutonomyResumeMode::Guided => "guided",
        AutonomyResumeMode::Autonomous => "autonomous",
    };
    payload.insert("mode".into(), JsonValue::String(mode_str.to_string()));
    let lane = post_autonomy_action(
        &client,
        base,
        token.as_deref(),
        &args.action.lane,
        "resume",
        payload,
        persona.as_deref(),
    )?;
    println!("Lane '{}' resumed (mode: {}).", lane.lane_id, lane.mode);
    render_autonomy_lane_detail(&lane);
    Ok(())
}

fn cmd_admin_autonomy_stop(args: &AdminAutonomyActionArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let persona = args.base.persona_id();
    let mut payload = JsonMap::new();
    if let Some(operator) = sanitize_option_string(&args.operator) {
        payload.insert("operator".into(), JsonValue::String(operator));
    }
    if let Some(reason) = sanitize_option_string(&args.reason) {
        payload.insert("reason".into(), JsonValue::String(reason));
    }
    let lane = post_autonomy_action(
        &client,
        base,
        token.as_deref(),
        &args.lane,
        "stop",
        payload,
        persona.as_deref(),
    )?;
    println!(
        "Lane '{}' stopped and remaining jobs flushed.",
        lane.lane_id
    );
    render_autonomy_lane_detail(&lane);
    Ok(())
}

fn cmd_admin_autonomy_flush(args: &AdminAutonomyFlushArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let lane = delete_autonomy_jobs(&client, base, token.as_deref(), &args.lane, args.state)?;
    let scope = match args.state {
        AutonomyFlushState::All => "all",
        AutonomyFlushState::Queued => "queued",
        AutonomyFlushState::InFlight => "in_flight",
    };
    println!("Lane '{}' flush complete (scope: {}).", lane.lane_id, scope);
    render_autonomy_lane_detail(&lane);
    Ok(())
}

fn cmd_admin_autonomy_budgets(args: &AdminAutonomyBudgetsArgs) -> Result<()> {
    if !args.clear
        && args.wall_clock_secs.is_none()
        && args.tokens.is_none()
        && args.spend_cents.is_none()
    {
        bail!("provide at least one budget flag (--wall-clock-secs, --tokens, --spend-cents) or --clear");
    }

    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let persona = args.base.persona_id();

    let mut payload = JsonMap::new();
    if args.clear {
        payload.insert("clear".into(), JsonValue::Bool(true));
    } else {
        if let Some(value) = args.wall_clock_secs {
            payload.insert("wall_clock_secs".into(), JsonValue::from(value));
        }
        if let Some(value) = args.tokens {
            payload.insert("tokens".into(), JsonValue::from(value));
        }
        if let Some(value) = args.spend_cents {
            payload.insert("spend_cents".into(), JsonValue::from(value));
        }
    }
    if args.dry_run {
        payload.insert("dry_run".into(), JsonValue::Bool(true));
    }

    let response = post_autonomy_budgets(
        &client,
        base,
        token.as_deref(),
        &args.lane,
        payload,
        persona.as_deref(),
    )?;

    if args.json {
        let value =
            serde_json::to_value(&response).context("serializing autonomy budgets response")?;
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
            );
        }
        return Ok(());
    }

    if response.dry_run {
        println!(
            "Dry-run preview for lane '{}'; no budgets persisted.",
            response.lane
        );
    } else {
        println!("Budgets updated for lane '{}'.", response.lane);
    }
    if let Some(snapshot) = response.snapshot {
        render_autonomy_lane_detail(&snapshot);
    } else {
        println!("(No lane snapshot returned by the server.)");
    }
    Ok(())
}

fn cmd_admin_autonomy_engagement_reset(args: &AdminAutonomyEngagementArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let response = delete_autonomy_engagement(&client, base, token.as_deref(), &args.lane)
        .with_context(|| format!("resetting engagement for lane '{}'", args.lane))?;

    if args.json {
        let value =
            serde_json::to_value(&response).context("serializing engagement reset response")?;
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&value).unwrap_or_else(|_| value.to_string())
            );
        }
        return Ok(());
    }

    println!("Lane '{}' engagement ledger reset.", response.lane);
    println!("  Score      : {:.2}", response.score);
    let stale_display = response
        .stale_secs
        .map(format_duration_secs)
        .unwrap_or_else(|| "-".into());
    println!("  Stale for  : {}", stale_display);
    println!(
        "  Attention  : {}",
        response.attention.as_deref().unwrap_or("none")
    );
    if args.show_audit {
        let tail = args.audit_entries.clamp(1, 20);
        if let Err(err) = render_recent_engagement_audit(&args.lane, tail) {
            eprintln!(
                "warning: unable to read engagement audit log for lane '{}': {:#}",
                args.lane, err
            );
        }
    }
    Ok(())
}

fn render_recent_engagement_audit(lane: &str, limit: usize) -> Result<()> {
    let state_dir = effective_paths().state_dir;
    let path = Path::new(&state_dir).join("audit.log");
    if !path.exists() {
        println!(
            "Audit log not found at {} – skipping audit tail for lane '{}'.",
            path.display(),
            lane
        );
        return Ok(());
    }

    let file =
        File::open(&path).with_context(|| format!("opening audit log at {}", path.display()))?;
    let reader = io::BufReader::new(file);
    let mut entries: VecDeque<JsonValue> = VecDeque::with_capacity(limit);

    for line in reader.lines() {
        let line = line.context("reading audit log line")?;
        if line.trim().is_empty() {
            continue;
        }
        let value: JsonValue = match serde_json::from_str(&line) {
            Ok(val) => val,
            Err(_) => continue,
        };
        if value.get("action").and_then(|v| v.as_str()) != Some("autonomy.engagement.reset") {
            continue;
        }
        if value.pointer("/details/lane").and_then(|v| v.as_str()) != Some(lane) {
            continue;
        }
        if entries.len() == limit {
            entries.pop_front();
        }
        entries.push_back(value);
    }

    if entries.is_empty() {
        println!(
            "No engagement reset audit entries recorded yet for lane '{}'.",
            lane
        );
        return Ok(());
    }

    println!("Recent engagement reset audits for lane '{}':", lane);
    for entry in entries.iter().rev() {
        let time_raw = entry.get("time").and_then(|v| v.as_str()).unwrap_or("-");
        let time_display = DateTime::parse_from_rfc3339(time_raw)
            .map(|ts| {
                ts.with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|_| time_raw.to_string());
        let score = entry
            .pointer("/details/score")
            .and_then(|v| v.as_f64())
            .map(|s| format!("{:.2}", s))
            .unwrap_or_else(|| "n/a".into());
        let stale_secs = entry
            .pointer("/details/stale_secs")
            .and_then(|v| v.as_u64());
        let stale_display = stale_secs
            .map(format_duration_secs)
            .unwrap_or_else(|| "-".into());
        let attention = entry
            .pointer("/details/attention")
            .and_then(|v| v.as_str())
            .unwrap_or("none");

        println!(
            "  {} | score={} | stale={} | attention={}",
            time_display, score, stale_display, attention
        );
    }

    Ok(())
}

fn cmd_admin_persona_seed(args: &AdminPersonaSeedArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let base = args.base.trim_end_matches('/');

    let owner_ref = if let Some(value) = args.owner_ref.as_ref() {
        value.clone()
    } else {
        fetch_workspace_owner_ref(base, token.as_deref(), args.timeout)
            .context("resolving owner_ref via /state/identity")?
    };

    let state_dir = args
        .state_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(effective_paths().state_dir));

    let traits = load_json_arg("traits", &args.traits)?;
    let mut preferences = load_json_arg("preferences", &args.preferences)?;
    if args.enable_telemetry {
        let scope = args
            .telemetry_scope
            .clone()
            .unwrap_or_else(|| args.owner_kind.clone());
        apply_vibe_preferences(&mut preferences, scope);
    }
    let worldview = load_json_arg("worldview", &args.worldview)?;
    let vibe_profile = load_json_arg("vibe_profile", &args.vibe_profile)?;
    let calibration = load_json_arg("calibration", &args.calibration)?;

    let kernel = Kernel::open(&state_dir)
        .with_context(|| format!("opening kernel at {}", state_dir.display()))?;

    let entry = kernel
        .upsert_persona_entry(PersonaEntryUpsert {
            id: args.id.clone(),
            owner_kind: args.owner_kind.clone(),
            owner_ref,
            name: args.name.clone(),
            archetype: args.archetype.clone(),
            traits,
            preferences,
            worldview,
            vibe_profile,
            calibration,
        })
        .with_context(|| format!("upserting persona {}", args.id))?;

    let preview = match fetch_persona_preview(base, &args.id, token.as_deref(), args.timeout) {
        Ok(value) => value,
        Err(err) => {
            if !args.json {
                eprintln!("warning: unable to fetch persona preview: {err}");
            }
            None
        }
    };

    output_persona_entry(&entry, preview.as_ref(), args.json, args.pretty)?;

    if !args.json {
        println!(
            "Seeded persona {} (owner_kind={}, owner_ref={})",
            entry.id, entry.owner_kind, entry.owner_ref
        );
        if args.enable_telemetry {
            let scope = args
                .telemetry_scope
                .clone()
                .unwrap_or_else(|| args.owner_kind.clone());
            println!("Telemetry enabled (scope: {scope})");
        }
        if let Some(preview) = preview.as_ref() {
            print_persona_preview(preview);
        }
    }

    Ok(())
}

fn output_persona_entry(
    entry: &PersonaEntry,
    preview: Option<&JsonValue>,
    json: bool,
    pretty: bool,
) -> Result<()> {
    if json {
        let mut root = JsonMap::new();
        root.insert("entry".into(), serde_json::to_value(entry)?);
        if let Some(preview) = preview {
            root.insert("preview".into(), preview.clone());
        }
        let value = JsonValue::Object(root);
        if pretty {
            println!("{}", serde_json::to_string_pretty(&value)?);
        } else {
            println!("{}", serde_json::to_string(&value)?);
        }
    }
    Ok(())
}

fn fetch_persona_preview(
    base: &str,
    persona_id: &str,
    token: Option<&str>,
    timeout: u64,
) -> Result<Option<JsonValue>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .context("building HTTP client")?;
    let url = format!("{}/state/persona/{}", base, persona_id);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = match req.send() {
        Ok(resp) => resp,
        Err(err) if err.is_timeout() || err.is_connect() => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("requesting {}", url)),
    };
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value = resp
        .json::<JsonValue>()
        .context("parsing persona preview response")?;
    Ok(Some(value))
}

fn print_persona_preview(preview: &JsonValue) {
    if let Some(bias) = preview
        .get("context_bias_preview")
        .and_then(|v| v.as_object())
    {
        println!("Context bias preview:");
        if let Some(lanes) = bias.get("lane_priorities").and_then(|v| v.as_object()) {
            let mut entries: Vec<(&String, f32)> = lanes
                .iter()
                .filter_map(|(lane, value)| value.as_f64().map(|val| (lane, val as f32)))
                .collect();
            entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            if entries.is_empty() {
                println!("  (no lane priorities detected)");
            } else {
                for (lane, weight) in entries.into_iter().take(5) {
                    println!("  lane {:<16} {:+.2}", lane, weight);
                }
            }
        }
        if let Some(slots) = bias.get("slot_overrides").and_then(|v| v.as_object()) {
            let mut overrides: Vec<(&String, usize)> = slots
                .iter()
                .filter_map(|(slot, value)| value.as_u64().map(|val| (slot, val as usize)))
                .collect();
            overrides.sort_by_key(|(_, limit)| Reverse(*limit));
            if !overrides.is_empty() {
                println!("  slot minimums:");
                for (slot, limit) in overrides.into_iter().take(5) {
                    println!("    {:<16} {}", slot, limit);
                }
            }
        }
        if let Some(delta) = bias.get("min_score_delta").and_then(|v| v.as_f64()) {
            if delta.abs() > f64::EPSILON {
                println!("  min_score_delta: {:+.2}", delta);
            }
        }
    }

    if let Some(metrics) = preview
        .get("vibe_metrics_preview")
        .and_then(|v| v.as_object())
    {
        println!("Vibe metrics preview:");
        if let Some(total) = metrics.get("total_feedback").and_then(|v| v.as_u64()) {
            println!("  total_feedback: {}", total);
        }
        if let Some(avg) = metrics.get("average_strength").and_then(|v| v.as_f64()) {
            println!("  average_strength: {:.2}", avg);
        }

        let counts = metrics
            .get("signal_counts")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let strengths = metrics
            .get("signal_strength")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let weights = metrics
            .get("signal_weights")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        if !counts.is_empty() {
            let mut signals: Vec<(String, f32, Option<f32>, u64)> = counts
                .into_iter()
                .filter_map(|(signal, count)| {
                    count.as_u64().map(|c| {
                        let weight = weights
                            .get(&signal)
                            .and_then(|v| v.as_f64())
                            .unwrap_or(c as f64) as f32;
                        let avg = strengths
                            .get(&signal)
                            .and_then(|v| v.as_f64())
                            .map(|v| v as f32);
                        (signal, weight, avg, c)
                    })
                })
                .collect();
            signals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
            println!("  top signals:");
            for (signal, weight, avg, count) in signals.into_iter().take(5) {
                match avg {
                    Some(avg) => println!(
                        "    {:<16} count={:<3} weight={:.2} avg_strength={:.2}",
                        signal, count, weight, avg
                    ),
                    None => println!("    {:<16} count={:<3} weight={:.2}", signal, count, weight),
                }
            }
        }
    }
}

fn fetch_workspace_owner_ref(base: &str, token: Option<&str>, timeout: u64) -> Result<String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .context("building HTTP client")?;
    let url = format!("{}/state/identity", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    if resp.status() == StatusCode::UNAUTHORIZED {
        bail!("Unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !resp.status().is_success() {
        bail!("failed to fetch identity: status {}", resp.status());
    }
    let body: JsonValue = resp.json().context("parsing identity response")?;
    body.get("workspace")
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("workspace id missing in identity response"))
}

fn load_json_arg(label: &str, raw: &Option<String>) -> Result<JsonValue> {
    if let Some(source) = raw {
        let text = if let Some(path) = source.strip_prefix('@') {
            read_to_string(path).with_context(|| format!("reading {label} from {}", path))?
        } else {
            source.clone()
        };
        serde_json::from_str(&text).with_context(|| format!("parsing {label} JSON"))
    } else {
        Ok(json!({}))
    }
}

fn apply_vibe_preferences(preferences: &mut JsonValue, scope: String) {
    if !preferences.is_object() {
        *preferences = json!({});
    }
    let telemetry = preferences
        .as_object_mut()
        .expect("preferences object")
        .entry("telemetry".to_string())
        .or_insert_with(|| json!({}));
    if !telemetry.is_object() {
        *telemetry = json!({});
    }
    let vibe = telemetry
        .as_object_mut()
        .expect("telemetry object")
        .entry("vibe".to_string())
        .or_insert_with(|| json!({}));
    if !vibe.is_object() {
        *vibe = json!({});
    }
    let vibe_obj = vibe.as_object_mut().expect("vibe object");
    vibe_obj.insert("enabled".to_string(), JsonValue::Bool(true));
    vibe_obj.insert("scope".to_string(), JsonValue::String(scope));
}

fn cmd_admin_persona_grant(args: &AdminPersonaGrantArgs) -> Result<()> {
    let token = resolve_admin_token(&args.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("building HTTP client")?;
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/leases", base);

    let mut payload = JsonMap::new();
    payload.insert(
        "capability".into(),
        JsonValue::String("persona:manage".into()),
    );
    payload.insert(
        "ttl_secs".into(),
        JsonValue::from(args.ttl_secs.clamp(1, 86_400)),
    );
    if let Some(scope) = sanitize_option_string(&args.scope) {
        payload.insert("scope".into(), JsonValue::String(scope));
    }
    if let Some(budget) = args.budget {
        payload.insert("budget".into(), JsonValue::from(budget));
    }

    let response = with_admin_headers(client.post(&url).json(&payload), token.as_deref())
        .send()
        .with_context(|| format!("POST {}", url))?;

    let status = response.status();
    let body = response.text().unwrap_or_default();
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        bail!("{} returned {}: {}", url, status, body);
    }

    let value: JsonValue =
        serde_json::from_str(&body).with_context(|| format!("parsing response from {}", url))?;
    if args.json {
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
            );
        } else {
            println!("{}", value);
        }
    } else {
        let id = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        let ttl_until = value
            .get("ttl_until")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        if let Some(scope) = value.get("scope").and_then(|v| v.as_str()) {
            println!(
                "Granted persona:manage lease {} (scope: {}) valid until {}",
                id, scope, ttl_until
            );
        } else {
            println!(
                "Granted persona:manage lease {} valid until {}",
                id, ttl_until
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyBudgets {
    #[serde(default)]
    wall_clock_remaining_secs: Option<u64>,
    #[serde(default)]
    tokens_remaining: Option<u64>,
    #[serde(default)]
    spend_remaining_cents: Option<u64>,
}

fn default_autonomy_mode() -> String {
    "guided".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyLane {
    lane_id: String,
    #[serde(default = "default_autonomy_mode")]
    mode: String,
    #[serde(default)]
    active_jobs: u64,
    #[serde(default)]
    queued_jobs: u64,
    #[serde(default)]
    last_event: Option<String>,
    #[serde(default)]
    last_operator: Option<String>,
    #[serde(default)]
    last_reason: Option<String>,
    #[serde(default)]
    updated_ms: Option<u64>,
    #[serde(default)]
    budgets: Option<CliAutonomyBudgets>,
    #[serde(default)]
    alerts: Vec<String>,
    #[serde(default)]
    last_budget_update_ms: Option<u64>,
    #[serde(default)]
    engagement_score: Option<f32>,
    #[serde(default)]
    engagement_stale_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyLanesResponse {
    lanes: Vec<CliAutonomyLane>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyLaneEnvelope {
    lane: CliAutonomyLane,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyBudgetsEnvelope {
    ok: bool,
    lane: String,
    #[serde(default)]
    snapshot: Option<CliAutonomyLane>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CliAutonomyEngagementReset {
    ok: bool,
    lane: String,
    score: f32,
    #[serde(default)]
    stale_secs: Option<u64>,
    #[serde(default)]
    attention: Option<String>,
}

fn fetch_autonomy_lanes(
    client: &Client,
    base: &str,
    token: Option<&str>,
) -> Result<CliAutonomyLanesResponse> {
    let url = format!("{}/state/autonomy/lanes", base);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading autonomy lanes response")?;
    parse_admin_response(status, body, &url)
}

fn fetch_autonomy_lane(
    client: &Client,
    base: &str,
    token: Option<&str>,
    lane: &str,
) -> Result<CliAutonomyLaneEnvelope> {
    let url = format!("{}/state/autonomy/lanes/{}", base, lane);
    let mut req = client.get(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading autonomy lane response")?;
    parse_admin_response(status, body, &url)
}

fn post_autonomy_action(
    client: &Client,
    base: &str,
    token: Option<&str>,
    lane: &str,
    action: &str,
    mut payload: JsonMap<String, JsonValue>,
    persona_id: Option<&str>,
) -> Result<CliAutonomyLane> {
    if let Some(pid) = persona_id {
        payload.insert("persona_id".into(), JsonValue::String(pid.to_string()));
    }
    let url = format!("{}/admin/autonomy/{}/{}", base, lane, action);
    let mut req = client.post(&url);
    req = with_admin_headers(req, token);
    let req = req.json(&JsonValue::Object(payload));
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading autonomy action response")?;
    let envelope: CliAutonomyLaneEnvelope = parse_admin_response(status, body, &url)?;
    Ok(envelope.lane)
}

fn delete_autonomy_jobs(
    client: &Client,
    base: &str,
    token: Option<&str>,
    lane: &str,
    scope: AutonomyFlushState,
) -> Result<CliAutonomyLane> {
    let scope_param = match scope {
        AutonomyFlushState::All => "all",
        AutonomyFlushState::Queued => "queued",
        AutonomyFlushState::InFlight => "in_flight",
    };
    let url = format!(
        "{}/admin/autonomy/{}/jobs?state={}",
        base, lane, scope_param
    );
    let mut req = client.delete(&url);
    req = with_admin_headers(req, token);
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading autonomy flush response")?;
    let envelope: CliAutonomyLaneEnvelope = parse_admin_response(status, body, &url)?;
    Ok(envelope.lane)
}

fn post_autonomy_budgets(
    client: &Client,
    base: &str,
    token: Option<&str>,
    lane: &str,
    mut payload: JsonMap<String, JsonValue>,
    persona_id: Option<&str>,
) -> Result<CliAutonomyBudgetsEnvelope> {
    if let Some(pid) = persona_id {
        payload.insert("persona_id".into(), JsonValue::String(pid.to_string()));
    }
    let url = format!("{}/admin/autonomy/{}/budgets", base, lane);
    let mut req = client.post(&url);
    req = with_admin_headers(req, token);
    let req = req.json(&JsonValue::Object(payload));
    let resp = req.send().with_context(|| format!("requesting {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading autonomy budgets response")?;
    parse_admin_response(status, body, &url)
}

fn delete_autonomy_engagement(
    client: &Client,
    base: &str,
    token: Option<&str>,
    lane: &str,
) -> Result<CliAutonomyEngagementReset> {
    let url = format!("{}/admin/autonomy/{}/engagement", base, lane);
    let mut req = client.delete(&url);
    req = with_admin_headers(req, token);
    let resp = req
        .send()
        .with_context(|| format!("resetting engagement via {}", url))?;
    let status = resp.status();
    let body = resp.text().context("reading engagement reset response")?;
    parse_admin_response(status, body, &url)
}

fn parse_admin_response<T>(status: StatusCode, body: String, url: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    if status == StatusCode::UNAUTHORIZED {
        bail!("unauthorized: provide --admin-token or set ARW_ADMIN_TOKEN");
    }
    if !status.is_success() {
        bail!("{} returned {}: {}", url, status, body);
    }
    serde_json::from_str(&body).with_context(|| format!("parsing response from {}", url))
}

fn sanitize_option_string(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn render_autonomy_lane_list(lanes: &[CliAutonomyLane]) {
    if lanes.is_empty() {
        println!("No autonomy lanes tracked yet.");
        return;
    }
    let mut rows = lanes.to_vec();
    rows.sort_by(|a, b| a.lane_id.cmp(&b.lane_id));
    println!(
        "{:<24} {:<11} {:>6} {:>6} {:<18} {:<20} Alerts / Budgets",
        "Lane", "Mode", "Active", "Queued", "Last Event", "Updated"
    );
    for lane in rows {
        let last_event = lane.last_event.as_deref().unwrap_or("-");
        let updated = format_timestamp_ms(lane.updated_ms);
        let mut summary_parts: Vec<String> = Vec::new();
        if !lane.alerts.is_empty() {
            summary_parts.push(format!("alerts: {}", lane.alerts.join("; ")));
        }
        if let Some(budgets) = lane.budgets.as_ref() {
            let text = budgets_summary(budgets);
            if text != "n/a" {
                summary_parts.push(format!("budgets {}", text));
            }
        }
        if let Some(score) = lane.engagement_score {
            summary_parts.push(format!("eng {:.2}", score));
        }
        if let Some(stale) = lane.engagement_stale_secs {
            summary_parts.push(format!("stale {}", format_duration_secs(stale)));
        }
        let summary = if summary_parts.is_empty() {
            "-".to_string()
        } else {
            summary_parts.join(" | ")
        };
        println!(
            "{:<24} {:<11} {:>6} {:>6} {:<18} {:<20} {}",
            lane.lane_id,
            lane.mode,
            lane.active_jobs,
            lane.queued_jobs,
            last_event,
            updated,
            summary
        );
    }
}

fn render_autonomy_lane_detail(lane: &CliAutonomyLane) {
    println!("Lane           : {}", lane.lane_id);
    println!("Mode           : {}", lane.mode);
    println!("Active jobs    : {}", lane.active_jobs);
    println!("Queued jobs    : {}", lane.queued_jobs);
    println!(
        "Last event     : {}",
        lane.last_event.as_deref().unwrap_or("-")
    );
    println!(
        "Last operator  : {}",
        lane.last_operator.as_deref().unwrap_or("-")
    );
    println!(
        "Last reason    : {}",
        lane.last_reason.as_deref().unwrap_or("-")
    );
    println!("Updated        : {}", format_timestamp_ms(lane.updated_ms));
    if let Some(score) = lane.engagement_score {
        println!("Engagement     : {:.2}", score);
    } else {
        println!("Engagement     : -");
    }
    if let Some(stale) = lane.engagement_stale_secs {
        println!("Stale for      : {}", format_duration_secs(stale));
    } else {
        println!("Stale for      : -");
    }
    if !lane.alerts.is_empty() {
        println!("Alerts         : {}", lane.alerts.join("; "));
    } else {
        println!("Alerts         : none");
    }
    if let Some(budgets) = lane.budgets.as_ref() {
        println!("Budgets        : {}", budgets_summary(budgets));
        println!(
            "  Wall clock    : {}",
            budgets
                .wall_clock_remaining_secs
                .map(|v| format!("{}s", v))
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  Tokens        : {}",
            budgets
                .tokens_remaining
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  Spend         : {}",
            budgets
                .spend_remaining_cents
                .map(format_spend_cents)
                .unwrap_or_else(|| "n/a".into())
        );
        println!(
            "  Updated at    : {}",
            format_timestamp_ms(lane.last_budget_update_ms)
        );
    } else {
        println!("Budgets        : none");
    }
}

fn format_timestamp_ms(ms: Option<u64>) -> String {
    ms.and_then(|value| Local.timestamp_millis_opt(value as i64).single())
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".into())
}

fn format_duration_secs(secs: u64) -> String {
    if secs == 0 {
        return "0s".into();
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 && parts.len() < 3 {
        parts.push(format!("{}s", seconds));
    }
    if parts.is_empty() {
        "0s".into()
    } else {
        parts.join(" ")
    }
}

fn budgets_summary(budgets: &CliAutonomyBudgets) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(value) = budgets.wall_clock_remaining_secs {
        parts.push(format!("wall={}s", value));
    }
    if let Some(value) = budgets.tokens_remaining {
        parts.push(format!("tokens={}", value));
    }
    if let Some(value) = budgets.spend_remaining_cents {
        parts.push(format!("spend={}", format_spend_cents(value)));
    }
    if parts.is_empty() {
        "n/a".to_string()
    } else {
        parts.join(" ")
    }
}

fn format_spend_cents(cents: u64) -> String {
    let dollars = cents / 100;
    let remainder = cents % 100;
    format!("${}.{:02}", dollars, remainder)
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

    let scope_leases = collect_scope_leases(snapshot);
    let now = Utc::now();

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

        let mut lease_summary: Option<String> = None;
        if !id.is_empty() {
            if let Some(entries) = scope_leases.get(&id) {
                lease_summary = Some(summarize_scope_leases(entries, now));
            }
        }
        if lease_summary.is_none() && !description.is_empty() {
            if let Some(entries) = scope_leases.get(&description) {
                lease_summary = Some(summarize_scope_leases(entries, now));
            }
        }
        if let Some(summary) = lease_summary {
            println!("    Leases: {}", summary);
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct ScopeLeaseInfo {
    capability: String,
    matched_capability: Option<String>,
    ttl_until: Option<String>,
}

fn collect_scope_leases(snapshot: &JsonValue) -> HashMap<String, Vec<ScopeLeaseInfo>> {
    let mut scoped: HashMap<String, Vec<ScopeLeaseInfo>> = HashMap::new();
    if let Some(items) = snapshot
        .get("leases")
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
    {
        for lease in items {
            let scope = lease
                .get("scope")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let capability = lease
                .get("capability")
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            if let (Some(scope_id), Some(cap)) = (scope, capability) {
                let info = ScopeLeaseInfo {
                    capability: cap.to_string(),
                    matched_capability: lease
                        .get("matched_capability")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    ttl_until: lease
                        .get("ttl_until")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                };
                scoped.entry(scope_id.to_string()).or_default().push(info);
            }
        }
    }
    scoped
}

fn summarize_scope_leases(entries: &[ScopeLeaseInfo], now: DateTime<Utc>) -> String {
    if entries.is_empty() {
        return "none".into();
    }
    let mut by_capability: BTreeMap<String, ScopeLeaseInfo> = BTreeMap::new();
    for entry in entries {
        by_capability
            .entry(entry.capability.clone())
            .and_modify(|existing| {
                let existing_ttl = existing.ttl_until.as_deref().and_then(parse_ttl_until);
                let new_ttl = entry.ttl_until.as_deref().and_then(parse_ttl_until);
                let replace = match (existing_ttl, new_ttl) {
                    (None, Some(_)) => true,
                    (Some(_), None) => false,
                    (Some(old), Some(newer)) => newer > old,
                    (None, None) => false,
                };
                if replace {
                    *existing = entry.clone();
                }
            })
            .or_insert_with(|| entry.clone());
    }

    let mut parts: Vec<String> = Vec::new();
    for info in by_capability.values() {
        let label = info
            .matched_capability
            .as_ref()
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .unwrap_or(info.capability.as_str());

        if let Some(ttl_str) = info.ttl_until.as_deref() {
            match parse_ttl_until(ttl_str) {
                Some(ttl) => {
                    let delta = ttl - now;
                    if delta > ChronoDuration::zero() {
                        parts.push(format!(
                            "{} expires in {} ({})",
                            label,
                            human_duration(delta),
                            ttl.format("%Y-%m-%d %H:%M:%S")
                        ));
                    } else {
                        let elapsed = -delta;
                        parts.push(format!(
                            "{} expired {} ago ({})",
                            label,
                            human_duration(elapsed),
                            ttl.format("%Y-%m-%d %H:%M:%S")
                        ));
                    }
                }
                None => {
                    parts.push(format!("{} (invalid ttl {})", label, ttl_str));
                }
            }
        } else {
            parts.push(format!("{} (ttl unknown)", label));
        }
    }

    if parts.is_empty() {
        "none".into()
    } else {
        parts.join(" | ")
    }
}

fn parse_ttl_until(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn human_duration(delta: ChronoDuration) -> String {
    let mut seconds = delta.num_seconds().abs();
    let days = seconds / 86_400;
    seconds %= 86_400;
    let hours = seconds / 3_600;
    seconds %= 3_600;
    let minutes = seconds / 60;
    seconds %= 60;

    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    parts.join("")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;

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
    fn budgets_summary_reports_n_a_when_empty() {
        let budgets = CliAutonomyBudgets {
            wall_clock_remaining_secs: None,
            tokens_remaining: None,
            spend_remaining_cents: None,
        };
        assert_eq!(budgets_summary(&budgets), "n/a");
    }

    #[test]
    fn budgets_summary_formats_present_fields() {
        let budgets = CliAutonomyBudgets {
            wall_clock_remaining_secs: Some(90),
            tokens_remaining: Some(12),
            spend_remaining_cents: Some(1_505),
        };
        assert_eq!(budgets_summary(&budgets), "wall=90s tokens=12 spend=$15.05");
    }
}
