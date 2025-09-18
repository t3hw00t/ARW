use axum::extract::MatchedPath;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use once_cell::sync::Lazy;
use sha2::Digest as _;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone, Debug)]
struct Cfg {
    enabled: bool,
    sample_n: u64,
    ua: bool,
    ua_hash: bool,
    referer: bool,
    ref_strip_qs: bool,
    trust_forward: bool,
}

static CFG: Lazy<Cfg> = Lazy::new(|| Cfg {
    enabled: std::env::var("ARW_ACCESS_LOG").ok().as_deref() == Some("1"),
    sample_n: std::env::var("ARW_ACCESS_SAMPLE_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1),
    ua: std::env::var("ARW_ACCESS_UA").ok().as_deref() == Some("1"),
    ua_hash: std::env::var("ARW_ACCESS_UA_HASH").ok().as_deref() == Some("1"),
    referer: std::env::var("ARW_ACCESS_REF").ok().as_deref() == Some("1"),
    ref_strip_qs: std::env::var("ARW_ACCESS_REF_STRIP_QS").ok().as_deref() == Some("1"),
    trust_forward: std::env::var("ARW_TRUST_FORWARD_HEADERS").ok().as_deref() == Some("1"),
});

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn first_forwarded_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
        let ip = v.split(',').next().unwrap_or("").trim();
        if !ip.is_empty() {
            // strip port if present
            if let Some((host, _)) = ip.rsplit_once(':') {
                if host
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
                {
                    return Some(host.to_string());
                }
            }
            return Some(ip.to_string());
        }
    }
    if let Some(v) = headers.get("forwarded").and_then(|h| h.to_str().ok()) {
        for part in v.split(';').flat_map(|s| s.split(',')) {
            let part = part.trim();
            if let Some(rest) = part.strip_prefix("for=") {
                let ip = rest.trim_matches('"');
                return Some(ip.to_string());
            }
        }
    }
    None
}

pub async fn access_log_mw(req: Request<axum::body::Body>, next: Next) -> Response {
    if !CFG.enabled {
        return next.run(req).await;
    }
    let started = Instant::now();
    let method = req.method().clone();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let remote = if CFG.trust_forward {
        first_forwarded_ip(req.headers()).or_else(|| {
            req.extensions()
                .get::<axum::extract::ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip().to_string())
        })
    } else {
        req.extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
    };
    let headers = req.headers().clone();
    let res = next.run(req).await;
    let dur_ms = started.elapsed().as_millis() as u64;
    let status = res.status().as_u16();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    if CFG.sample_n > 1 && n % CFG.sample_n != 0 {
        return res;
    }
    let mut obj = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "method": method.as_str(),
        "path": path,
        "status": status,
        "dur_ms": dur_ms,
    });
    if let Some(ip) = remote {
        obj["remote"] = serde_json::Value::String(ip);
    }
    if CFG.ua || CFG.ua_hash {
        if let Some(ua) = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|h| h.to_str().ok())
        {
            if CFG.ua_hash {
                let mut hasher = sha2::Sha256::new();
                hasher.update(ua.as_bytes());
                let d = hasher.finalize();
                obj["ua_hash"] = serde_json::Value::String(hex::encode(d));
            } else if CFG.ua {
                obj["ua"] = serde_json::Value::String(ua.to_string());
            }
        }
    }
    if CFG.referer {
        if let Some(rf) = headers
            .get(axum::http::header::REFERER)
            .and_then(|h| h.to_str().ok())
        {
            let val = if CFG.ref_strip_qs {
                rf.split('?').next().unwrap_or("").to_string()
            } else {
                rf.to_string()
            };
            if !val.is_empty() {
                obj["referer"] = serde_json::Value::String(val);
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string(&obj).unwrap_or_else(|_| "{}".into())
    );
    res
}
