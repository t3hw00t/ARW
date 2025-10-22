use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
#[cfg(test)]
use serde_json::json;
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::util::{
    format_local_timestamp, format_relative_from_now, resolve_admin_token, resolve_persona_id,
    with_admin_headers,
};

#[derive(Subcommand)]
pub enum OrchestratorCmd {
    /// Inspect the mini-agent catalog exposed by the orchestrator
    Catalog(OrchestratorCatalogArgs),
    /// Start a persona-aware training run via /orchestrator/mini_agents/start_training
    Start(Box<OrchestratorStartArgs>),
    /// List recent orchestrator jobs and their status
    Jobs(OrchestratorJobsArgs),
}

#[derive(Args, Clone)]
pub struct OrchestratorBaseArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Request timeout (seconds)
    #[arg(long, default_value_t = 10)]
    pub timeout: u64,
}

impl OrchestratorBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout.max(1))
    }
}

#[derive(Args, Clone)]
pub struct OrchestratorCatalogArgs {
    #[command(flatten)]
    pub base: OrchestratorBaseArgs,
    /// Emit server JSON instead of a human summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Filter catalog entries by status (alpha/beta/stable/incubating)
    #[arg(long)]
    pub status: Option<String>,
    /// Filter catalog entries by category (memory/validation/governor/etc.)
    #[arg(long)]
    pub category: Option<String>,
}

#[derive(Args, Clone)]
pub struct OrchestratorJobsArgs {
    #[command(flatten)]
    pub base: OrchestratorBaseArgs,
    /// Maximum number of jobs to fetch (1-500)
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
    /// Emit server JSON instead of a human summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
}

#[derive(Args, Clone)]
pub struct OrchestratorStartArgs {
    #[command(flatten)]
    pub base: OrchestratorBaseArgs,
    /// Training goal (becomes the orchestrator job label)
    #[arg(value_name = "GOAL")]
    pub goal: String,
    /// Persona id to tag the training run (falls back to ARW_PERSONA_ID)
    #[arg(long)]
    pub persona_id: Option<String>,
    /// Inline JSON payload merged into the request body
    #[arg(long, value_name = "JSON")]
    pub data_json: Option<String>,
    /// Path to a JSON payload merged into the request body
    #[arg(long, value_name = "PATH")]
    pub data_file: Option<PathBuf>,
    /// Training preset hint (balanced/performance/power-saver/quick/deep/verified)
    #[arg(long)]
    pub preset: Option<String>,
    /// Override training mode (guided/expert/etc.)
    #[arg(long)]
    pub mode: Option<String>,
    /// Diversity hint (0.0-1.0)
    #[arg(long)]
    pub diversity: Option<f64>,
    /// Recency hint (0.0-1.0)
    #[arg(long)]
    pub recency: Option<f64>,
    /// Compression hint (0.0-1.0)
    #[arg(long)]
    pub compression: Option<f64>,
    /// Budget tokens hint
    #[arg(long, value_name = "TOKENS")]
    pub budget_tokens: Option<u32>,
    /// Episode count hint
    #[arg(long, value_name = "EPISODES")]
    pub episodes: Option<u32>,
    /// Associate the job with a project
    #[arg(long)]
    pub project: Option<String>,
    /// Add a topic slug (repeatable)
    #[arg(long = "topic", value_name = "TOPIC")]
    pub topics: Vec<String>,
    /// Emit server JSON instead of a human summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Follow the orchestrator job until completion
    #[arg(long)]
    pub follow: bool,
}

pub fn execute(cmd: OrchestratorCmd) -> Result<()> {
    match cmd {
        OrchestratorCmd::Catalog(args) => catalog(args),
        OrchestratorCmd::Start(args) => start(*args),
        OrchestratorCmd::Jobs(args) => jobs(args),
    }
}

fn catalog(args: OrchestratorCatalogArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(args.base.timeout())
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let url = format!("{base}/orchestrator/mini_agents");
    let response = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting orchestrator catalog from {url}"))?;
    if !response.status().is_success() {
        bail!(
            "catalog request failed: status {}",
            response.status().as_u16()
        );
    }
    let payload: JsonValue = response.json().context("parsing catalog response")?;
    if args.json {
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
    let filtered = filter_catalog(&payload, args.status.as_deref(), args.category.as_deref());
    println!("{}", render_catalog_summary(&filtered).trim_end());
    Ok(())
}

fn jobs(args: OrchestratorJobsArgs) -> Result<()> {
    if args.limit <= 0 || args.limit > 500 {
        bail!("--limit must be between 1 and 500");
    }
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(args.base.timeout())
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let url = format!("{base}/state/orchestrator/jobs?limit={}", args.limit);
    let response = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting orchestrator jobs from {url}"))?;
    if response.status().is_client_error() && response.status().as_u16() == 501 {
        bail!("kernel disabled: orchestrator jobs unavailable");
    }
    if !response.status().is_success() {
        bail!("jobs request failed: status {}", response.status().as_u16());
    }
    let payload: JsonValue = response
        .json()
        .context("parsing orchestrator jobs response")?;
    if args.json {
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
    println!("{}", render_jobs_summary(&payload).trim_end());
    Ok(())
}

fn start(args: OrchestratorStartArgs) -> Result<()> {
    if args.data_file.is_some() && args.data_json.is_some() {
        bail!("--data-file and --data-json cannot be combined");
    }
    let token = resolve_admin_token(&args.base.admin_token);
    let persona = resolve_persona_id(&args.persona_id);
    let base = args.base.base_url().to_string();
    let client = Client::builder()
        .timeout(args.base.timeout())
        .build()
        .context("building HTTP client")?;

    let base_data = load_data_override(args.data_file.as_ref(), args.data_json.as_ref())?;
    let payload = build_start_payload(&args, persona.as_deref(), base_data)?;

    let response = with_admin_headers(
        client.post(format!("{base}/orchestrator/mini_agents/start_training")),
        token.as_deref(),
    )
    .json(&payload)
    .send()
    .context("starting orchestrator training job")?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<failed to read error body>".into());
        bail!(
            "training request failed: status {} body {}",
            status.as_u16(),
            body
        );
    }

    let body: JsonValue = response.json().context("parsing training response")?;
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
    println!("{}", render_start_summary(&body).trim_end());
    if args.follow {
        if let Some(job_id) = job_id_from_payload(&body) {
            follow_training_job(
                &client,
                &base,
                token.as_deref(),
                &job_id,
                persona.as_deref(),
            )?;
        } else {
            eprintln!("warning: --follow requested but response did not include job_id");
        }
    }
    Ok(())
}

fn load_data_override(
    file: Option<&PathBuf>,
    inline: Option<&String>,
) -> Result<Option<JsonValue>> {
    if let Some(path) = file {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let parsed: JsonValue = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing json from {}", path.display()))?;
        return Ok(Some(parsed));
    }
    if let Some(raw) = inline {
        let parsed: JsonValue =
            serde_json::from_str(raw).context("parsing inline --data-json payload")?;
        return Ok(Some(parsed));
    }
    Ok(None)
}

fn build_start_payload(
    args: &OrchestratorStartArgs,
    persona_id: Option<&str>,
    base_data: Option<JsonValue>,
) -> Result<JsonValue> {
    let trimmed_goal = args.goal.trim();
    if trimmed_goal.is_empty() {
        bail!("goal cannot be empty");
    }
    let mut root_map = match base_data {
        Some(JsonValue::Object(map)) => map,
        Some(other) => bail!(
            "training data overrides must be JSON objects (received {})",
            other
        ),
        None => JsonMap::new(),
    };

    if let Some(pid) = persona_id {
        root_map.insert("persona_id".into(), JsonValue::String(pid.to_string()));
    }
    if let Some(project) = args
        .project
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        root_map.insert("project".into(), JsonValue::String(project.to_string()));
    }
    if !args.topics.is_empty() {
        let mut existing_topics: BTreeSet<String> = root_map
            .get("topics")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|value| value.as_str().map(|s| s.trim().to_string()))
            .filter(|value| !value.is_empty())
            .collect();
        for topic in &args.topics {
            let trimmed = topic.trim();
            if !trimmed.is_empty() {
                existing_topics.insert(trimmed.to_string());
            }
        }
        if !existing_topics.is_empty() {
            root_map.insert(
                "topics".into(),
                JsonValue::Array(existing_topics.into_iter().map(JsonValue::String).collect()),
            );
        }
    }

    let mut training_map = match root_map.remove("training") {
        Some(JsonValue::Object(map)) => map,
        Some(other) => bail!(
            "expected training overrides to be an object, found {}",
            other
        ),
        None => JsonMap::new(),
    };

    if let Some(preset) = args
        .preset
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        training_map.insert("preset".into(), JsonValue::String(preset.to_string()));
    }
    if let Some(mode) = args
        .mode
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        training_map.insert("mode".into(), JsonValue::String(mode.to_string()));
    }
    if let Some(diversity) = clamp_unit("diversity", args.diversity)? {
        if let Some(num) = serde_json::Number::from_f64(diversity) {
            training_map.insert("diversity".into(), JsonValue::Number(num));
        }
    }
    if let Some(recency) = clamp_unit("recency", args.recency)? {
        if let Some(num) = serde_json::Number::from_f64(recency) {
            training_map.insert("recency".into(), JsonValue::Number(num));
        }
    }
    if let Some(compression) = clamp_unit("compression", args.compression)? {
        if let Some(num) = serde_json::Number::from_f64(compression) {
            training_map.insert("compression".into(), JsonValue::Number(num));
        }
    }
    if let Some(tokens) = args.budget_tokens {
        training_map.insert("budget_tokens".into(), JsonValue::Number(tokens.into()));
    }
    if let Some(episodes) = args.episodes {
        training_map.insert("episodes".into(), JsonValue::Number(episodes.into()));
    }
    if !training_map.is_empty() {
        root_map.insert("training".into(), JsonValue::Object(training_map));
    }

    let mut request = JsonMap::new();
    request.insert("goal".into(), JsonValue::String(trimmed_goal.to_string()));
    if !root_map.is_empty() {
        request.insert("data".into(), JsonValue::Object(root_map));
    }
    if let Some(pid) = persona_id {
        request.insert("persona_id".into(), JsonValue::String(pid.to_string()));
    }
    Ok(JsonValue::Object(request))
}

fn filter_catalog(payload: &JsonValue, status: Option<&str>, category: Option<&str>) -> JsonValue {
    let items = payload
        .get("items")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if status.is_none() && category.is_none() {
        return JsonValue::Array(items);
    }
    let status_norm = status.map(|s| s.trim().to_ascii_lowercase());
    let category_norm = category.map(|s| s.trim().to_ascii_lowercase());
    JsonValue::Array(
        items
            .into_iter()
            .filter(|item| match (status_norm.as_ref(), item.get("status")) {
                (Some(expected), Some(status_value)) => status_value
                    .as_str()
                    .map(|candidate| candidate.trim().to_ascii_lowercase() == **expected)
                    .unwrap_or(false),
                (Some(_), None) => false,
                (None, _) => true,
            })
            .filter(|item| {
                match (
                    category_norm.as_ref(),
                    item.get("category").and_then(|v| v.as_str()),
                ) {
                    (Some(expected), Some(candidate)) => {
                        candidate.trim().to_ascii_lowercase() == **expected
                    }
                    (Some(_), None) => false,
                    (None, _) => true,
                }
            })
            .collect(),
    )
}

fn render_catalog_summary(payload: &JsonValue) -> String {
    let mut out = String::new();
    let items = payload.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        out.push_str("(catalog empty)\n");
        return out;
    }
    for item in items {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>");
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("<unnamed>");
        let category = item
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("n/a");
        let status = item
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let preset = item
            .get("training")
            .and_then(|v| v.get("preset"))
            .and_then(|v| v.as_str())
            .unwrap_or("balanced");
        let est_runtime = item
            .get("training")
            .and_then(|v| v.get("est_runtime_minutes"))
            .and_then(|v| v.as_u64());
        out.push_str(&format!(
            "- {id} :: {name} (category: {category}, status: {status}, preset: {preset}"
        ));
        if let Some(minutes) = est_runtime {
            out.push_str(&format!(", est_runtime: {minutes}m"));
        }
        out.push_str(")\n");
        if let Some(summary) = item.get("summary").and_then(|v| v.as_str()) {
            out.push_str(&format!("  summary: {}\n", summary));
        }
        if let Some(requirements) = item
            .get("requirements")
            .and_then(|v| v.get("leases"))
            .and_then(|v| v.as_array())
        {
            if !requirements.is_empty() {
                let leases: Vec<&str> = requirements
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect();
                if !leases.is_empty() {
                    out.push_str(&format!("  leases: {}\n", leases.join(", ")));
                }
            }
        }
    }
    out
}

fn render_jobs_summary(payload: &JsonValue) -> String {
    let mut out = String::new();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
    let items = payload
        .get("items")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        out.push_str("(no orchestrator jobs)\n");
        return out;
    }
    for item in items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("<id>");
        let goal = item
            .get("goal")
            .and_then(|v| v.as_str())
            .unwrap_or("<goal>");
        let status = item
            .get("status_label")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("status").and_then(|v| v.as_str()))
            .unwrap_or("Unknown");
        let persona = item
            .get("persona_id")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        let created = item.get("created").and_then(|v| v.as_str()).unwrap_or("");
        let updated = item.get("updated").and_then(|v| v.as_str()).unwrap_or("");
        let progress = item
            .get("progress")
            .and_then(|v| v.as_f64())
            .map(|p| (p * 100.0).round() as i64);

        out.push_str(&format!("- {goal} [{status}] (job {id}"));
        if let Some(p) = progress {
            out.push_str(&format!(", progress: {p}%"));
        }
        if let Some(persona) = persona.as_ref() {
            out.push_str(&format!(", persona: {persona}"));
        }
        out.push_str(")\n");
        if !created.is_empty() {
            let formatted = format_local_timestamp(parse_rfc3339_millis(created).unwrap_or(0));
            out.push_str(&format!("  created: {formatted}"));
            if let Some(ms) = parse_rfc3339_millis(created) {
                out.push_str(&format!(" ({})", format_relative_from_now(ms, now_ms)));
            }
            out.push('\n');
        }
        if !updated.is_empty() {
            let formatted = format_local_timestamp(parse_rfc3339_millis(updated).unwrap_or(0));
            out.push_str(&format!("  updated: {formatted}"));
            if let Some(ms) = parse_rfc3339_millis(updated) {
                out.push_str(&format!(" ({})", format_relative_from_now(ms, now_ms)));
            }
            out.push('\n');
        }
        if let Some(data) = item.get("data").and_then(|v| v.as_object()) {
            if let Some(training) = data.get("training").and_then(|v| v.as_object()) {
                let mut hints = Vec::new();
                if let Some(preset) = training
                    .get("preset")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                {
                    hints.push(format!("preset={preset}"));
                }
                if let Some(mode) = training
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                {
                    hints.push(format!("mode={mode}"));
                }
                if let Some(div) = training.get("diversity").and_then(|v| v.as_f64()) {
                    hints.push(format!("diversity={div:.2}"));
                }
                if let Some(rec) = training.get("recency").and_then(|v| v.as_f64()) {
                    hints.push(format!("recency={rec:.2}"));
                }
                if let Some(comp) = training.get("compression").and_then(|v| v.as_f64()) {
                    hints.push(format!("compression={comp:.2}"));
                }
                if let Some(tokens) = training.get("budget_tokens").and_then(|v| v.as_u64()) {
                    hints.push(format!("budget_tokens={tokens}"));
                }
                if let Some(episodes) = training.get("episodes").and_then(|v| v.as_u64()) {
                    hints.push(format!("episodes={episodes}"));
                }
                if !hints.is_empty() {
                    out.push_str(&format!("  training: {}\n", hints.join(", ")));
                }
            }
        }
    }
    out
}

fn job_id_from_payload(payload: &JsonValue) -> Option<String> {
    payload
        .get("job_id")
        .or_else(|| payload.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn render_start_summary(payload: &JsonValue) -> String {
    let mut out = String::new();
    let job_id = payload
        .get("job_id")
        .or_else(|| payload.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    let goal = payload
        .get("goal")
        .and_then(|v| v.as_str())
        .unwrap_or("<goal>");
    let persona = payload
        .get("data")
        .and_then(|v| v.get("persona_id"))
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("persona_id").and_then(|v| v.as_str()));
    out.push_str(&format!("started orchestrator job {job_id} → {goal}\n"));
    if let Some(persona) = persona {
        out.push_str(&format!("  persona: {persona}\n"));
    }
    if let Some(data) = payload.get("data").and_then(|v| v.as_object()) {
        if let Some(training) = data.get("training").and_then(|v| v.as_object()) {
            let mut hints = Vec::new();
            if let Some(preset) = training.get("preset").and_then(|v| v.as_str()) {
                hints.push(format!("preset={preset}"));
            }
            if let Some(mode) = training.get("mode").and_then(|v| v.as_str()) {
                hints.push(format!("mode={mode}"));
            }
            if let Some(div) = training.get("diversity").and_then(|v| v.as_f64()) {
                hints.push(format!("diversity={div:.2}"));
            }
            if let Some(rec) = training.get("recency").and_then(|v| v.as_f64()) {
                hints.push(format!("recency={rec:.2}"));
            }
            if let Some(comp) = training.get("compression").and_then(|v| v.as_f64()) {
                hints.push(format!("compression={comp:.2}"));
            }
            if let Some(tokens) = training.get("budget_tokens").and_then(|v| v.as_u64()) {
                hints.push(format!("budget_tokens={tokens}"));
            }
            if let Some(episodes) = training.get("episodes").and_then(|v| v.as_u64()) {
                hints.push(format!("episodes={episodes}"));
            }
            if !hints.is_empty() {
                out.push_str(&format!("  training: {}\n", hints.join(", ")));
            }
        }
    }
    out
}

fn follow_training_job(
    client: &Client,
    base: &str,
    token: Option<&str>,
    job_id: &str,
    persona_id: Option<&str>,
) -> Result<()> {
    println!("following {job_id}… (Ctrl-C to stop)");
    let mut last_status: Option<String> = None;
    let mut last_progress: Option<i64> = None;
    let mut printed_training = false;
    let mut printed_persona = false;
    let mut backoff_secs = 2_u64;
    let max_backoff = 20_u64;
    let mut not_found_logged = false;
    let start = Instant::now();

    loop {
        match fetch_job_snapshot(client, base, token, job_id) {
            Ok(Some(job)) => {
                not_found_logged = false;
                backoff_secs = 2;
                if !printed_persona {
                    if let Some(persona) = job
                        .get("persona_id")
                        .and_then(|v| v.as_str())
                        .or(persona_id)
                    {
                        println!("  persona: {persona}");
                        printed_persona = true;
                    }
                }
                if !printed_training {
                    if let Some(training) = job
                        .get("data")
                        .and_then(|v| v.get("training"))
                        .and_then(|v| v.as_object())
                    {
                        if !training.is_empty() {
                            let hints: Vec<String> = [
                                "preset",
                                "mode",
                                "diversity",
                                "recency",
                                "compression",
                                "budget_tokens",
                                "episodes",
                            ]
                            .iter()
                            .filter_map(|key| training.get(*key).map(|value| (*key, value)))
                            .map(|(key, value)| format!("{key}={value}"))
                            .collect();
                            if !hints.is_empty() {
                                println!("  training hints: {}", hints.join(", "));
                            }
                        }
                    }
                    printed_training = true;
                }

                let status = job
                    .get("status_label")
                    .and_then(|v| v.as_str())
                    .or_else(|| job.get("status").and_then(|v| v.as_str()))
                    .unwrap_or("Unknown")
                    .to_string();
                let slug = job
                    .get("status_slug")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let progress_pct = job
                    .get("progress")
                    .and_then(|v| v.as_f64())
                    .map(|p| (p * 100.0).round() as i64);

                let status_changed = last_status.as_deref() != Some(status.as_str());
                let progress_changed = progress_pct != last_progress;

                if status_changed || progress_changed {
                    if let Some(updated) = job.get("updated").and_then(|v| v.as_str()) {
                        if let Some(ms) = parse_rfc3339_millis(updated) {
                            let now_ms = chrono::Utc::now().timestamp_millis();
                            let now_ms = if now_ms < 0 { 0 } else { now_ms as u64 };
                            println!(
                                "  updated {} ({})",
                                format_local_timestamp(ms),
                                format_relative_from_now(ms, now_ms)
                            );
                        }
                    }
                }

                if status_changed {
                    println!("  status → {status} ({slug})");
                    last_status = Some(status.clone());
                }
                if progress_changed {
                    if let Some(pct) = progress_pct {
                        println!("  progress: {pct}%");
                    }
                    last_progress = progress_pct;
                }

                if is_terminal_status(slug.as_str()) {
                    if let Some(result) = job.get("result") {
                        println!(
                            "  result: {}",
                            serde_json::to_string_pretty(result)
                                .unwrap_or_else(|_| result.to_string())
                        );
                    }
                    println!("job {job_id} finished after {:.1?}", start.elapsed());
                    return Ok(());
                }
            }
            Ok(None) => {
                if !not_found_logged {
                    println!("  (job not visible yet; waiting)");
                    not_found_logged = true;
                }
            }
            Err(err) => {
                eprintln!("  follow error: {err:?}");
            }
        }
        thread::sleep(Duration::from_secs(backoff_secs));
        backoff_secs = (backoff_secs * 2).min(max_backoff);
    }
}

fn fetch_job_snapshot(
    client: &Client,
    base: &str,
    token: Option<&str>,
    job_id: &str,
) -> Result<Option<JsonValue>> {
    let url = format!("{base}/state/orchestrator/jobs?limit=200");
    let response = with_admin_headers(client.get(&url), token)
        .send()
        .with_context(|| format!("requesting orchestrator jobs from {url}"))?;
    if response.status().is_client_error() && response.status().as_u16() == 501 {
        bail!("kernel disabled: orchestrator jobs unavailable");
    }
    if !response.status().is_success() {
        bail!("jobs request failed: status {}", response.status().as_u16());
    }
    let payload: JsonValue = response
        .json()
        .context("parsing orchestrator jobs response")?;
    let items = payload
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(items.into_iter().find(|item| {
        item.get("id")
            .and_then(|v| v.as_str())
            .map(|id| id == job_id)
            .unwrap_or(false)
    }))
}

fn is_terminal_status(status_slug: &str) -> bool {
    matches!(status_slug, "completed" | "failed" | "cancelled")
}

fn clamp_unit(label: &str, value: Option<f64>) -> Result<Option<f64>> {
    if let Some(val) = value {
        if !(0.0..=1.0).contains(&val) {
            bail!("{label} must be between 0.0 and 1.0 (received {val})");
        }
        return Ok(Some(val));
    }
    Ok(None)
}

fn parse_rfc3339_millis(input: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(input)
        .map(|dt| {
            let ms = dt.timestamp_millis();
            if ms < 0 {
                None
            } else {
                Some(ms as u64)
            }
        })
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::blocking::Client;
    use std::time::Duration;

    #[test]
    fn build_start_payload_injects_persona_and_training_hints() -> Result<()> {
        let args = OrchestratorStartArgs {
            base: OrchestratorBaseArgs {
                base: "http://127.0.0.1:8091".into(),
                admin_token: None,
                timeout: 5,
            },
            goal: "Improve summarisation".into(),
            persona_id: Some("persona.alpha".into()),
            data_json: None,
            data_file: None,
            preset: Some("balanced".into()),
            mode: Some("guided".into()),
            diversity: Some(0.4),
            recency: Some(0.6),
            compression: Some(0.5),
            budget_tokens: Some(32000),
            episodes: Some(12),
            project: Some("demo".into()),
            topics: vec!["summary".into(), "persona".into()],
            json: false,
            pretty: false,
            follow: false,
        };

        let payload = build_start_payload(&args, Some("persona.alpha"), None)?;
        assert_eq!(
            payload.get("goal").and_then(|v| v.as_str()).expect("goal"),
            "Improve summarisation"
        );
        assert_eq!(
            payload
                .get("persona_id")
                .and_then(|v| v.as_str())
                .expect("persona_id"),
            "persona.alpha"
        );
        let data = payload
            .get("data")
            .and_then(|v| v.as_object())
            .expect("data");
        assert_eq!(
            data.get("persona_id")
                .and_then(|v| v.as_str())
                .expect("persona"),
            "persona.alpha"
        );
        assert_eq!(
            data.get("project")
                .and_then(|v| v.as_str())
                .expect("project"),
            "demo"
        );
        let topics = data
            .get("topics")
            .and_then(|v| v.as_array())
            .expect("topics");
        let topics: BTreeSet<_> = topics
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();
        assert!(topics.contains("summary"));
        assert!(topics.contains("persona"));

        let training = data
            .get("training")
            .and_then(|v| v.as_object())
            .expect("training");
        assert_eq!(
            training
                .get("preset")
                .and_then(|v| v.as_str())
                .expect("preset"),
            "balanced"
        );
        assert_eq!(
            training.get("mode").and_then(|v| v.as_str()).expect("mode"),
            "guided"
        );
        assert_eq!(
            training
                .get("budget_tokens")
                .and_then(|v| v.as_u64())
                .expect("budget_tokens"),
            32_000
        );
        assert_eq!(
            training
                .get("episodes")
                .and_then(|v| v.as_u64())
                .expect("episodes"),
            12
        );
        assert_eq!(
            training
                .get("diversity")
                .and_then(|v| v.as_f64())
                .expect("diversity"),
            0.4
        );
        assert_eq!(
            training
                .get("recency")
                .and_then(|v| v.as_f64())
                .expect("recency"),
            0.6
        );
        assert_eq!(
            training
                .get("compression")
                .and_then(|v| v.as_f64())
                .expect("compression"),
            0.5
        );
        Ok(())
    }

    #[test]
    fn build_start_payload_preserves_training_object_from_file() -> Result<()> {
        let mut base_map = JsonMap::new();
        base_map.insert("project".into(), JsonValue::String("existing".into()));
        base_map.insert(
            "training".into(),
            json!({
                "preset": "performance",
                "diversity": 0.25
            }),
        );
        let args = OrchestratorStartArgs {
            base: OrchestratorBaseArgs {
                base: "http://127.0.0.1:8091".into(),
                admin_token: None,
                timeout: 5,
            },
            goal: "Improve summarisation".into(),
            persona_id: None,
            data_json: None,
            data_file: None,
            preset: Some("quick".into()),
            mode: None,
            diversity: None,
            recency: Some(0.75),
            compression: None,
            budget_tokens: None,
            episodes: None,
            project: None,
            topics: vec![],
            json: false,
            pretty: false,
            follow: false,
        };
        let payload = build_start_payload(&args, None, Some(JsonValue::Object(base_map)))?;
        let data = payload
            .get("data")
            .and_then(|v| v.as_object())
            .expect("data object");
        assert_eq!(
            data.get("project")
                .and_then(|v| v.as_str())
                .expect("project"),
            "existing"
        );
        let training = data
            .get("training")
            .and_then(|v| v.as_object())
            .expect("training object");
        assert_eq!(
            training
                .get("preset")
                .and_then(|v| v.as_str())
                .expect("preset"),
            "quick"
        );
        assert_eq!(
            training
                .get("recency")
                .and_then(|v| v.as_f64())
                .expect("recency"),
            0.75
        );
        assert_eq!(
            training
                .get("diversity")
                .and_then(|v| v.as_f64())
                .expect("diversity"),
            0.25
        );
        Ok(())
    }

    #[test]
    fn clamp_unit_rejects_out_of_range() {
        assert!(clamp_unit("diversity", Some(-0.1)).is_err());
        assert!(clamp_unit("recency", Some(1.2)).is_err());
        assert!(clamp_unit("compression", Some(0.5)).is_ok());
    }

    #[test]
    fn follow_training_job_completes_when_terminal_status() -> Result<()> {
        use httpmock::prelude::*;

        let server = MockServer::start();
        let job_id = "job-123";
        let response_body = json!({
            "items": [{
                "id": job_id,
                "status": "completed",
                "status_slug": "completed",
                "progress": 1.0,
                "updated": "2025-10-22T12:00:00Z",
                "persona_id": "persona.alpha",
                "data": {
                    "training": {
                        "preset": "balanced",
                        "diversity": 0.4
                    }
                },
                "result": { "ok": true }
            }]
        });
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/state/orchestrator/jobs")
                .query_param("limit", "200");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(response_body.clone());
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("build http client for follow test")?;

        follow_training_job(
            &client,
            &server.base_url(),
            None,
            job_id,
            Some("persona.alpha"),
        )?;
        mock.assert();
        Ok(())
    }
}
