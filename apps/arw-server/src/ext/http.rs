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
        if value == "*" {
            return true;
        }
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

/// Inclusive byte range describing the slice `[start, end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    /// Length of the range in bytes.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Error emitted when parsing a `Range` header fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRangeError {
    Invalid,
    Unsatisfiable,
}

/// Parse a single `bytes=` range header into an inclusive [`ByteRange`].
pub fn parse_single_byte_range(value: &str, len: u64) -> Result<ByteRange, ByteRangeError> {
    if len == 0 {
        return Err(ByteRangeError::Unsatisfiable);
    }

    let trimmed = value.trim();
    if !trimmed.starts_with("bytes=") {
        return Err(ByteRangeError::Invalid);
    }

    let spec = trimmed[6..].trim();
    if spec.is_empty() || spec.contains(',') {
        return Err(ByteRangeError::Invalid);
    }

    let (start_token, end_token) = match spec.split_once('-') {
        Some(pair) => pair,
        None => return Err(ByteRangeError::Invalid),
    };

    let start_token = start_token.trim();
    let end_token = end_token.trim();

    if start_token.is_empty() {
        let suffix: u64 = end_token.parse().map_err(|_| ByteRangeError::Invalid)?;
        if suffix == 0 {
            return Err(ByteRangeError::Invalid);
        }
        let length = suffix.min(len);
        let start = len - length;
        let end = len - 1;
        Ok(ByteRange { start, end })
    } else {
        let start: u64 = start_token.parse().map_err(|_| ByteRangeError::Invalid)?;
        if start >= len {
            return Err(ByteRangeError::Unsatisfiable);
        }

        if end_token.is_empty() {
            Ok(ByteRange {
                start,
                end: len - 1,
            })
        } else {
            let end: u64 = end_token.parse().map_err(|_| ByteRangeError::Invalid)?;
            if start > end {
                return Err(ByteRangeError::Invalid);
            }
            let end = end.min(len - 1);
            Ok(ByteRange { start, end })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn etag_value_quotes_and_preserves_case() {
        let header = etag_value("abc123DEF");
        assert_eq!(header.to_str().unwrap(), "\"abc123DEF\"");
    }

    #[test]
    fn if_none_match_matches_star_and_multiple_values() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_static("W/\"foo\", \"bar\", *"),
        );

        assert!(if_none_match_matches(&headers, "bar"));
        assert!(if_none_match_matches(&headers, "foo"));
        assert!(if_none_match_matches(&headers, "baz"));
    }

    #[test]
    fn if_none_match_rejects_non_matching_values() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"foo\""));

        assert!(!if_none_match_matches(&headers, "bar"));
    }

    #[test]
    fn if_none_match_ignores_non_utf8_header() {
        // SAFETY: value contains invalid UTF-8; constructing via from_bytes succeeds.
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_bytes(b"\xff").unwrap(),
        );

        assert!(!if_none_match_matches(&headers, "anything"));
    }

    #[test]
    fn not_modified_since_obeys_one_second_precision() {
        let reference = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let mut headers = HeaderMap::new();
        let header_value = http_date_value(reference).expect("http-date");
        headers.insert(header::IF_MODIFIED_SINCE, header_value);

        assert!(not_modified_since(&headers, reference));
        assert!(not_modified_since(
            &headers,
            reference - Duration::from_millis(500)
        ));
        assert!(!not_modified_since(
            &headers,
            reference + Duration::from_secs(2)
        ));
    }

    #[test]
    fn not_modified_response_sets_expected_headers() {
        let etag = etag_value("hash");
        let last_modified =
            http_date_value(SystemTime::UNIX_EPOCH + Duration::from_secs(42)).unwrap();
        let response = not_modified_response(&etag, Some(&last_modified), "public, max-age=60");
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        let headers = response.headers();
        assert_eq!(headers.get(header::ETAG), Some(&etag));
        assert_eq!(headers.get(header::LAST_MODIFIED), Some(&last_modified));
        assert_eq!(
            headers
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=60")
        );
        assert_eq!(
            headers
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .and_then(|v| v.to_str().ok()),
            Some("nosniff")
        );
    }

    #[test]
    fn parse_single_range_start_end() {
        let range = parse_single_byte_range("bytes=10-49", 100).expect("range");
        assert_eq!(range, ByteRange { start: 10, end: 49 });
        assert_eq!(range.len(), 40);
    }

    #[test]
    fn parse_single_range_open_ended() {
        let range = parse_single_byte_range("bytes=10-", 16).expect("range");
        assert_eq!(range, ByteRange { start: 10, end: 15 });
    }

    #[test]
    fn parse_single_range_suffix() {
        let range = parse_single_byte_range("bytes=-4", 10).expect("range");
        assert_eq!(range, ByteRange { start: 6, end: 9 });
    }

    #[test]
    fn parse_single_range_suffix_larger_than_len() {
        let range = parse_single_byte_range("bytes=-999", 10).expect("range");
        assert_eq!(range, ByteRange { start: 0, end: 9 });
    }

    #[test]
    fn parse_single_range_unsatisfiable_start_outside_len() {
        let err = parse_single_byte_range("bytes=50-60", 10).unwrap_err();
        assert_eq!(err, ByteRangeError::Unsatisfiable);
    }

    #[test]
    fn parse_single_range_invalid_when_start_after_end() {
        let err = parse_single_byte_range("bytes=20-10", 100).unwrap_err();
        assert_eq!(err, ByteRangeError::Invalid);
    }

    #[test]
    fn parse_single_range_invalid_format() {
        let err = parse_single_byte_range("items=0-10", 100).unwrap_err();
        assert_eq!(err, ByteRangeError::Invalid);
    }
}
