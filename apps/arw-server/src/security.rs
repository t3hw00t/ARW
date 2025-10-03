use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use once_cell::sync::Lazy;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct ClientAddrs {
    remote: Option<String>,
    forwarded: Option<String>,
    forwarded_trusted: bool,
}

impl ClientAddrs {
    fn new(remote: Option<String>, forwarded: Option<String>) -> Self {
        let forwarded_trusted = forwarded.is_some() && trust_forward_headers();
        Self {
            remote,
            forwarded,
            forwarded_trusted,
        }
    }

    pub fn remote(&self) -> Option<&str> {
        self.remote.as_deref()
    }

    pub fn forwarded(&self) -> Option<&str> {
        self.forwarded.as_deref()
    }

    pub fn forwarded_trusted(&self) -> bool {
        self.forwarded_trusted
    }

    pub fn remote_is_loopback(&self) -> bool {
        self.remote
            .as_deref()
            .map(|ip| is_loopback_ip(ip))
            .unwrap_or(false)
    }

    pub fn forwarded_is_loopback(&self) -> bool {
        self.forwarded
            .as_deref()
            .map(|ip| is_loopback_ip(ip))
            .unwrap_or(false)
    }
}

tokio::task_local! {
    static CLIENT_ADDR: ClientAddrs;
}

fn csp_auto_enabled() -> bool {
    std::env::var("ARW_CSP_AUTO")
        .ok()
        .map(|v| v != "0" && !v.eq_ignore_ascii_case("off"))
        .unwrap_or(true)
}

fn csp_value_for(path: &str) -> Option<String> {
    if let Ok(v) = std::env::var("ARW_CSP") {
        let t = v.trim();
        if t.eq_ignore_ascii_case("off") || t == "0" {
            return None;
        }
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let preset = std::env::var("ARW_CSP_PRESET").unwrap_or_else(|_| "relaxed".into());
    // In debug UI paths we intentionally relax the CSP to avoid breaking
    // development panels that rely on inline handlers and scripts.
    let is_debug_ui = std::env::var("ARW_DEBUG").ok().is_some_and(|v| v != "0")
        && (path.starts_with("/admin/debug") || path.starts_with("/admin/ui"));
    let debug_csp_strict = std::env::var("ARW_DEBUG_CSP_STRICT")
        .ok()
        .is_some_and(|v| v != "0");
    if preset.eq_ignore_ascii_case("strict") && (!is_debug_ui || debug_csp_strict) {
        // Generate a per-response nonce for script/style sources.
        static CTR: AtomicU64 = AtomicU64::new(1);
        let c = CTR.fetch_add(1, Ordering::Relaxed);
        let now = Instant::now();
        let pid = std::process::id();
        let seed = format!("{}-{:?}-{}", c, now, pid);
        let mut h = sha2::Sha256::new();
        use sha2::Digest as _;
        h.update(seed.as_bytes());
        let digest = h.finalize();
        let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(&digest[..16]);
        // Progressive hardening for debug UI: keep script 'unsafe-inline' to avoid breaking inline event handlers,
        // but drop style 'unsafe-inline'. Non-debug pages use fully strict script/style with nonce.
        // Prefer blocking inline scripts entirely; allow inline style attributes for layout simplicity.
        let (script_src, style_src) = if is_debug_ui {
            (
                format!("script-src 'self' 'unsafe-inline' 'nonce-{}'", nonce_b64),
                "style-src 'self' 'unsafe-inline'".to_string(),
            )
        } else {
            (
                format!("script-src 'self' 'nonce-{}'", nonce_b64),
                "style-src 'self' 'unsafe-inline'".to_string(),
            )
        };
        let val = format!(
            "default-src 'self'; img-src 'self'; {} ; {} ; connect-src 'self' https: http:; frame-ancestors 'none'",
            style_src, script_src
        );
        Some(val)
    } else {
        let val = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'self' https: http:; frame-ancestors 'none'";
        Some(val.to_string())
    }
}

pub async fn client_addr_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    let addrs = collect_client_addrs(&req);
    CLIENT_ADDR
        .scope(addrs, async move { next.run(req).await })
        .await
}

pub fn client_addrs() -> ClientAddrs {
    CLIENT_ADDR
        .try_with(|info| info.clone())
        .unwrap_or_default()
}

fn collect_client_addrs<B>(req: &Request<B>) -> ClientAddrs {
    let headers = req.headers();
    let forwarded = forwarded_ip(headers);
    let remote = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_string());

    ClientAddrs::new(remote, forwarded)
}

fn forwarded_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        for part in xff.split(',') {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(valid) = parse_ip(trimmed) {
                return Some(valid);
            }
        }
    }
    if let Some(forwarded) = headers.get("forwarded").and_then(|h| h.to_str().ok()) {
        for segment in forwarded.split(';').flat_map(|s| s.split(',')) {
            let segment = segment.trim();
            if let Some(raw) = segment.strip_prefix("for=") {
                let candidate = raw.trim_matches('"');
                if let Some(valid) = parse_ip(candidate) {
                    return Some(valid);
                }
            }
        }
    }
    if let Some(real) = headers.get("x-real-ip").and_then(|h| h.to_str().ok()) {
        if let Some(valid) = parse_ip(real.trim()) {
            return Some(valid);
        }
    }
    None
}

fn parse_ip(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }

    let candidate = raw.trim_matches('[').trim_matches(']');

    if let Ok(addr) = candidate.parse::<std::net::IpAddr>() {
        return Some(addr.to_string());
    }

    if let Some((host, _port)) = candidate.rsplit_once(':') {
        if let Ok(addr) = host.parse::<std::net::IpAddr>() {
            return Some(addr.to_string());
        }
    }

    None
}

fn is_loopback_ip(addr: &str) -> bool {
    if let Ok(ip) = addr.parse::<std::net::IpAddr>() {
        return ip.is_loopback() || ip.is_unspecified();
    }
    matches!(
        addr.trim().to_ascii_lowercase().as_str(),
        "localhost" | "::1" | "[::1]" | "127.0.0.1"
    )
}

fn trust_forward_headers() -> bool {
    std::env::var("ARW_TRUST_FORWARD_HEADERS")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub async fn headers_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    // Basic security headers (idempotent)
    let h = res.headers_mut();
    let add_hdr =
        |h: &mut axum::http::HeaderMap, name: &str, val: &str| match HeaderName::from_bytes(
            name.as_bytes(),
        ) {
            Ok(name) if !h.contains_key(&name) => {
                if let Ok(v) = HeaderValue::from_str(val) {
                    h.insert(name, v);
                }
            }
            Err(err) => tracing::warn!(header = %name, %err, "invalid security header name"),
            _ => {}
        };
    add_hdr(h, "x-content-type-options", "nosniff");
    add_hdr(h, "x-frame-options", "DENY");
    let refpol = std::env::var("ARW_REFERRER_POLICY").unwrap_or_else(|_| "no-referrer".into());
    add_hdr(h, "referrer-policy", &refpol);
    add_hdr(
        h,
        "permissions-policy",
        "geolocation=(), microphone=(), camera=()",
    );
    if std::env::var("ARW_HSTS").ok().as_deref() == Some("1") {
        add_hdr(
            h,
            "strict-transport-security",
            "max-age=31536000; includeSubDomains",
        );
    }
    // CSP only for HTML unless overridden
    let is_html = h
        .get(header::CONTENT_TYPE)
        .and_then(|ct| ct.to_str().ok())
        .map(|v| v.to_ascii_lowercase().starts_with("text/html"))
        .unwrap_or(false);
    let csp_name = HeaderName::from_static("content-security-policy");
    if is_html && csp_auto_enabled() && !h.contains_key(&csp_name) {
        if let Some(v) = csp_value_for(&path) {
            if let Ok(hv) = HeaderValue::from_str(&v) {
                h.insert(csp_name, hv);
            }
        }
    }
    res
}

#[derive(Clone, Copy)]
struct RateLimitConfig {
    max: usize,
    window: Duration,
}

fn rate_limit_config() -> RateLimitConfig {
    let max = std::env::var("ARW_ADMIN_RATE_LIMIT")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(60);
    let window_secs = std::env::var("ARW_ADMIN_RATE_WINDOW_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(60);
    RateLimitConfig {
        max,
        window: Duration::from_secs(window_secs),
    }
}

static ADMIN_RATE_LIMITER: Lazy<Mutex<HashMap<String, VecDeque<Instant>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(crate) fn admin_rate_limit_allow(fingerprint: &str, addrs: &ClientAddrs) -> bool {
    let _ = fingerprint; // fingerprint remains available for future telemetry.
    let cfg = rate_limit_config();
    if cfg.max == 0 {
        return true;
    }

    let mut keys = vec!["global".to_string()];
    if let Some(remote) = addrs.remote() {
        keys.push(format!("ip:{}", remote));
    } else {
        keys.push("ip:unknown".to_string());
    }
    if addrs.forwarded_trusted() {
        if let Some(fwd) = addrs.forwarded() {
            keys.push(format!("ip:{}", fwd));
        }
    }
    keys.sort();
    keys.dedup();

    let now = Instant::now();
    let mut map = ADMIN_RATE_LIMITER
        .lock()
        .expect("admin rate limiter mutex poisoned");

    for key in &keys {
        let entry = map.entry(key.clone()).or_default();
        entry.retain(|ts| now.saturating_duration_since(*ts) <= cfg.window);
        if entry.len() >= cfg.max {
            return false;
        }
    }

    for key in keys {
        map.entry(key).or_default().push_back(now);
    }
    true
}

#[cfg(test)]
pub(crate) fn reset_admin_rate_limiter_for_tests() {
    if let Ok(mut guard) = ADMIN_RATE_LIMITER.lock() {
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_relaxed_default_for_html() {
        let mut env = crate::test_support::env::guard();
        env.remove("ARW_CSP");
        env.remove("ARW_CSP_PRESET");
        env.remove("ARW_DEBUG");
        let v = super::csp_value_for("/admin/ui/models");
        assert!(v.unwrap().contains("script-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn csp_strict_non_debug_uses_nonce() {
        let mut env = crate::test_support::env::guard();
        env.set("ARW_CSP_PRESET", "strict");
        env.remove("ARW_DEBUG");
        let v = super::csp_value_for("/about").unwrap();
        assert!(v.contains("script-src 'self' 'nonce-"));
        // Ensure we did not allow inline scripts
        assert!(!v.contains("script-src 'self' 'unsafe-inline'"));
        env.remove("ARW_CSP_PRESET");
    }

    #[test]
    fn csp_debug_relaxed_even_when_strict_preset() {
        let mut env = crate::test_support::env::guard();
        env.set("ARW_CSP_PRESET", "strict");
        env.set("ARW_DEBUG", "1");
        env.remove("ARW_DEBUG_CSP_STRICT");
        let v = super::csp_value_for("/admin/debug").unwrap();
        assert!(v.contains("script-src 'self' 'unsafe-inline'"));
        env.remove("ARW_CSP_PRESET");
        env.remove("ARW_DEBUG");
    }

    #[test]
    fn rate_limiter_blocks_by_remote_ip_even_with_unique_fingerprints() {
        reset_admin_rate_limiter_for_tests();
        let mut env = crate::test_support::env::guard();
        env.set("ARW_ADMIN_RATE_LIMIT", "2");
        env.set("ARW_ADMIN_RATE_WINDOW_SECS", "60");
        env.remove("ARW_TRUST_FORWARD_HEADERS");

        let addrs = ClientAddrs::new(Some("203.0.113.10".into()), None);
        assert!(admin_rate_limit_allow("fp-1", &addrs));
        assert!(admin_rate_limit_allow("fp-2", &addrs));
        assert!(!admin_rate_limit_allow("fp-3", &addrs));

        reset_admin_rate_limiter_for_tests();
    }

    #[test]
    fn rate_limiter_uses_trusted_forwarded_ip() {
        reset_admin_rate_limiter_for_tests();
        let mut env = crate::test_support::env::guard();
        env.set("ARW_ADMIN_RATE_LIMIT", "1");
        env.set("ARW_ADMIN_RATE_WINDOW_SECS", "60");
        env.set("ARW_TRUST_FORWARD_HEADERS", "1");

        // Actual socket appears loopback, but trusted forwarding reveals the real remote IP.
        let addrs = ClientAddrs::new(Some("127.0.0.1".into()), Some("198.51.100.3".into()));
        assert!(admin_rate_limit_allow("fp-a", &addrs));
        assert!(!admin_rate_limit_allow("fp-b", &addrs));

        env.remove("ARW_TRUST_FORWARD_HEADERS");
        reset_admin_rate_limiter_for_tests();
    }
}
