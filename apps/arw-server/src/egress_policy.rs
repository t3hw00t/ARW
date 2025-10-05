use serde_json::{json, Value};
use std::{
    cell::RefCell,
    sync::{Mutex, OnceLock},
};

use crate::{capsule_guard, AppState};

fn domain_suffix(host: &str) -> Option<String> {
    use std::net::IpAddr;

    let trimmed = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.parse::<IpAddr>().is_ok() {
        return None;
    }

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    let last = parts.last().copied().unwrap();
    let penultimate = parts[parts.len() - 2];
    let candidate = format!("{penultimate}.{last}");

    for suffix in combined_multi_label_suffixes().iter() {
        let len = suffix.len();
        if parts.len() < len {
            continue;
        }
        let start = parts.len() - len;
        if parts[start..]
            .iter()
            .zip(suffix.iter())
            .all(|(host_part, cfg_part)| host_part == cfg_part)
        {
            let joined = suffix.join(".");
            if start == 0 {
                return Some(joined);
            }
            let owner = parts[start - 1];
            return Some(format!("{owner}.{joined}"));
        }
    }

    if is_predefined_multi_label(penultimate, last) && parts.len() >= 3 {
        let registrable = format!("{}.{}", parts[parts.len() - 3], candidate);
        return Some(registrable);
    }

    Some(candidate)
}

fn is_predefined_multi_label(second: &str, tld: &str) -> bool {
    match tld {
        "uk" => matches!(
            second,
            "co" | "org" | "gov" | "ac" | "sch" | "ltd" | "plc" | "me"
        ),
        "jp" => matches!(
            second,
            "co" | "or" | "ne" | "ac" | "ad" | "ed" | "go" | "gr" | "lg"
        ),
        "nz" => matches!(second, "co" | "org" | "govt" | "ac" | "geek"),
        "au" => matches!(second, "com" | "net" | "org" | "edu" | "gov" | "csiro"),
        "br" => matches!(second, "com" | "gov" | "edu" | "org" | "net"),
        "cn" => matches!(second, "com" | "net" | "org" | "gov" | "edu" | "ac" | "mil"),
        "in" => matches!(second, "co" | "org" | "ac" | "gov" | "net" | "res"),
        "id" => matches!(second, "co" | "or" | "ac" | "go" | "net" | "web" | "my"),
        "sg" => matches!(second, "com" | "net" | "org" | "gov" | "edu" | "per"),
        "hk" => matches!(second, "com" | "net" | "org" | "gov" | "edu" | "idv"),
        "kr" => matches!(second, "co" | "or" | "ne" | "go" | "re" | "pe"),
        "tw" => matches!(second, "com" | "net" | "org" | "gov" | "edu" | "idv"),
        "mx" => matches!(second, "com" | "org" | "gob" | "edu" | "net"),
        "ar" => matches!(second, "com" | "gob" | "edu" | "org" | "net"),
        "cl" => matches!(second, "com" | "gob" | "edu" | "org" | "net"),
        "pe" => matches!(second, "com" | "gob" | "edu" | "org" | "net"),
        "ph" => matches!(second, "com" | "gov" | "edu" | "org" | "net"),
        "th" => matches!(second, "com" | "go" | "ac" | "net" | "or" | "in"),
        "sa" => matches!(second, "com" | "gov" | "edu" | "med" | "net" | "org"),
        "za" => matches!(second, "co" | "gov" | "ac" | "org" | "net" | "law" | "mil"),
        _ => false,
    }
}

thread_local! {
    static ENV_MULTI_LABEL_SUFFIXES: RefCell<(Option<String>, Vec<Vec<String>>)> =
        const { RefCell::new((None, Vec::new())) };
}

static CONFIG_MULTI_LABEL_SUFFIXES: OnceLock<Mutex<Vec<Vec<String>>>> = OnceLock::new();

#[cfg(test)]
fn reset_env_multi_label_suffix_cache() {
    ENV_MULTI_LABEL_SUFFIXES.with(|cache| {
        *cache.borrow_mut() = (None, Vec::new());
    });
}

pub(crate) fn parse_multi_label_suffix(entry: &str) -> Option<Vec<String>> {
    let trimmed = entry.trim().trim_start_matches('.');
    if trimmed.is_empty() || !trimmed.contains('.') {
        return None;
    }
    let parts = trimmed
        .split('.')
        .filter_map(|segment| {
            let seg = segment.trim();
            if seg.is_empty() {
                None
            } else {
                Some(seg.to_ascii_lowercase())
            }
        })
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return None;
    }
    Some(parts)
}

pub(crate) fn env_multi_label_suffixes() -> Vec<Vec<String>> {
    ENV_MULTI_LABEL_SUFFIXES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let raw = std::env::var("ARW_EGRESS_MULTI_LABEL_SUFFIXES").unwrap_or_default();
        if cache.0.as_deref() != Some(&raw) {
            let parsed = raw
                .split(',')
                .filter_map(parse_multi_label_suffix)
                .collect::<Vec<_>>();
            cache.0 = Some(raw);
            cache.1 = parsed;
        }
        cache.1.clone()
    })
}

fn configured_multi_label_suffixes() -> Vec<Vec<String>> {
    if let Some(lock) = CONFIG_MULTI_LABEL_SUFFIXES.get() {
        let guard = match lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        (*guard).clone()
    } else {
        Vec::new()
    }
}

fn combined_multi_label_suffixes() -> Vec<Vec<String>> {
    let mut combined = configured_multi_label_suffixes();
    combined.extend(env_multi_label_suffixes());
    combined
}

pub(crate) fn set_configured_multi_label_suffixes(entries: Vec<Vec<String>>) {
    let lock = CONFIG_MULTI_LABEL_SUFFIXES.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = entries;
}

#[cfg(test)]
fn reset_configured_multi_label_suffixes() {
    if let Some(lock) = CONFIG_MULTI_LABEL_SUFFIXES.get() {
        let mut guard = match lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.clear();
    }
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
        let host_norm = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if let Some(rule_port) = self.port {
            if port != Some(rule_port) {
                return false;
            }
        }
        if self.wildcard {
            let suffix = &self.suffix;
            if host_norm.len() <= suffix.len() {
                return false;
            }
            if !host_norm.ends_with(suffix) {
                return false;
            }
            let boundary_idx = host_norm.len() - suffix.len() - 1;
            matches!(host_norm.as_bytes().get(boundary_idx), Some(b'.'))
        } else {
            host_norm == self.suffix
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

pub(crate) fn config_multi_label_suffixes(cfg: &Value) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let source = cfg
        .get("egress")
        .and_then(|v| v.get("multi_label_suffixes"));
    match source {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    if let Some(parts) = parse_multi_label_suffix(s) {
                        out.push(parts);
                    }
                }
            }
        }
        Some(Value::String(s)) => {
            if let Some(parts) = parse_multi_label_suffix(s) {
                out.push(parts);
            }
        }
        _ => {}
    }
    out
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
    set_configured_multi_label_suffixes(config_multi_label_suffixes(&cfg));
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
    use crate::test_support;
    use crate::test_support::env as test_env;
    use arw_policy::PolicyEngine;
    use arw_protocol::GatingCapsule;
    use arw_wasi::NoopHost;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    #[test]
    fn allow_rule_wildcard_requires_label_boundary() {
        let rule = AllowRule::new("*.example.com").expect("rule");
        assert!(rule.matches("api.example.com", None));
        assert!(rule.matches("deep.branch.example.com", None));
        assert!(!rule.matches("example.com", None));
        assert!(!rule.matches("badexample.com", None));
    }

    #[test]
    fn domain_suffix_handles_apex_and_subdomains() {
        assert_eq!(
            domain_suffix("example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            domain_suffix("www.example.com"),
            Some("example.com".to_string())
        );
        assert_eq!(
            domain_suffix("sub.service.example.com"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn domain_suffix_handles_common_multi_label_suffixes() {
        assert_eq!(
            domain_suffix("foo.example.co.uk"),
            Some("example.co.uk".to_string())
        );
        assert_eq!(
            domain_suffix("example.co.uk"),
            Some("example.co.uk".to_string())
        );
        assert_eq!(
            domain_suffix("bar.example.com.au"),
            Some("example.com.au".to_string())
        );
    }

    #[test]
    fn domain_suffix_handles_extended_multi_label_suffixes() {
        assert_eq!(
            domain_suffix("service.example.co.in"),
            Some("example.co.in".to_string())
        );
        assert_eq!(
            domain_suffix("api.example.com.sg"),
            Some("example.com.sg".to_string())
        );
        assert_eq!(
            domain_suffix("chat.example.co.kr"),
            Some("example.co.kr".to_string())
        );
    }

    #[test]
    fn domain_suffix_respects_env_extensions() {
        const KEY: &str = "ARW_EGRESS_MULTI_LABEL_SUFFIXES";
        let mut env = test_env::guard();

        reset_configured_multi_label_suffixes();
        reset_env_multi_label_suffix_cache();
        env.set(KEY, "internal.test,gov.bc.ca");
        let env_suffixes = env_multi_label_suffixes();
        assert!(env_suffixes
            .iter()
            .any(|suffix| suffix == &["internal".to_string(), "test".to_string()]));
        assert_eq!(
            domain_suffix("service.example.internal.test"),
            Some("example.internal.test".to_string())
        );
        assert_eq!(
            domain_suffix("node.team.internal.test"),
            Some("team.internal.test".to_string())
        );
        assert_eq!(
            domain_suffix("app.utilities.gov.bc.ca"),
            Some("utilities.gov.bc.ca".to_string())
        );

        reset_configured_multi_label_suffixes();
        reset_env_multi_label_suffix_cache();
        let _ = domain_suffix("example.com");
    }

    #[test]
    fn domain_suffix_respects_config_extensions() {
        const KEY: &str = "ARW_EGRESS_MULTI_LABEL_SUFFIXES";
        let mut env = test_env::guard();
        env.remove(KEY);

        reset_configured_multi_label_suffixes();
        reset_env_multi_label_suffix_cache();
        set_configured_multi_label_suffixes(vec![vec!["gov".into(), "bc".into(), "ca".into()]]);
        assert_eq!(
            domain_suffix("app.utilities.gov.bc.ca"),
            Some("utilities.gov.bc.ca".to_string())
        );
        reset_configured_multi_label_suffixes();
        reset_env_multi_label_suffix_cache();
        let _ = domain_suffix("example.com");
    }

    #[test]
    fn domain_suffix_ignores_ip_literals() {
        assert_eq!(domain_suffix("127.0.0.1"), None);
        assert_eq!(domain_suffix("::1"), None);
    }

    #[test]
    fn allow_rule_exact_and_port_matching_are_case_insensitive() {
        let rule = AllowRule::new("Foo.Example.com:8443").expect("rule");
        assert!(rule.matches("foo.example.com", Some(8443)));
        assert!(rule.matches("FOO.EXAMPLE.COM", Some(8443)));
        assert!(!rule.matches("foo.example.com", Some(443)));
        assert!(!rule.matches("foo.example.com", None));
    }

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

    async fn build_state(dir: &Path, env_guard: &mut test_env::EnvGuard) -> AppState {
        env_guard.set("ARW_DEBUG", "1");
        crate::util::reset_state_dir_for_tests();
        env_guard.set("ARW_STATE_DIR", dir.display().to_string());
        let bus = arw_events::Bus::new_with_replay(64, 64);
        let kernel = arw_kernel::Kernel::open(dir).expect("init kernel for tests");
        let policy = PolicyEngine::load_from_env();
        let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
        let host: Arc<dyn arw_wasi::ToolHost> = Arc::new(NoopHost);
        AppState::builder(bus, kernel, policy_handle, host, true)
            .with_config_state(Arc::new(Mutex::new(json!({"mode": "test"}))))
            .with_config_history(Arc::new(Mutex::new(Vec::new())))
            .with_sse_capacity(64)
            .build()
            .await
    }

    fn capsule_with_hops(id: &str, ttl: u32) -> GatingCapsule {
        GatingCapsule {
            id: id.to_string(),
            version: "1".into(),
            issued_at_ms: 0,
            issuer: Some("issuer".into()),
            hop_ttl: Some(ttl),
            propagate: None,
            denies: vec![],
            contracts: vec![],
            lease_duration_ms: Some(60_000),
            renew_within_ms: Some(10_000),
            signature: Some("sig".into()),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    #[tokio::test]
    async fn resolve_policy_refreshes_capsules() {
        let temp = tempdir().expect("tempdir");
        let mut ctx = test_support::begin_state_env(temp.path());
        let state = build_state(temp.path(), &mut ctx.env).await;

        let capsule = capsule_with_hops("egress-refresh", 3);
        state.capsules().adopt(&capsule, now_ms()).await;

        let before = state.capsules().snapshot().await;
        let items = before["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["remaining_hops"].as_u64(), Some(2));

        let _ = resolve_policy(&state).await;

        let after = state.capsules().snapshot().await;
        let items = after["items"].as_array().expect("items array");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["remaining_hops"].as_u64(), Some(1));
    }
}
