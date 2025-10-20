use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde_json::Value as JsonValue;

#[derive(Args, Clone)]
pub struct PingArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    pub admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    pub timeout: u64,
}

#[derive(Subcommand, Clone)]
pub enum SpecCmd {
    /// Fetch /spec/health and print JSON
    Health(SpecHealthArgs),
}

#[derive(Args, Clone)]
pub struct SpecHealthArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    pub base: String,
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

pub fn run_ping(args: &PingArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()?;
    let mut headers = HeaderMap::new();
    let token = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());
    if let Some(token_value) = token.as_deref() {
        let auth_value = HeaderValue::from_str(&format!("Bearer {}", token_value))
            .context("invalid bearer token for Authorization header")?;
        headers.insert(AUTHORIZATION, auth_value);
    }
    let health_resp = client
        .get(format!("{}/healthz", base))
        .headers(headers.clone())
        .send()?;
    let ok_health = health_resp.status().is_success();
    let about_resp = client
        .get(format!("{}/about", base))
        .headers(headers)
        .send()?;
    let about_json: JsonValue = about_resp.json().unwrap_or_else(|_| serde_json::json!({}));
    let payload = serde_json::json!({
        "base": base,
        "healthz": {"status": health_resp.status().as_u16()},
        "about": about_json,
        "ok": ok_health,
    });
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub fn execute_spec(cmd: SpecCmd) -> Result<()> {
    match cmd {
        SpecCmd::Health(args) => cmd_spec_health(&args),
    }
}

fn cmd_spec_health(args: &SpecHealthArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/spec/health", base);
    let client = Client::builder().timeout(Duration::from_secs(5)).build()?;
    let resp = client.get(url).send()?;
    let txt = resp.text()?;
    if args.pretty {
        let value: JsonValue = serde_json::from_str(&txt).unwrap_or_else(|_| serde_json::json!({}));
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", txt);
    }
    Ok(())
}
