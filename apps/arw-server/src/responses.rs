use arw_protocol::{ApiEnvelope, ProblemDetails};
use axum::body::{self, Body};
use axum::http::{self, header::HeaderValue, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;
use std::borrow::ToOwned;
use std::convert::Infallible;
use uuid::Uuid;

pub const HEADER_ENVELOPE_APPLIED: &str = "x-arw-envelope-applied";
pub const HEADER_ENVELOPE_BYPASS: &str = "x-arw-envelope-bypass";
pub const HEADER_ENVELOPE_REQUEST: &str = "x-arw-envelope";
const QUERY_ENVELOPE_PARAM: &str = "arw-envelope";

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum EnvelopePref {
    Auto,
    Force,
    Skip,
}

pub fn problem_details(status: StatusCode, title: &str, detail: Option<&str>) -> ProblemDetails {
    ProblemDetails {
        r#type: "about:blank".to_string(),
        title: title.to_string(),
        status: status.as_u16(),
        detail: detail.map(|v| v.to_string()),
        instance: None,
        trace_id: None,
        code: None,
    }
}

pub fn problem_response(
    status: StatusCode,
    title: &str,
    detail: Option<&str>,
) -> axum::response::Response {
    (status, Json(problem_details(status, title, detail))).into_response()
}

pub fn kernel_disabled() -> axum::response::Response {
    problem_response(
        StatusCode::NOT_IMPLEMENTED,
        "Kernel Disabled",
        Some("Operation requires ARW_KERNEL_ENABLE=1"),
    )
}

pub fn unauthorized(detail: Option<&str>) -> axum::response::Response {
    problem_response(StatusCode::UNAUTHORIZED, "Unauthorized", detail)
}

pub fn attach_corr(payload: &mut Value) {
    if let Value::Object(map) = payload {
        if !map.contains_key("corr_id") {
            map.insert("corr_id".into(), Value::String(Uuid::new_v4().to_string()));
        }
    }
}

pub async fn require_admin(headers: &HeaderMap) -> Result<(), Box<axum::response::Response>> {
    if crate::admin_ok(headers).await {
        Ok(())
    } else {
        Err(Box::new(unauthorized(None)))
    }
}

/// Unified success response builder that optionally envelopes payloads.
pub struct JsonResponse<T> {
    status: StatusCode,
    payload: T,
    corr_id: Option<String>,
    code: Option<String>,
    message: Option<String>,
}

impl<T> JsonResponse<T> {
    pub fn new(payload: T) -> Self {
        Self {
            status: StatusCode::OK,
            payload,
            corr_id: None,
            code: None,
            message: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    #[allow(dead_code)]
    pub fn with_corr_id<S: Into<String>>(mut self, corr_id: S) -> Self {
        self.corr_id = Some(corr_id.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_code<S: Into<String>>(mut self, code: S) -> Self {
        self.code = Some(code.into());
        self
    }

    #[allow(dead_code)]
    pub fn with_message<S: Into<String>>(mut self, message: S) -> Self {
        self.message = Some(message.into());
        self
    }

    #[allow(dead_code)]
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl<T> From<T> for JsonResponse<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

fn parse_pref_token(token: &str) -> Option<EnvelopePref> {
    let trimmed = token.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "0" | "false" | "off" | "no" | "none" | "raw" | "skip" => Some(EnvelopePref::Skip),
        "1" | "true" | "on" | "yes" | "wrap" | "force" => Some(EnvelopePref::Force),
        "auto" | "default" => Some(EnvelopePref::Auto),
        _ => None,
    }
}

fn pref_from_query(uri: &http::Uri) -> Option<EnvelopePref> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        if !key.eq_ignore_ascii_case(QUERY_ENVELOPE_PARAM) {
            continue;
        }
        let raw_value = parts.next().unwrap_or("");
        let value = raw_value.replace('+', " ");
        if let Some(pref) = parse_pref_token(&value) {
            return Some(pref);
        }
    }
    None
}

fn pref_from_headers(headers: &HeaderMap) -> Option<EnvelopePref> {
    headers
        .get(HEADER_ENVELOPE_REQUEST)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_pref_token)
}

impl<T: Serialize> IntoResponse for JsonResponse<T> {
    fn into_response(self) -> axum::response::Response {
        let envelope_enabled = crate::config::api_envelope_enabled();

        let mut response = if envelope_enabled {
            let env = ApiEnvelope {
                ok: self.status.is_success(),
                data: self.payload,
                corr_id: self.corr_id,
                code: self.code,
                message: self.message,
            };

            if self.status == StatusCode::OK {
                Json(env).into_response()
            } else {
                (self.status, Json(env)).into_response()
            }
        } else if self.status == StatusCode::OK {
            Json(self.payload).into_response()
        } else {
            (self.status, Json(self.payload)).into_response()
        };

        if envelope_enabled {
            response
                .headers_mut()
                .insert(HEADER_ENVELOPE_APPLIED, HeaderValue::from_static("1"));
        }

        response
    }
}

pub fn json_ok<T: Serialize>(value: T) -> JsonResponse<T> {
    JsonResponse::new(value)
}

#[allow(dead_code)]
pub fn json_with_status<T: Serialize>(status: StatusCode, value: T) -> JsonResponse<T> {
    JsonResponse::new(value).with_status(status)
}

pub fn mark_envelope_bypass(resp: &mut Response) {
    resp.headers_mut()
        .insert(HEADER_ENVELOPE_BYPASS, HeaderValue::from_static("1"));
}

#[allow(dead_code)]
pub fn json_raw<T: Serialize>(value: T) -> Response {
    let mut resp = Json(value).into_response();
    mark_envelope_bypass(&mut resp);
    resp
}

pub fn json_raw_status<T: Serialize>(status: StatusCode, value: T) -> Response {
    let mut resp = (status, Json(value)).into_response();
    mark_envelope_bypass(&mut resp);
    resp
}

/// Wrap successful JSON responses when `ARW_API_ENVELOPE` is enabled.
pub async fn envelope_mw(
    req: Request<Body>,
    next: Next,
) -> Result<axum::response::Response, Infallible> {
    let header_pref = pref_from_headers(req.headers());
    let query_pref = pref_from_query(req.uri());
    let request_pref = header_pref.or(query_pref).unwrap_or(EnvelopePref::Auto);
    let mut response = next.run(req).await;

    if response
        .headers_mut()
        .remove(HEADER_ENVELOPE_BYPASS)
        .is_some()
    {
        return Ok(response);
    }

    let already_applied = response
        .headers_mut()
        .remove(HEADER_ENVELOPE_APPLIED)
        .is_some();

    let baseline = crate::config::api_envelope_enabled();
    let target_wrap = match request_pref {
        EnvelopePref::Force => true,
        EnvelopePref::Skip => false,
        EnvelopePref::Auto => baseline,
    };

    let status = response.status();
    let is_success_json = status.is_success()
        && status != StatusCode::NO_CONTENT
        && response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.starts_with("application/json"))
            .unwrap_or(false);

    if !target_wrap {
        if already_applied && is_success_json {
            let (mut parts, body) = response.into_parts();
            match body::to_bytes(body, usize::MAX).await {
                Ok(bytes) => {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        if let Some(data) = value.get("data") {
                            parts.headers.remove(HEADER_ENVELOPE_APPLIED);
                            if let Ok(body_bytes) = serde_json::to_vec(data) {
                                parts.headers.insert(
                                    axum::http::header::CONTENT_TYPE,
                                    HeaderValue::from_static("application/json"),
                                );
                                parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                                if let Ok(len) =
                                    HeaderValue::from_str(&body_bytes.len().to_string())
                                {
                                    parts
                                        .headers
                                        .insert(axum::http::header::CONTENT_LENGTH, len);
                                }

                                let mut raw_response =
                                    Response::from_parts(parts, Body::from(body_bytes));
                                mark_envelope_bypass(&mut raw_response);
                                return Ok(raw_response);
                            }
                        }
                    }
                    parts.headers.remove(HEADER_ENVELOPE_APPLIED);
                    let mut fallback = Response::from_parts(parts, Body::from(bytes));
                    mark_envelope_bypass(&mut fallback);
                    return Ok(fallback);
                }
                Err(_) => {
                    let mut fallback = Response::new(Body::empty());
                    mark_envelope_bypass(&mut fallback);
                    return Ok(fallback);
                }
            }
        }

        if request_pref == EnvelopePref::Skip {
            mark_envelope_bypass(&mut response);
        }
        return Ok(response);
    }

    if !is_success_json {
        return Ok(response);
    }

    if already_applied {
        response
            .headers_mut()
            .insert(HEADER_ENVELOPE_APPLIED, HeaderValue::from_static("1"));
        return Ok(response);
    }

    let (mut parts, body) = response.into_parts();
    match body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => {
            if bytes.is_empty() {
                let mut resp = Response::from_parts(parts, Body::from(bytes));
                resp.headers_mut()
                    .insert(HEADER_ENVELOPE_APPLIED, HeaderValue::from_static("1"));
                return Ok(resp);
            }

            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(data) => {
                    parts.headers.remove(HEADER_ENVELOPE_APPLIED);
                    let corr_id = data
                        .get("corr_id")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    let code = data
                        .get("code")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    let message = data
                        .get("message")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    let envelope = ApiEnvelope {
                        ok: true,
                        data,
                        corr_id,
                        code,
                        message,
                    };

                    match serde_json::to_vec(&envelope) {
                        Ok(body_bytes) => {
                            parts.headers.insert(
                                axum::http::header::CONTENT_TYPE,
                                HeaderValue::from_static("application/json"),
                            );
                            parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                            if let Ok(len) = HeaderValue::from_str(&body_bytes.len().to_string()) {
                                parts
                                    .headers
                                    .insert(axum::http::header::CONTENT_LENGTH, len);
                            }

                            parts
                                .headers
                                .insert(HEADER_ENVELOPE_APPLIED, HeaderValue::from_static("1"));

                            Ok(Response::from_parts(parts, Body::from(body_bytes)))
                        }
                        Err(_) => Ok(Response::from_parts(parts, Body::from(bytes))),
                    }
                }
                Err(_) => Ok(Response::from_parts(parts, Body::from(bytes))),
            }
        }
        Err(_) => Ok(Response::from_parts(parts, Body::empty())),
    }
}
