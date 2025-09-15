use crate::{util::effective_posture, AppState};
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt as _, Empty, Full};
use hyper::body::Incoming as IncomingBody;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use once_cell::sync::Lazy;
use std::convert::Infallible;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

type ProxyBody = BoxBody<Bytes, Infallible>;

#[allow(dead_code)]
fn empty_body() -> ProxyBody {
    Empty::<Bytes>::new().boxed()
}
#[allow(dead_code)]
fn bytes_body(b: Bytes) -> ProxyBody {
    Full::new(b).boxed()
}

fn dns_guard() -> bool {
    std::env::var("ARW_DNS_GUARD_ENABLE").ok().as_deref() == Some("1")
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
    let enable = std::env::var("ARW_EGRESS_PROXY_ENABLE").ok().as_deref() == Some("1");
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
        .timeout(std::time::Duration::from_secs(30))
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
    // Guards
    if std::env::var("ARW_EGRESS_BLOCK_IP_LITERALS")
        .ok()
        .as_deref()
        == Some("1")
        && host.parse::<std::net::IpAddr>().is_ok()
    {
        let _ = maybe_log_egress(
            &state,
            "deny",
            Some("ip_literal"),
            Some(&host),
            Some(port),
            Some("tcp"),
            None,
            None,
            corr_id_hdr.as_deref(),
            proj_hdr.as_deref(),
        );
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Full::new(Bytes::from_static(b"IP literal blocked")).boxed())
            .unwrap();
    }
    if dns_guard() && (port == 853 || is_doh_host(&host)) {
        let _ = maybe_log_egress(
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
        );
        return Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Full::new(Bytes::from_static(b"DNS guard")).boxed())
            .unwrap();
    }
    if let Ok(list) = std::env::var("ARW_NET_ALLOWLIST") {
        if !list.trim().is_empty() {
            let hosts: Vec<String> = list
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let hlow = host.to_ascii_lowercase();
            let ok = hosts.iter().any(|p| {
                let plow = p.to_ascii_lowercase();
                hlow == plow || hlow.ends_with(&format!(".{plow}"))
            });
            if !ok {
                let _ = maybe_log_egress(
                    &state,
                    "deny",
                    Some("allowlist"),
                    Some(&host),
                    Some(port),
                    Some("tcp"),
                    None,
                    None,
                    corr_id_hdr.as_deref(),
                    proj_hdr.as_deref(),
                );
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"Host not in allowlist")).boxed())
                    .unwrap();
            }
        }
    }
    // Policy
    let dec = state.policy.lock().await.evaluate_action("net.tcp.connect");
    if !dec.allow {
        if let Some(cap) = dec.require_capability.as_deref() {
            if state
                .kernel
                .find_valid_lease("local", cap)
                .ok()
                .flatten()
                .is_none()
            {
                let _ = maybe_log_egress(
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
                );
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"Lease required")).boxed())
                    .unwrap();
            }
        }
    }
    // Establish TCP to target
    let target = format!("{}:{}", host, port);
    let mut server_stream = match tokio::net::TcpStream::connect(target).await {
        Ok(s) => s,
        Err(e) => {
            warn!("connect failed: {}", e);
            let _ = maybe_log_egress(
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
            );
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
                let _ = maybe_log_egress(
                    &st,
                    "allow",
                    Some("connect"),
                    Some(&host),
                    Some(port),
                    Some("tcp"),
                    Some(s2c_bytes as i64),
                    Some(c2s_bytes as i64),
                    corr_id_hdr.as_deref(),
                    proj_hdr.as_deref(),
                );
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
    // Guards
    if std::env::var("ARW_EGRESS_BLOCK_IP_LITERALS")
        .ok()
        .as_deref()
        == Some("1")
    {
        if let Some(h) = &host {
            if h.parse::<std::net::IpAddr>().is_ok() {
                let _ = maybe_log_egress(
                    &state,
                    "deny",
                    Some("ip_literal"),
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
                );
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Full::new(Bytes::from_static(b"IP literal blocked")).boxed())
                    .unwrap();
            }
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
            let _ = maybe_log_egress(
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
            );
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Full::new(Bytes::from_static(b"DNS guard")).boxed())
                .unwrap();
        }
    }
    if let Ok(list) = std::env::var("ARW_NET_ALLOWLIST") {
        if !list.trim().is_empty() {
            let hosts: Vec<String> = list
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            if let Some(h) = &host {
                let hlow = h.to_ascii_lowercase();
                let ok = hosts.iter().any(|p| {
                    let plow = p.to_ascii_lowercase();
                    hlow == plow || hlow.ends_with(&format!(".{plow}"))
                });
                if !ok {
                    let _ = maybe_log_egress(
                        &state,
                        "deny",
                        Some("allowlist"),
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
                    );
                    return Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(Full::new(Bytes::from_static(b"Host not in allowlist")).boxed())
                        .unwrap();
                }
            }
        }
    }
    // Policy
    let dec = state.policy.lock().await.evaluate_action("net.http.proxy");
    if !dec.allow {
        if let Some(cap) = dec.require_capability.as_deref() {
            if state
                .kernel
                .find_valid_lease("local", cap)
                .ok()
                .flatten()
                .is_none()
            {
                let _ = maybe_log_egress(
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
                );
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
            let _ = maybe_log_egress(
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
            );
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
    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
        let _ = state.kernel.append_egress(
            "allow",
            Some("http"),
            host.as_deref(),
            Some(port as i64).as_ref().copied(),
            Some(&scheme),
            Some(bytes_in),
            Some(body_len),
            corr_id_hdr.as_deref(),
            proj_hdr.as_deref(),
            Some(&effective_posture()),
        );
    }
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
fn maybe_log_egress(
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
) -> anyhow::Result<i64> {
    let mut row_id: i64 = 0;
    if std::env::var("ARW_EGRESS_LEDGER_ENABLE").ok().as_deref() == Some("1") {
        row_id = state.kernel.append_egress(
            decision,
            reason,
            host,
            port.map(|p| p as i64).as_ref().copied(),
            proto,
            bytes_in,
            bytes_out,
            corr_id,
            proj,
            Some(&effective_posture()),
        )?;
    }
    // Publish SSE event (CloudEvents metadata applied by bus)
    let posture = effective_posture();
    state.bus.publish(
        "egress.ledger.appended",
        &serde_json::json!({
            "id": if row_id>0 { serde_json::Value::from(row_id) } else { serde_json::Value::Null },
            "decision": decision,
            "reason": reason,
            "dest_host": host,
            "dest_port": port,
            "protocol": proto,
            "bytes_in": bytes_in,
            "bytes_out": bytes_out,
            "corr_id": corr_id,
            "proj": proj,
            "posture": posture
        }),
    );
    Ok(row_id)
}
