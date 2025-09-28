use axum::extract::ConnectInfo;
use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use once_cell::sync::Lazy;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
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

fn csp_value() -> Option<String> {
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
    let val = match preset.as_str() {
        "strict" => "default-src 'none'; img-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self' https: http:; frame-ancestors 'none'",
        _ => "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; script-src 'self' 'unsafe-inline'; connect-src 'self' https: http:; frame-ancestors 'none'",
    };
    Some(val.to_string())
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
        if let Some(v) = csp_value() {
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
        let entry = map.entry(key.clone()).or_insert_with(VecDeque::new);
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
