use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;
use tokio::task_local;
use uuid::Uuid;

const HEADER_CORR: &str = "x-arw-corr";
const HEADER_ALT_CORR: &str = "x-correlation-id";
const HEADER_REQUEST_ID: &str = "x-request-id";
const HEADER_CORR_SOURCE: &str = "x-arw-corr-source";
const MAX_ID_LEN: usize = 128;

task_local! {
    static REQ_CORR: RequestCorrelation;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrelationSource {
    Provided,
    RequestId,
    Generated,
}

impl CorrelationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CorrelationSource::Provided => "provided",
            CorrelationSource::RequestId => "request_id",
            CorrelationSource::Generated => "generated",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestCorrelation {
    request_id: String,
    corr_id: String,
    source: CorrelationSource,
}

impl RequestCorrelation {
    pub fn new<R: Into<String>, C: Into<String>>(
        request_id: R,
        corr_id: C,
        source: CorrelationSource,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            corr_id: corr_id.into(),
            source,
        }
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn corr_id(&self) -> &str {
        &self.corr_id
    }

    pub fn source(&self) -> CorrelationSource {
        self.source
    }
}

pub async fn correlation_mw(mut req: Request<Body>, next: Next) -> Response {
    let request_id_header = HeaderName::from_static(HEADER_REQUEST_ID);
    let corr_header = HeaderName::from_static(HEADER_CORR);

    let (request_id, request_id_generated) = match req
        .headers()
        .get(&request_id_header)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_id)
    {
        Some(id) => (id, false),
        None => {
            let id = Uuid::new_v4().to_string();
            (id, true)
        }
    };

    let corr_from_header = req
        .headers()
        .get(&corr_header)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_id)
        .or_else(|| {
            req.headers()
                .get(HeaderName::from_static(HEADER_ALT_CORR))
                .and_then(|value| value.to_str().ok())
                .and_then(normalize_id)
        });

    let (corr_id, source) = match corr_from_header {
        Some(id) => (id, CorrelationSource::Provided),
        None => {
            let source = if request_id_generated {
                CorrelationSource::Generated
            } else {
                CorrelationSource::RequestId
            };
            (request_id.clone(), source)
        }
    };

    if req.headers().get(&request_id_header).is_none() {
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            req.headers_mut().insert(request_id_header.clone(), value);
        }
    }
    if req.headers().get(&corr_header).is_none() {
        if let Ok(value) = HeaderValue::from_str(&corr_id) {
            req.headers_mut().insert(corr_header.clone(), value);
        }
    }

    let correlation = RequestCorrelation::new(request_id, corr_id, source);

    req.extensions_mut().insert(correlation.clone());
    let corr_source_header = HeaderName::from_static(HEADER_CORR_SOURCE);
    if req.headers().get(&corr_source_header).is_none() {
        if let Ok(value) = HeaderValue::from_str(correlation.source().as_str()) {
            req.headers_mut().insert(corr_source_header.clone(), value);
        }
    }
    REQ_CORR
        .scope(correlation.clone(), async move {
            let mut res = next.run(req).await;
            if res.headers().get(&request_id_header).is_none() {
                if let Ok(value) = HeaderValue::from_str(correlation.request_id()) {
                    res.headers_mut().insert(request_id_header.clone(), value);
                }
            }
            if res.headers().get(&corr_header).is_none() {
                if let Ok(value) = HeaderValue::from_str(correlation.corr_id()) {
                    res.headers_mut().insert(corr_header.clone(), value);
                }
            }
            if res.headers().get(&corr_source_header).is_none() {
                if let Ok(value) = HeaderValue::from_str(correlation.source().as_str()) {
                    res.headers_mut().insert(corr_source_header.clone(), value);
                }
            }
            res
        })
        .await
}

pub fn context<B>(req: &Request<B>) -> Option<RequestCorrelation> {
    req.extensions().get::<RequestCorrelation>().cloned()
}

pub fn current() -> Option<RequestCorrelation> {
    REQ_CORR.try_with(|ctx| ctx.clone()).ok()
}

fn normalize_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(trimmed.len().min(MAX_ID_LEN));
    for ch in trimmed.chars() {
        if ch.is_control() || ch == '\u{7f}' {
            continue;
        }
        if out.len() >= MAX_ID_LEN {
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    #[test]
    fn normalize_id_trims_controls_and_limits_length() {
        assert_eq!(normalize_id("  abc  "), Some("abc".into()));
        assert!(normalize_id("   ").is_none());
        let noisy = "a\u{0007}b\u{007f}c";
        assert_eq!(normalize_id(noisy), Some("abc".into()));
        let long = "x".repeat(MAX_ID_LEN + 24);
        assert_eq!(normalize_id(&long).unwrap().len(), MAX_ID_LEN);
    }

    #[tokio::test]
    async fn correlation_middleware_exposes_current_context() {
        let app = Router::new()
            .route(
                "/",
                get(|| async move {
                    let ctx = current().expect("context available");
                    assert_eq!(ctx.corr_id(), "test-corr");
                    assert_eq!(ctx.request_id(), "req-xyz");
                    assert_eq!(ctx.source(), CorrelationSource::Provided);
                    axum::response::Response::new(Body::empty())
                }),
            )
            .layer(axum::middleware::from_fn(correlation_mw));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("x-arw-corr", "test-corr")
                    .header("x-request-id", "req-xyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");

        assert_eq!(
            response
                .headers()
                .get("x-arw-corr")
                .and_then(|v| v.to_str().ok()),
            Some("test-corr"),
        );
        assert_eq!(
            response
                .headers()
                .get("x-arw-corr-source")
                .and_then(|v| v.to_str().ok()),
            Some("provided"),
        );
        let req_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        assert!(!req_id.is_empty());
    }
}
