use axum::extract::ConnectInfo;
use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use once_cell::sync::Lazy;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

tokio::task_local! {
    static CLIENT_ADDR: Option<String>;
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
    let is_debug_ui = std::env::var("ARW_DEBUG").ok().map_or(false, |v| v != "0")
        && (path.starts_with("/admin/debug") || path.starts_with("/admin/ui"));
    let debug_csp_strict = std::env::var("ARW_DEBUG_CSP_STRICT")
        .ok()
        .map_or(false, |v| v != "0");
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
    let ip = extract_client_addr(&req);
    CLIENT_ADDR
        .scope(ip, async move { next.run(req).await })
        .await
}

pub fn client_addr() -> Option<String> {
    CLIENT_ADDR.try_with(|opt| opt.clone()).unwrap_or(None)
}

fn extract_client_addr<B>(req: &Request<B>) -> Option<String> {
    let forwarded = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(',').find_map(|part| {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
        });
    if forwarded.is_some() {
        return forwarded;
    }

    if let Some(real) = req
        .headers()
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(real);
    }

    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.to_string())
}

pub async fn headers_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut res = next.run(req).await;
    // Basic security headers (idempotent)
    let h = res.headers_mut();
    let add_hdr = |h: &mut axum::http::HeaderMap, name: &str, val: &str| {
        let name = HeaderName::from_bytes(name.as_bytes()).unwrap();
        if !h.contains_key(&name) {
            if let Ok(v) = HeaderValue::from_str(val) {
                h.insert(name, v);
            }
        }
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

pub(crate) fn admin_rate_limit_allow(fingerprint: &str, ip: Option<&str>) -> bool {
    let cfg = rate_limit_config();
    if cfg.max == 0 {
        return true;
    }
    let key = format!("{}@{}", fingerprint, ip.unwrap_or("unknown"));
    let now = Instant::now();
    let mut map = ADMIN_RATE_LIMITER
        .lock()
        .expect("admin rate limiter mutex poisoned");
    let mut remove_key = false;
    let allowed = {
        let entry = map.entry(key.clone()).or_default();
        entry.retain(|ts| now.saturating_duration_since(*ts) <= cfg.window);
        if entry.is_empty() {
            remove_key = true;
        }
        if entry.len() >= cfg.max {
            false
        } else {
            entry.push_back(now);
            remove_key = false;
            true
        }
    };
    if remove_key {
        map.remove(&key);
    }
    allowed
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
        std::env::remove_var("ARW_CSP");
        std::env::remove_var("ARW_CSP_PRESET");
        std::env::remove_var("ARW_DEBUG");
        let v = super::csp_value_for("/admin/ui/models");
        assert!(v.unwrap().contains("script-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn csp_strict_non_debug_uses_nonce() {
        std::env::set_var("ARW_CSP_PRESET", "strict");
        std::env::remove_var("ARW_DEBUG");
        let v = super::csp_value_for("/about").unwrap();
        assert!(v.contains("script-src 'self' 'nonce-"));
        // Ensure we did not allow inline scripts
        assert!(!v.contains("script-src 'self' 'unsafe-inline'"));
        std::env::remove_var("ARW_CSP_PRESET");
    }

    #[test]
    fn csp_debug_relaxed_even_when_strict_preset() {
        std::env::set_var("ARW_CSP_PRESET", "strict");
        std::env::set_var("ARW_DEBUG", "1");
        std::env::remove_var("ARW_DEBUG_CSP_STRICT");
        let v = super::csp_value_for("/admin/debug").unwrap();
        assert!(v.contains("script-src 'self' 'unsafe-inline'"));
        std::env::remove_var("ARW_CSP_PRESET");
        std::env::remove_var("ARW_DEBUG");
    }
}
