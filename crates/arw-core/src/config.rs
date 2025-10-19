use anyhow::Result;
use jsonschema::{validator_for, Validator};
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub portable: Option<bool>,
    #[serde(default)]
    pub state_dir: Option<String>,
    #[serde(default)]
    pub cache_dir: Option<String>,
    #[serde(default)]
    pub logs_dir: Option<String>,
    /// Optional HTTP port for the local service
    #[serde(default)]
    pub port: Option<u16>,
    /// Optional external base URL for reverse-proxy (e.g., <https://arw.example.com>)
    #[serde(default)]
    pub external_base_url: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct Config {
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
}

static CONFIG_SCHEMA: Lazy<Validator> = Lazy::new(|| {
    let schema = schemars::schema_for!(Config);
    let schema_value = serde_json::to_value(&schema).expect("schema value");
    validator_for(&schema_value).expect("valid schema")
});

/// Returns the JSON schema describing the configuration structure.
///
/// # Panics
///
/// Panics if schema generation fails; this indicates a programming error.
pub fn config_schema_json() -> serde_json::Value {
    let schema = schemars::schema_for!(Config);
    serde_json::to_value(&schema).expect("schema json")
}

pub fn write_schema_file(path: &str) -> std::io::Result<()> {
    let schema_json = config_schema_json();
    std::fs::write(path, serde_json::to_string_pretty(&schema_json)?)
}

pub fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    let raw: toml::Value = toml::from_str(&content)?;
    let json_value = serde_json::to_value(&raw)?;
    let validation_errors: Vec<_> = CONFIG_SCHEMA
        .iter_errors(&json_value)
        .map(|e| e.to_string())
        .collect();
    if !validation_errors.is_empty() {
        return Err(anyhow::anyhow!(validation_errors.join(", ")));
    }
    let cfg: Config = toml::from_str(&content)?;
    Ok(cfg)
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
pub struct ClusterConfig {
    /// Enable multi-core/connector mode.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Event bus backend: "local" (default), "nats".
    #[serde(default)]
    pub bus: Option<String>,
    /// Work queue backend: "local" (default), "nats".
    #[serde(default)]
    pub queue: Option<String>,
    /// NATS connection URL, e.g. <nats://127.0.0.1:4222>
    #[serde(default)]
    pub nats_url: Option<String>,
    /// Optional explicit node id (defaults to hostname)
    #[serde(default)]
    pub node_id: Option<String>,
}
