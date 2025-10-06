use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use arw_events::Bus;
use arw_topics::TOPIC_IDENTITY_RELOADED;
use chrono::Utc;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use utoipa::ToSchema;

use crate::tasks::TaskHandle;

const MAX_PRINCIPALS: usize = 256;
const MAX_TOKEN_PER_PRINCIPAL: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("identity config path not set")]
    MissingPath,
    #[error("failed to read config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse tenants file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("tenants file exceeds limits")]
    LimitsExceeded,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IdentityPrincipal {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub source: IdentitySource,
}

impl IdentityPrincipal {
    fn normalize(mut self) -> Self {
        self.roles = normalize_vec(self.roles);
        self.scopes = normalize_vec(self.scopes);
        self.display_name = self.display_name.map(|name| name.trim().to_string());
        self
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles
            .iter()
            .any(|value| value.eq_ignore_ascii_case(role))
    }
}

#[derive(Clone, Copy, Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    Config,
    Env,
}

#[derive(Clone, Debug, Default, Serialize, ToSchema)]
pub struct IdentitySnapshot {
    pub loaded_ms: u64,
    pub source_path: Option<String>,
    pub version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub principals: Vec<IdentityPrincipalSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_principals: Vec<IdentityPrincipalSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IdentityPrincipalSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub roles: Vec<String>,
    pub scopes: Vec<String>,
    pub tokens: usize,
    pub source: IdentitySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Default)]
struct IdentityState {
    snapshot: IdentitySnapshot,
    by_token: HashMap<String, Arc<IdentityPrincipal>>,
    by_id: HashMap<String, Arc<IdentityPrincipal>>,
}

pub struct IdentityRegistry {
    bus: Bus,
    state: Arc<RwLock<IdentityState>>,
    config_path: Option<PathBuf>,
    env_tokens: Vec<EnvToken>,
}

static GLOBAL_REGISTRY: OnceCell<Arc<IdentityRegistry>> = OnceCell::new();

#[derive(Clone)]
struct EnvToken {
    fingerprint: String,
    principal: Arc<IdentityPrincipal>,
}

impl IdentityRegistry {
    pub async fn new(bus: Bus) -> Arc<Self> {
        let config_path = crate::config::identity_config_path();
        let env_tokens = load_env_tokens();
        let registry = Arc::new(Self {
            bus,
            state: Arc::new(RwLock::new(IdentityState::default())),
            config_path,
            env_tokens,
        });
        if let Err(err) = registry.reload().await {
            match err {
                IdentityError::MissingPath => {
                    debug!("no identity config discovered; env tokens only");
                }
                _ => warn!(error = %err, "failed to bootstrap identity registry"),
            }
        }
        registry
    }

    pub async fn reload(&self) -> Result<(), IdentityError> {
        let Some(path) = self.config_path.as_ref() else {
            return Err(IdentityError::MissingPath);
        };
        let bytes = tokio::fs::read(path).await?;
        let file: TenantsFile = toml::from_slice(&bytes)?;
        if file.principals.len() > MAX_PRINCIPALS {
            return Err(IdentityError::LimitsExceeded);
        }
        let mut diagnostics = Vec::new();
        let mut by_token: HashMap<String, Arc<IdentityPrincipal>> = HashMap::new();
        let mut by_id: HashMap<String, Arc<IdentityPrincipal>> = HashMap::new();
        let mut principal_summaries = Vec::new();

        for principal in file.principals {
            if principal.disabled {
                continue;
            }
            let Some(principal_id) = sanitize_id(&principal.id) else {
                diagnostics.push(format!("skipped principal `{}` (invalid id)", principal.id));
                continue;
            };
            let mut tokens = Vec::new();
            for token in principal.token_sha256.iter() {
                let trimmed = token.trim().to_ascii_lowercase();
                if trimmed.len() != 64 || !trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
                    diagnostics.push(format!(
                        "principal `{}` token ignored: invalid sha256 {token}",
                        principal_id
                    ));
                    continue;
                }
                tokens.push(trimmed);
            }
            if tokens.is_empty() {
                diagnostics.push(format!("principal `{}` has no tokens", principal_id));
                continue;
            }
            if tokens.len() > MAX_TOKEN_PER_PRINCIPAL {
                diagnostics.push(format!(
                    "principal `{}` token count exceeds limit ({})",
                    principal_id, MAX_TOKEN_PER_PRINCIPAL
                ));
                continue;
            }
            let principal_obj = Arc::new(
                IdentityPrincipal {
                    id: principal_id.clone(),
                    display_name: principal
                        .display_name
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                    roles: principal.roles.unwrap_or_default(),
                    scopes: principal.scopes.unwrap_or_default(),
                    source: IdentitySource::Config,
                }
                .normalize(),
            );
            for fp in tokens {
                if by_token.contains_key(&fp) {
                    diagnostics.push(format!(
                        "fingerprint collision for principal `{}`; token skipped",
                        principal_id
                    ));
                    continue;
                }
                by_token.insert(fp, principal_obj.clone());
            }
            principal_summaries.push(IdentityPrincipalSummary {
                id: principal_obj.id.clone(),
                display_name: principal_obj.display_name.clone(),
                roles: principal_obj.roles.clone(),
                scopes: principal_obj.scopes.clone(),
                tokens: by_token
                    .values()
                    .filter(|p| p.id == principal_obj.id)
                    .count(),
                source: IdentitySource::Config,
                notes: None,
            });
            by_id.insert(principal_obj.id.clone(), principal_obj);
        }

        let mut env_summaries = Vec::new();
        for env in &self.env_tokens {
            env_summaries.push(IdentityPrincipalSummary {
                id: env.principal.id.clone(),
                display_name: env.principal.display_name.clone(),
                roles: env.principal.roles.clone(),
                scopes: env.principal.scopes.clone(),
                tokens: 1,
                source: IdentitySource::Env,
                notes: Some("env token".into()),
            });
        }

        let snapshot = IdentitySnapshot {
            loaded_ms: Utc::now().timestamp_millis() as u64,
            source_path: Some(path.display().to_string()),
            version: file.version.unwrap_or(1),
            principals: principal_summaries,
            env_principals: env_summaries,
            diagnostics,
        };

        {
            let mut guard = self.state.write().await;
            guard.snapshot = snapshot;
            guard.by_token = by_token;
            guard.by_id = by_id;
        }

        self.bus.publish(
            TOPIC_IDENTITY_RELOADED,
            &serde_json::json!({
                "path": path.display().to_string(),
                "loaded_ms": Utc::now().timestamp_millis(),
            }),
        );
        info!(
            path = %path.display(),
            "identity registry reloaded (principals={}, env={})",
            self.state_tokens_len().await,
            self.env_tokens.len()
        );
        Ok(())
    }

    pub async fn snapshot(&self) -> IdentitySnapshot {
        let guard = self.state.read().await;
        guard.snapshot.clone()
    }

    pub async fn verify_token(&self, presented: &str) -> Option<Arc<IdentityPrincipal>> {
        let fingerprint = sha256_hex(presented);

        if let Some(env) = self
            .env_tokens
            .iter()
            .find(|entry| entry.fingerprint == fingerprint)
        {
            return Some(env.principal.clone());
        }

        let guard = self.state.read().await;
        guard.by_token.get(&fingerprint).cloned()
    }

    pub fn watch(self: &Arc<Self>) -> Option<TaskHandle> {
        let path = self.config_path.clone()?;
        let registry = self.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(3));
            let mut last_modified = None;
            loop {
                ticker.tick().await;
                match tokio::fs::metadata(&path).await {
                    Ok(metadata) => {
                        let modified = metadata.modified().ok();
                        if modified == last_modified {
                            continue;
                        }
                        last_modified = modified;
                        if let Err(err) = registry.reload().await {
                            warn!(path = %path.display(), %err, "identity reload failed");
                        }
                    }
                    Err(err) => {
                        warn!(path = %path.display(), error = %err, "identity metadata unavailable");
                    }
                }
            }
        });
        Some(TaskHandle::new("config.watch.identity", handle))
    }

    async fn state_tokens_len(&self) -> usize {
        let guard = self.state.read().await;
        guard.by_token.len()
    }
}

fn normalize_vec(values: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.to_string();
        if !seen
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&normalized))
        {
            seen.push(normalized);
        }
    }
    seen
}

fn sanitize_id(raw: &str) -> Option<String> {
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

fn sha256_hex(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn load_env_tokens() -> Vec<EnvToken> {
    let mut tokens = Vec::new();
    if let Ok(value) = std::env::var("ARW_ADMIN_TOKEN") {
        if !value.trim().is_empty() {
            let fingerprint = sha256_hex(value.trim());
            tokens.push(EnvToken {
                fingerprint,
                principal: Arc::new(
                    IdentityPrincipal {
                        id: "env:admin".into(),
                        display_name: Some("Env Admin Token".into()),
                        roles: vec!["admin".into()],
                        scopes: vec!["*".into()],
                        source: IdentitySource::Env,
                    }
                    .normalize(),
                ),
            });
        }
    }
    if let Ok(hash) = std::env::var("ARW_ADMIN_TOKEN_SHA256") {
        let trimmed = hash.trim().to_ascii_lowercase();
        if trimmed.len() == 64 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
            tokens.push(EnvToken {
                fingerprint: trimmed,
                principal: Arc::new(
                    IdentityPrincipal {
                        id: "env:admin-hash".into(),
                        display_name: Some("Env Admin (hashed)".into()),
                        roles: vec!["admin".into()],
                        scopes: vec!["*".into()],
                        source: IdentitySource::Env,
                    }
                    .normalize(),
                ),
            });
        } else if !trimmed.is_empty() {
            warn!("ARW_ADMIN_TOKEN_SHA256 is not a valid 64-character hex digest; ignoring");
        }
    }
    tokens
}

impl Clone for IdentityRegistry {
    fn clone(&self) -> Self {
        Self {
            bus: self.bus.clone(),
            state: self.state.clone(),
            config_path: self.config_path.clone(),
            env_tokens: self.env_tokens.clone(),
        }
    }
}

pub fn set_global_registry(registry: Arc<IdentityRegistry>) {
    let _ = GLOBAL_REGISTRY.set(registry);
}

pub fn global_registry() -> Option<Arc<IdentityRegistry>> {
    GLOBAL_REGISTRY.get().cloned()
}

#[derive(Debug, Deserialize)]
struct TenantsFile {
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    principals: Vec<TenantPrincipal>,
}

#[derive(Debug, Deserialize)]
struct TenantPrincipal {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(default)]
    scopes: Option<Vec<String>>,
    #[serde(default)]
    token_sha256: Vec<String>,
    #[serde(default)]
    disabled: bool,
}
