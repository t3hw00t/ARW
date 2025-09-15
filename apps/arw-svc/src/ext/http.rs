//! Small HTTP helpers to keep handlers lean and consistent.
use axum::http::HeaderMap;

/// Build standard headers for immutable CAS blobs (ETag, caching, type, ranges, last-modified).
pub fn build_blob_headers(meta: &std::fs::Metadata, etag_token: &str) -> HeaderMap {
    use axum::http::header;
    use axum::http::HeaderValue;
    let mut headers = HeaderMap::new();
    let _ = headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    let _ = headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Ok(h) = HeaderValue::from_str(&format!("\"{}\"", etag_token)) {
        let _ = headers.insert(header::ETAG, h);
    }
    let _ = headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    if let Ok(modified) = meta.modified() {
        let dt = chrono::DateTime::<chrono::Utc>::from(modified).to_rfc2822();
        if let Ok(h) = HeaderValue::from_str(&dt) {
            let _ = headers.insert(header::LAST_MODIFIED, h);
        }
    }
    headers
}

/// Check conditional request headers (If-None-Match / If-Modified-Since). Returns true if Not Modified.
pub fn is_not_modified(
    headers_in: &HeaderMap,
    headers_out: &HeaderMap,
    meta: &std::fs::Metadata,
    etag_token: &str,
) -> bool {
    use axum::http::header;
    if let Some(inm) = headers_in.get(header::IF_NONE_MATCH) {
        let etag_val = format!("\"{}\"", etag_token);
        if inm
            .to_str()
            .ok()
            .map(|s| s.contains(&etag_val))
            .unwrap_or(false)
        {
            return true;
        }
    }
    if let Some(ims) = headers_in.get(header::IF_MODIFIED_SINCE) {
        if let Ok(ims_s) = ims.to_str() {
            if let Some(lm) = headers_out.get(header::LAST_MODIFIED) {
                if let Ok(lm_s) = lm.to_str() {
                    if let (Ok(ims_dt), Ok(lm_dt)) = (
                        chrono::DateTime::parse_from_rfc2822(ims_s),
                        chrono::DateTime::parse_from_rfc2822(lm_s),
                    ) {
                        if ims_dt >= lm_dt {
                            return true;
                        }
                    }
                }
            }
            if let Ok(modified) = meta.modified() {
                if let Ok(ts) = chrono::DateTime::parse_from_rfc2822(ims_s) {
                    if ts.with_timezone(&chrono::Utc)
                        >= chrono::DateTime::<chrono::Utc>::from(modified)
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Parse a single HTTP bytes range spec against a total length.
/// Supports: bytes=start-end, bytes=start-, bytes=-suffix
pub fn parse_range_spec(hdr: &str, total_len: u64) -> Option<(u64, u64)> {
    let s = hdr.trim();
    if total_len == 0 || !s.starts_with("bytes=") {
        return None;
    }
    let spec = &s[6..];
    if let Some(hy) = spec.find('-') {
        let (a, b) = spec.split_at(hy);
        let b = &b[1..];
        if a.is_empty() {
            if let Ok(n) = b.parse::<u64>() {
                let n = n.min(total_len);
                let start = total_len.saturating_sub(n);
                let end = total_len.saturating_sub(1);
                return Some((start, end));
            }
        } else if b.is_empty() {
            if let Ok(start) = a.parse::<u64>() {
                if start < total_len {
                    return Some((start, total_len.saturating_sub(1)));
                }
            }
        } else if let (Ok(start), Ok(end)) = (a.parse::<u64>(), b.parse::<u64>()) {
            if start <= end && end < total_len {
                return Some((start, end));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_range() {
        assert_eq!(parse_range_spec("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range_spec("bytes=10-", 20), Some((10, 19)));
        assert_eq!(parse_range_spec("bytes=-5", 11), Some((6, 10)));
        assert_eq!(parse_range_spec("bytes=200-", 100), None);
        assert_eq!(parse_range_spec("", 100), None);
    }
}
