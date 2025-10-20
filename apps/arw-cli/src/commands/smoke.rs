use std::env;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Context, Result};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;
use serde_json::{json, Value as JsonValue};
use sha2::Digest;
use tempfile::TempDir;

use crate::{submit_action_payload, wait_for_action};

#[derive(Args, Clone)]
pub struct SmokeCommonArgs {
    /// Port to bind the temporary server on (default: 18181 / 18182)
    #[arg(long)]
    pub port: Option<u16>,
    /// Path to an existing arw-server binary; auto-detected when omitted
    #[arg(long)]
    pub server_bin: Option<PathBuf>,
    /// Seconds to wait for /healthz to become ready
    #[arg(long, default_value_t = 30)]
    pub wait_timeout_secs: u64,
    /// Preserve the temporary state/logs directory instead of deleting it
    #[arg(long)]
    pub keep_temp: bool,
}

#[derive(Args, Clone)]
pub struct SmokeTriadArgs {
    #[command(flatten)]
    pub common: SmokeCommonArgs,
    /// Admin token to use; defaults to an ephemeral value
    #[arg(long)]
    pub admin_token: Option<String>,
}

#[derive(Args, Clone)]
pub struct SmokeContextArgs {
    #[command(flatten)]
    pub common: SmokeCommonArgs,
    /// Admin token to use; defaults to an ephemeral value
    #[arg(long)]
    pub admin_token: Option<String>,
}

#[derive(Subcommand)]
pub enum SmokeCmd {
    /// Run action/state/event smoke checks
    Triad(SmokeTriadArgs),
    /// Run context telemetry smoke checks
    Context(SmokeContextArgs),
}

pub fn execute(cmd: SmokeCmd) -> Result<()> {
    match cmd {
        SmokeCmd::Triad(args) => cmd_smoke_triad(&args),
        SmokeCmd::Context(args) => cmd_smoke_context(&args),
    }
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

        thread::sleep(Duration::from_millis(400));
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
    submit_action_payload(
        client,
        base,
        admin_token,
        "demo.echo",
        json!({ "msg": message }),
    )
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
    match fs::read_to_string(log_path) {
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
