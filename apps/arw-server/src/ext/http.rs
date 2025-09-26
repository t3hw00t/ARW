use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use std::time::{Duration, SystemTime};

/// Build a quoted strong ETag header value (e.g., "\"abc...\"").
pub fn etag_value(tag: &str) -> HeaderValue {
    let mut quoted = String::with_capacity(tag.len() + 2);
    quoted.push('"');
    quoted.push_str(tag);
    quoted.push('"');
    HeaderValue::from_str(&quoted).expect("stable ETag header value")
}

/// Format a [`SystemTime`] as an RFC 7231 HTTP-date header value.
pub fn http_date_value(time: SystemTime) -> Option<HeaderValue> {
    let formatted = httpdate::fmt_http_date(time);
    HeaderValue::from_str(&formatted).ok()
}

/// Returns true when the request's `If-None-Match` header would short-circuit to 304.
pub fn if_none_match_matches(headers: &HeaderMap, etag: &str) -> bool {
    let raw = match headers.get(header::IF_NONE_MATCH) {
        Some(value) => match value.to_str() {
            Ok(s) => s,
            Err(_) => return false,
        },
        None => return false,
    };

    let trimmed = raw.trim();
    if trimmed == "*" {
        return true;
    }

    trimmed.split(',').any(|candidate| {
        let mut value = candidate.trim();
        if let Some(rest) = value.strip_prefix("W/") {
            value = rest.trim();
        }
        let normalized = value.trim_matches('"');
        normalized.eq_ignore_ascii_case(etag)
    })
}

/// Returns true when the request's `If-Modified-Since` header would produce a 304 response.
pub fn not_modified_since(headers: &HeaderMap, modified: SystemTime) -> bool {
    let since = match headers.get(header::IF_MODIFIED_SINCE) {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|s| httpdate::parse_http_date(s).ok())
        {
            Some(time) => time,
            None => return false,
        },
        None => return false,
    };

    let modified_epoch = match modified.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur,
        Err(_) => return false,
    };
    let since_epoch = match since.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => dur,
        Err(_) => return false,
    };

    // HTTP dates have 1-second precision. Treat matching seconds (or slightly earlier) as not modified.
    modified_epoch <= since_epoch + Duration::from_secs(1)
}

/// Build a standard `304 Not Modified` response with shared caching headers.
pub fn not_modified_response(
    etag: &HeaderValue,
    last_modified: Option<&HeaderValue>,
    cache_control: &str,
) -> Response {
    let mut builder = Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, etag.clone())
        .header(header::CACHE_CONTROL, cache_control)
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    if let Some(value) = last_modified {
        builder = builder.header(header::LAST_MODIFIED, value.clone());
    }
    builder
        .body(Body::empty())
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
