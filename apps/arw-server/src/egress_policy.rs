use serde_json::{json, Value};

use crate::{capsule_guard, AppState};

fn domain_suffix(host: &str) -> Option<String> {
    host.find('.').and_then(|idx| {
        let s = &host[idx + 1..];
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    Off,
    Relaxed,
    Public,
    Standard,
    Allowlist,
    Custom,
    Strict,
}

impl Posture {
    pub fn from_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Posture::Off,
            "relaxed" => Posture::Relaxed,
            "allowlist" => Posture::Allowlist,
            "custom" => Posture::Custom,
            "strict" => Posture::Strict,
            "public" => Posture::Public,
            _ => Posture::Standard,
        }
    }

    pub fn effective(self) -> Self {
        match self {
            Posture::Relaxed => Posture::Off,
            Posture::Standard => Posture::Public,
            Posture::Strict => Posture::Allowlist,
            other => other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Posture::Off => "off",
            Posture::Relaxed => "relaxed",
            Posture::Public => "public",
            Posture::Standard => "standard",
            Posture::Allowlist => "allowlist",
            Posture::Custom => "custom",
            Posture::Strict => "strict",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AllowRule {
    suffix: String,
    wildcard: bool,
    port: Option<u16>,
}

impl AllowRule {
    fn new(pattern: &str) -> Option<Self> {
        let pat = pattern.trim();
        if pat.is_empty() {
            return None;
        }
        let (host, port) = if let Some((h, p)) = pat.rsplit_once(':') {
            if let Ok(port) = p.parse::<u16>() {
                (h.to_string(), Some(port))
            } else {
                (pat.to_string(), None)
            }
        } else {
            (pat.to_string(), None)
        };
        let wildcard = host.starts_with("*.");
        let suffix = if wildcard {
            host.trim_start_matches("*.").to_ascii_lowercase()
        } else {
            host.to_ascii_lowercase()
        };
        Some(Self {
            suffix,
            wildcard,
            port,
        })
    }

    fn matches(&self, host: &str, port: Option<u16>) -> bool {
        if let Some(rule_port) = self.port {
            if port != Some(rule_port) {
                return false;
            }
        }
        if self.wildcard {
            host.ends_with(&self.suffix)
        } else {
            host == self.suffix
        }
    }
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        ),
        Err(_) => default,
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPolicy {
    pub posture: Posture,
    pub allow_rules: Vec<AllowRule>,
    pub block_ip_literals: bool,
    pub dns_guard_enabled: bool,
    pub proxy_enabled: bool,
    pub ledger_enabled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PostureDefaults {
    pub block_ip_literals: bool,
    pub dns_guard_enabled: bool,
    pub proxy_enabled: bool,
    pub ledger_enabled: bool,
}

pub fn posture_defaults(posture: Posture) -> PostureDefaults {
    match posture {
        Posture::Off => PostureDefaults {
            block_ip_literals: false,
            dns_guard_enabled: false,
            proxy_enabled: false,
            ledger_enabled: false,
        },
        Posture::Relaxed => PostureDefaults {
            block_ip_literals: false,
            dns_guard_enabled: false,
            proxy_enabled: false,
            ledger_enabled: false,
        },
        Posture::Public => PostureDefaults {
            block_ip_literals: true,
            dns_guard_enabled: true,
            proxy_enabled: true,
            ledger_enabled: true,
        },
        Posture::Allowlist | Posture::Custom | Posture::Strict => PostureDefaults {
            block_ip_literals: true,
            dns_guard_enabled: true,
            proxy_enabled: true,
            ledger_enabled: true,
        },
        Posture::Standard => posture_defaults(Posture::Public),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DenyReason {
    IpLiteral,
    HostNotAllowed,
    PortNotAllowed,
    SchemeUnsupported,
}

#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub allow: bool,
    pub reason: Option<DenyReason>,
}

impl PolicyDecision {
    pub fn allow() -> Self {
        Self {
            allow: true,
            reason: None,
        }
    }

    pub fn deny(reason: DenyReason) -> Self {
        Self {
            allow: false,
            reason: Some(reason),
        }
    }
}

pub(crate) fn env_allowlist() -> Vec<String> {
    std::env::var("ARW_NET_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn config_allowlist(cfg: &Value) -> Vec<String> {
    cfg.get("egress")
        .and_then(|v| v.get("allowlist"))
        .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
        .unwrap_or_default()
}

fn env_posture() -> Option<String> {
    std::env::var("ARW_NET_POSTURE")
        .ok()
        .or_else(|| std::env::var("ARW_SECURITY_POSTURE").ok())
}

fn config_posture(cfg: &Value) -> Option<String> {
    cfg.get("egress")
        .and_then(|v| v.get("posture"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn default_public_allowlist() -> Vec<&'static str> {
    vec![
        "github.com",
        "api.github.com",
        "*.githubusercontent.com",
        "crates.io",
        "static.crates.io",
        "pypi.org",
        "files.pythonhosted.org",
        "registry.npmjs.org",
        "cdn.npmjs.org",
        "dl.yarnpkg.com",
        "go.dev",
        "proxy.golang.org",
        "*.docker.com",
        "*.docker.io",
        "index.docker.io",
        "auth.docker.io",
        "huggingface.co",
        "cdn-lfs.huggingface.co",
        "objects.githubusercontent.com",
        "raw.githubusercontent.com",
    ]
}

fn merge_allowlists(base: Vec<String>, extra: Vec<String>) -> Vec<AllowRule> {
    let mut rules: Vec<AllowRule> = Vec::new();
    for item in base.into_iter().chain(extra.into_iter()) {
        if let Some(rule) = AllowRule::new(&item) {
            rules.push(rule);
        }
    }
    rules
}

pub async fn resolve_policy(state: &AppState) -> ResolvedPolicy {
    capsule_guard::refresh_capsules(state).await;
    let cfg = state.config_state().lock().await.clone();
    let posture_str = env_posture()
        .or_else(|| config_posture(&cfg))
        .unwrap_or_else(|| "standard".into());
    let posture = Posture::from_str(&posture_str).effective();
    let defaults = posture_defaults(posture);
    let block_ip_literals = env_flag("ARW_EGRESS_BLOCK_IP_LITERALS", defaults.block_ip_literals);
    let dns_guard_enabled = env_flag("ARW_DNS_GUARD_ENABLE", defaults.dns_guard_enabled);
    let proxy_enabled = env_flag("ARW_EGRESS_PROXY_ENABLE", defaults.proxy_enabled);
    let ledger_enabled = env_flag("ARW_EGRESS_LEDGER_ENABLE", defaults.ledger_enabled);

    let env_list = env_allowlist();
    let cfg_list = config_allowlist(&cfg);

    let allow_rules = match posture {
        Posture::Off => Vec::new(),
        Posture::Public => {
            let mut base: Vec<String> = default_public_allowlist()
                .into_iter()
                .map(|s| s.to_string())
                .collect();
            base.extend(cfg_list);
            base.extend(env_list);
            merge_allowlists(base, Vec::new())
        }
        Posture::Allowlist | Posture::Custom => merge_allowlists(cfg_list, env_list),
        Posture::Standard => unreachable!(),
        Posture::Relaxed => Vec::new(),
        Posture::Strict => merge_allowlists(cfg_list, env_list),
    };

    ResolvedPolicy {
        posture,
        allow_rules,
        block_ip_literals,
        dns_guard_enabled,
        proxy_enabled,
        ledger_enabled,
    }
}

pub fn evaluate(
    policy: &ResolvedPolicy,
    host: Option<&str>,
    port: Option<u16>,
    scheme: &str,
) -> PolicyDecision {
    match policy.posture {
        Posture::Off => return PolicyDecision::allow(),
        Posture::Relaxed => return PolicyDecision::allow(),
        _ => {}
    }

    let host = match host {
        Some(h) => h.to_ascii_lowercase(),
        None => return PolicyDecision::deny(DenyReason::HostNotAllowed),
    };

    if policy.block_ip_literals && host.parse::<std::net::IpAddr>().is_ok() {
        return PolicyDecision::deny(DenyReason::IpLiteral);
    }

    match scheme {
        "http" | "https" => {}
        _ => return PolicyDecision::deny(DenyReason::SchemeUnsupported),
    }

    match policy.posture {
        Posture::Public | Posture::Allowlist | Posture::Custom | Posture::Strict => {
            if policy.allow_rules.is_empty() {
                return PolicyDecision::deny(DenyReason::HostNotAllowed);
            }
            let mut host_allowed = false;
            let mut port_allowed = false;
            for rule in &policy.allow_rules {
                if rule.matches(&host, None) {
                    host_allowed = true;
                    if port.is_none() || rule.port.is_none() {
                        port_allowed = true;
                    }
                }
                if let Some(p) = port {
                    if rule.matches(&host, Some(p)) {
                        host_allowed = true;
                        port_allowed = true;
                        break;
                    }
                }
            }
            if !host_allowed {
                return PolicyDecision::deny(DenyReason::HostNotAllowed);
            }
            if let Some(p) = port {
                if !port_allowed {
                    if policy.posture == Posture::Public {
                        if !matches!(p, 80 | 443) {
                            return PolicyDecision::deny(DenyReason::PortNotAllowed);
                        }
                    } else if !(matches!(p, 80 | 443)) {
                        return PolicyDecision::deny(DenyReason::PortNotAllowed);
                    }
                }
            }
        }
        Posture::Standard | Posture::Relaxed | Posture::Off => {}
    }

    if let Some(p) = port {
        if matches!(policy.posture, Posture::Public | Posture::Standard) && !matches!(p, 80 | 443) {
            return PolicyDecision::deny(DenyReason::PortNotAllowed);
        }
    }

    PolicyDecision::allow()
}

pub fn reason_code(reason: DenyReason) -> &'static str {
    match reason {
        DenyReason::IpLiteral => "ip_literal",
        DenyReason::HostNotAllowed => "allowlist",
        DenyReason::PortNotAllowed => "port",
        DenyReason::SchemeUnsupported => "scheme",
    }
}

pub fn capability_candidates(host: Option<&str>, port: Option<u16>, scheme: &str) -> Vec<String> {
    let mut caps: Vec<String> = Vec::new();
    if let Some(h) = host {
        caps.push(format!("net:host:{}", h));
        if let Some(domain) = domain_suffix(h) {
            caps.push(format!("net:domain:{}", domain));
        }
    }
    if let Some(p) = port {
        caps.push(format!("net:port:{}", p));
    }
    caps.push(format!("net:{}", scheme));
    if scheme != "http" {
        caps.push("net:http".into());
    }
    if scheme != "https" {
        caps.push("net:https".into());
    }
    caps.push("net:tcp".into());
    caps.push("net".into());
    caps.sort();
    caps.dedup();
    caps
}

pub async fn lease_grant(state: &AppState, caps: &[String]) -> Option<Value> {
    if !state.kernel_enabled() {
        return None;
    }
    let kernel = state.kernel_if_enabled()?;
    for cap in caps {
        if let Ok(Some(mut lease)) = kernel.find_valid_lease_async("local", cap).await {
            if let Some(obj) = lease.as_object_mut() {
                obj.entry("matched_capability")
                    .or_insert_with(|| json!(cap));
            }
            return Some(lease);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_matching() {
        let policy = ResolvedPolicy {
            posture: Posture::Allowlist,
            allow_rules: merge_allowlists(vec!["*.example.com".into()], Vec::new()),
            block_ip_literals: false,
            dns_guard_enabled: true,
            proxy_enabled: true,
            ledger_enabled: true,
        };
        assert!(matches!(
            evaluate(&policy, Some("api.example.com"), Some(443), "https"),
            PolicyDecision { allow: true, .. }
        ));
        assert!(matches!(
            evaluate(&policy, Some("example.org"), Some(443), "https"),
            PolicyDecision { allow: false, .. }
        ));
    }
}
