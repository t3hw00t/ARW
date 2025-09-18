use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;

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
