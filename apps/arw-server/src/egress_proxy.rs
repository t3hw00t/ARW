use crate::egress_policy::{capability_candidates, lease_grant, reason_code, DenyReason};
use crate::{egress_policy, http_timeout, util::effective_posture, AppState};
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt as _, Empty, Full};
use hyper::body::Incoming as IncomingBody;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use arw_topics as topics;
type ProxyBody = BoxBody<Bytes, Infallible>;

#[allow(dead_code)]
fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new().boxed()
}
#[allow(dead_code)]
fn bytes_body(b: Bytes) -> ProxyBody {
    Full::new(b).boxed()
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        ),
        Err(_) => default,
    }
}

fn dns_guard() -> bool {
    env_flag("ARW_DNS_GUARD_ENABLE", true)
}
fn is_doh_host(h: &str) -> bool {
    let h = h.to_ascii_lowercase();
    let suffixes = [
        "dns.google",
        "cloudflare-dns.com",
        "one.one.one.one",
        "security.cloudflare-dns.com",
        "dns.quad9.net",
        "quad9.net",
        "doh.opendns.com",
        "familyshield.opendns.com",
        "dns.nextdns.io",
        "nextdns.io",
        "adguard-dns.com",
        "doh.cleanbrowsing.org",
    ];
    suffixes
        .iter()
        .any(|s| h == *s || h.ends_with(&format!(".{s}")))
}

struct ProxyRuntime {
    port: u16,
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
}

static PROXY: Lazy<Mutex<Option<ProxyRuntime>>> = Lazy::new(|| Mutex::new(None));

pub async fn apply_current(state: AppState) {
    let enable = env_flag("ARW_EGRESS_PROXY_ENABLE", true);
    let port: u16 = std::env::var("ARW_EGRESS_PROXY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9080);
    apply(enable, port, state).await;
}

pub async fn apply(enable: bool, port: u16, state: AppState) {
    let mut guard = PROXY.lock().unwrap();
    match (&*guard, enable) {
        (Some(rt), true) if rt.port == port => {
            // already running with same port
            return;
        }
        (Some(rt), false) | (Some(rt), true) if rt.port != port => {
            rt.cancel.cancel();
            rt.handle.abort();
            *guard = None;
            if !enable {
                return;
            }
        }
        (None, false) => return,
        _ => {}
    }
    let bind = format!("127.0.0.1:{}", port);
    info!("egress proxy listening on {} (preview)", bind);
    let client = reqwest::Client::builder()
        .timeout(http_timeout::get_duration())
        .build()
        .expect("reqwest client");
    let cancel = CancellationToken::new();
    let cancel_child = cancel.clone();
    let st_outer = state.clone();
    let handle = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("egress proxy bind error: {}", e);
                return;
            }
        };
        let exec = TokioExecutor::new();
        loop {
            tokio::select! {
                _ = cancel_child.cancelled() => { info!("egress proxy stopping"); break; }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let st = st_outer.clone();
                            let cl = client.clone();
                            let exec2 = exec.clone();
                            tokio::spawn(async move {
                                let svc = service_fn(move |req| proxy_handler(st.clone(), cl.clone(), req));
                                let builder = AutoBuilder::new(exec2);
                                if let Err(e) = builder.serve_connection_with_upgrades(io, svc).await {
                                    warn!("proxy conn error: {}", e);
                                }
                            });
                        }
                        Err(e) => { warn!("proxy accept error: {}", e); }
                    }
                }
            }
        }
    });
    *guard = Some(ProxyRuntime {
        port,
        cancel,
        handle,
    });
}

async fn proxy_handler(
    state: AppState,
    client: reqwest::Client,
    req: Request<IncomingBody>,
) -> Result<Response<ProxyBody>, Infallible> {
    if req.method() == Method::CONNECT {
        return Ok(handle_connect(state, req).await);
    }
    Ok(handle_http_forward(state, client, req).await)
}

async fn handle_connect(state: AppState, req: Request<IncomingBody>) -> Response<ProxyBody> {
    // CONNECT authority is host:port
    let corr_id_hdr = req
        .headers()
        .get("x-arw-corr")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let proj_hdr = req
        .headers()
        .get("x-arw-project")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let authority = match req.uri().authority() {
        Some(a) => a.to_string(),
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from_static(b"Bad CONNECT")).boxed())
                .unwrap();
        }
    };
    let mut parts = authority.split(':');
    let host = parts.next().unwrap_or("").to_string();
    let port: u16 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(443);

    let policy = egress_policy::resolve_policy(&state).await;
    let posture_decision = egress_policy::evaluate(&policy, Some(&host), Some(port), "https");
    let caps = capability_candidates(Some(&host), Some(port), "https");
    let mut lease = lease_grant(&state, &caps).await;

    let mut base_meta = serde_json::Map::new();
    base_meta.insert(
        "proxy".into(),
        json!({"mode": "forward", "kind": "connect"}),
    );
    base_meta.insert("capabilities".into(), json!(caps));
    base_meta.insert("policy_posture".into(), json!(policy.posture.as_str()));
    base_meta.insert("policy_allow".into(), json!(posture_decision.allow));
    base_meta.insert("policy_dns_guard".into(), json!(policy.dns_guard_enabled));
    base_meta.insert("policy_proxy_enabled".into(), json!(policy.proxy_enabled));
    if let Some(reason) = posture_decision.reason {
        base_meta.insert("policy_reason".into(), json!(reason_code(reason)));
    }
    if let Some(ref lease_val) = lease {
        base_meta.insert("lease".into(), lease_val.clone());
        base_meta.insert("allowed_via".into(), json!("lease"));
    }

    if !posture_decision.allow && lease.is_none() {
        let reason = posture_decision
            .reason
            .unwrap_or(DenyReason::HostNotAllowed);
        let mut meta = base_meta.clone();
        meta.insert("deny_stage".into(), json!("posture"));
        meta.insert("deny_reason".into(), json!(reason_code(reason)));
        let meta_val = Value::Object(meta);
        log_egress_event(
            &state,
            "deny",
            Some(reason_code(reason)),
            Some(&host),
            Some(port),
            Some("tcp"),
            None,
            None,
            corr_id_hdr.as_deref(),
            proj_hdr.as_deref(),
            Some(meta_val),
        )
        .await;
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Full::new(Bytes::from_static(b"Egress blocked")).boxed())
            .unwrap();
    }

    if dns_guard() && (port == 853 || is_doh_host(&host)) {
        if lease.is_none() {
            let mut meta = base_meta.clone();
            meta.insert("dns_guard".into(), json!(true));
            meta.insert("deny_stage".into(), json!("dns_guard"));
            meta.insert("deny_reason".into(), json!("dns_guard"));
            let meta_val = Value::Object(meta);
            log_egress_event(
                &state,
                "deny",
                Some("dns_guard"),
                Some(&host),
                Some(port),
                Some("tcp"),
                None,
                None,
                corr_id_hdr.as_deref(),
                proj_hdr.as_deref(),
                Some(meta_val),
            )
            .await;
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Full::new(Bytes::from_static(b"DNS guard")).boxed())
                .unwrap();
        } else {
            base_meta.insert("dns_guard".into(), json!(true));
            base_meta.insert("allowed_via".into(), json!("lease"));
        }
    }

    let policy_decision = state
        .policy()
        .lock()
        .await
        .evaluate_action("net.tcp.connect");
    if !policy_decision.allow {
        if let Some(cap) = policy_decision.require_capability.as_deref() {
            let lease_vec = vec![cap.to_string()];
            if let Some(lease_val) = lease_grant(&state, &lease_vec).await {
                lease = Some(lease_val.clone());
                base_meta.insert("lease".into(), lease_val);
                base_meta.insert("allowed_via".into(), json!("lease"));
                base_meta.insert("policy_required_capability".into(), json!(cap));
            } else {
                let mut meta = base_meta.clone();
                meta.insert("deny_stage".into(), json!("policy"));
                meta.insert("deny_reason".into(), json!("lease_required"));
                meta.insert("policy_required_capability".into(), json!(cap));
                let meta_val = Value::Object(meta);
                log_egress_event(
                    &state,
                    "deny",
                    Some("lease_required"),
                    Some(&host),
                    Some(port),
                    Some("tcp"),
                    None,
                    None,
                    corr_id_hdr.as_deref(),
                    proj_hdr.as_deref(),
                    Some(meta_val),
                )
                .await;
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"Lease required")).boxed())
                    .unwrap();
            }
        } else {
            let mut meta = base_meta.clone();
            meta.insert("deny_stage".into(), json!("policy"));
            meta.insert("deny_reason".into(), json!("policy"));
            let meta_val = Value::Object(meta);
            log_egress_event(
                &state,
                "deny",
                Some("policy"),
                Some(&host),
                Some(port),
                Some("tcp"),
                None,
                None,
                corr_id_hdr.as_deref(),
                proj_hdr.as_deref(),
                Some(meta_val),
            )
            .await;
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Full::new(Bytes::from_static(b"Policy denied")).boxed())
                .unwrap();
        }
    }

    if !base_meta.contains_key("allowed_via") {
        base_meta.insert("allowed_via".into(), json!("policy"));
    }
    if let Some(ref lease_val) = lease {
        base_meta.insert("lease".into(), lease_val.clone());
    }

    let base_meta_value = Value::Object(base_meta.clone());
    let meta_arc = Arc::new(base_meta_value);

    // Establish TCP to target
    let target = format!("{}:{}", host, port);
    let mut server_stream = match tokio::net::TcpStream::connect(target).await {
        Ok(s) => s,
        Err(e) => {
            warn!("connect failed: {}", e);
            let mut meta = base_meta.clone();
            meta.insert("error".into(), json!("connect"));
            let meta_val = Value::Object(meta);
            log_egress_event(
                &state,
                "error",
                Some("connect"),
                Some(&host),
                Some(port),
                Some("tcp"),
                None,
                None,
                corr_id_hdr.as_deref(),
                proj_hdr.as_deref(),
                Some(meta_val),
            )
            .await;
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from_static(b"Connect failed")).boxed())
                .unwrap();
        }
    };
    // Prepare response and upgrade
    let mut resp = Response::new(Empty::<Bytes>::new().boxed());
    *resp.status_mut() = StatusCode::OK;
    let st = state.clone();
    let host_spawn = host.clone();
    let corr_spawn = corr_id_hdr.clone();
    let proj_spawn = proj_hdr.clone();
    let meta_spawn = meta_arc.clone();
    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                // Wrap upgraded stream for tokio IO traits
                let upgraded = TokioIo::new(upgraded);
                // Bidirectional copy with accounting
                let (mut cr, mut cw) = tokio::io::split(upgraded);
                let (mut sr, mut sw) = server_stream.split();
                let mut c2s_bytes = 0u64; // client->server
                let mut s2c_bytes = 0u64; // server->client

                let c2s = async {
                    let mut buf = [0u8; 16 * 1024];
                    loop {
                        let n = cr.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        c2s_bytes += n as u64;
                        if sw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    let _ = sw.shutdown().await;
                };
                let s2c = async {
                    let mut buf = [0u8; 16 * 1024];
                    loop {
                        let n = sr.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        s2c_bytes += n as u64;
                        if cw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    let _ = cw.shutdown().await;
                };
                let _ = tokio::join!(c2s, s2c);
                log_egress_event(
                    &st,
                    "allow",
                    Some("connect"),
                    Some(host_spawn.as_str()),
                    Some(port),
                    Some("tcp"),
                    Some(s2c_bytes as i64),
                    Some(c2s_bytes as i64),
                    corr_spawn.as_deref(),
                    proj_spawn.as_deref(),
                    Some((*meta_spawn).clone()),
                )
                .await;
            }
            Err(e) => {
                warn!("upgrade failed: {}", e);
            }
        }
    });
    resp
}

async fn handle_http_forward(
    state: AppState,
    client: reqwest::Client,
    req: Request<IncomingBody>,
) -> Response<ProxyBody> {
    // Expect absolute-form URI
    let corr_id_hdr = req
        .headers()
        .get("x-arw-corr")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let proj_hdr = req
        .headers()
        .get("x-arw-project")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());
    let uri = req.uri().to_string();
    let url = match reqwest::Url::parse(&uri) {
        Ok(u) => u,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from_static(b"Expected absolute-form URI")).boxed())
                .unwrap();
        }
    };
    let host = url.host_str().map(|s| s.to_string());
    let port: u16 = url.port_or_known_default().unwrap_or(80);
    let scheme = url.scheme().to_string();

    let policy = egress_policy::resolve_policy(&state).await;
    let posture_decision = egress_policy::evaluate(&policy, host.as_deref(), Some(port), &scheme);
    if !posture_decision.allow {
        let reason = posture_decision
            .reason
            .unwrap_or(DenyReason::HostNotAllowed);
        let caps = capability_candidates(host.as_deref(), Some(port), &scheme);
        if lease_grant(&state, &caps).await.is_none() {
            let code = reason_code(reason);
            log_egress_event(
                &state,
                "deny",
                Some(code),
                host.as_deref(),
                Some(port),
                Some(&scheme),
                None,
                None,
                req.headers()
                    .get("x-arw-corr")
                    .and_then(|h| h.to_str().ok()),
                req.headers()
                    .get("x-arw-project")
                    .and_then(|h| h.to_str().ok()),
                None,
            )
            .await;
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Full::new(Bytes::from_static(b"Egress blocked")).boxed())
                .unwrap();
        }
    }
    if dns_guard() {
        let path = url.path().to_string();
        let doh_like =
            host.as_deref().map(is_doh_host).unwrap_or(false) || path.contains("/dns-query");
        let wants_dns_message = req
            .headers()
            .get("accept")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.contains("application/dns-message"))
            .unwrap_or(false)
            || req
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.contains("application/dns-message"))
                .unwrap_or(false);
        if doh_like || wants_dns_message {
            let caps = capability_candidates(host.as_deref(), Some(port), &scheme);
            if lease_grant(&state, &caps).await.is_none() {
                log_egress_event(
                    &state,
                    "deny",
                    Some("dns_guard"),
                    host.as_deref(),
                    Some(port),
                    Some(&scheme),
                    None,
                    None,
                    req.headers()
                        .get("x-arw-corr")
                        .and_then(|h| h.to_str().ok()),
                    req.headers()
                        .get("x-arw-project")
                        .and_then(|h| h.to_str().ok()),
                    None,
                )
                .await;
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"DNS guard")).boxed())
                    .unwrap();
            }
        }
    }
    // Policy
    let dec = state
        .policy()
        .lock()
        .await
        .evaluate_action("net.http.proxy");
    if !dec.allow {
        if let Some(cap) = dec.require_capability.as_deref() {
            let lease_ok = if let Some(kernel) = state.kernel_if_enabled() {
                kernel
                    .find_valid_lease_async("local", cap)
                    .await
                    .ok()
                    .flatten()
                    .is_some()
            } else {
                false
            };
            if !lease_ok {
                log_egress_event(
                    &state,
                    "deny",
                    Some("lease_required"),
                    host.as_deref(),
                    Some(port),
                    Some(&scheme),
                    None,
                    None,
                    req.headers()
                        .get("x-arw-corr")
                        .and_then(|h| h.to_str().ok()),
                    req.headers()
                        .get("x-arw-project")
                        .and_then(|h| h.to_str().ok()),
                    None,
                )
                .await;
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"Lease required")).boxed())
                    .unwrap();
            }
        }
    }
    // Build outbound request
    let method = req.method().clone();
    let mut rb = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        url,
    );
    // headers
    for (k, v) in req.headers().iter() {
        if let Ok(name) = reqwest::header::HeaderName::from_bytes(k.as_str().as_bytes()) {
            if let Ok(val) = reqwest::header::HeaderValue::from_bytes(v.as_bytes()) {
                // Skip hop-by-hop headers
                if name == reqwest::header::CONNECTION
                    || name == reqwest::header::PROXY_AUTHORIZATION
                    || name == reqwest::header::TE
                    || name == reqwest::header::UPGRADE
                {
                    continue;
                }
                rb = rb.header(name, val);
            }
        }
    }
    // body
    let body_bytes = match req.into_body().collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => Bytes::new(),
    };
    let body_len = body_bytes.len() as i64;
    if body_len > 0 {
        rb = rb.body(body_bytes.clone());
    }
    // Send
    let out = match rb.send().await {
        Ok(r) => r,
        Err(e) => {
            log_egress_event(
                &state,
                "error",
                Some("forward"),
                host.as_deref(),
                Some(port),
                Some(&scheme),
                None,
                Some(body_len),
                corr_id_hdr.as_deref(),
                proj_hdr.as_deref(),
                None,
            )
            .await;
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Full::new(Bytes::from(format!("Forward error: {}", e))).boxed())
                .unwrap();
        }
    };
    let status = out.status();
    let out_headers = out.headers().clone();
    let resp_bytes = match out.bytes().await {
        Ok(b) => b,
        Err(_) => Bytes::new(),
    };
    let bytes_in = resp_bytes.len() as i64;
    log_egress_event(
        &state,
        "allow",
        Some("http"),
        host.as_deref(),
        Some(port),
        Some(&scheme),
        Some(bytes_in),
        Some(body_len),
        corr_id_hdr.as_deref(),
        proj_hdr.as_deref(),
        None,
    )
    .await;
    let mut builder = Response::builder().status(status);
    // Copy a few safe headers
    if let Some(ct) = out_headers.get(reqwest::header::CONTENT_TYPE) {
        builder = builder.header("content-type", ct);
    }
    if let Some(cl) = out_headers.get(reqwest::header::CONTENT_LENGTH) {
        builder = builder.header("content-length", cl);
    } else {
        builder = builder.header("content-length", resp_bytes.len());
    }
    builder.body(Full::new(resp_bytes).boxed()).unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn maybe_log_egress(
    state: &AppState,
    decision: &str,
    reason: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    proto: Option<&str>,
    bytes_in: Option<i64>,
    bytes_out: Option<i64>,
    corr_id: Option<&str>,
    proj: Option<&str>,
    meta: Option<&serde_json::Value>,
) -> anyhow::Result<i64> {
    let mut row_id: i64 = 0;
    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1")
        && state.kernel_enabled()
    {
        if let Some(kernel) = state.kernel_if_enabled() {
            row_id = kernel
                .append_egress_async(
                    decision.to_string(),
                    reason.map(|s| s.to_string()),
                    host.map(|s| s.to_string()),
                    port.map(|p| p as i64),
                    proto.map(|s| s.to_string()),
                    bytes_in,
                    bytes_out,
                    corr_id.map(|s| s.to_string()),
                    proj.map(|s| s.to_string()),
                    Some(effective_posture()),
                    meta.cloned(),
                )
                .await?;
        }
    }
    // Publish SSE event (CloudEvents metadata applied by bus)
    let posture = effective_posture();
    state.bus().publish(
        topics::TOPIC_EGRESS_LEDGER_APPENDED,
        &serde_json::json!({
            "id": if row_id > 0 { serde_json::Value::from(row_id) } else { serde_json::Value::Null },
            "decision": decision,
            "reason": reason,
            "dest_host": host,
            "dest_port": port,
            "protocol": proto,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
            "corr_id": corr_id,
            "proj": proj,
            "posture": posture,
            "meta": meta.cloned().unwrap_or(serde_json::Value::Null)
        }),
    );
    Ok(row_id)
}

#[allow(clippy::too_many_arguments)]
async fn log_egress_event(
    state: &AppState,
    decision: &str,
    reason: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    proto: Option<&str>,
    bytes_in: Option<i64>,
    bytes_out: Option<i64>,
    corr_id: Option<&str>,
    proj: Option<&str>,
    meta: Option<serde_json::Value>,
) {
    if let Err(err) = maybe_log_egress(
        state,
        decision,
        reason,
        host,
        port,
        proto,
        bytes_in,
        bytes_out,
        corr_id,
        proj,
        meta.as_ref(),
    )
    .await
    {
        warn!(?err, "failed to append egress ledger entry");
    }
}
