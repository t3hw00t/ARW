use anyhow::Result;
use async_trait::async_trait;
use base64::Engine; // for base64 encode
use serde_json::Value;

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        ),
        Err(_) => default,
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WasiError {
    #[error("unsupported tool: {0}")]
    Unsupported(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("interrupted: {0}")]
    Interrupted(String),
    #[error("denied: {reason}")]
    Denied {
        reason: String,
        dest_host: Option<String>,
        dest_port: Option<i64>,
        protocol: Option<String>,
    },
}

/// Minimal abstraction for a future WASI Component host.
/// For now it provides a trait we can use in the server to route tool calls
/// when the runtime is available.
#[async_trait]
pub trait ToolHost: Send + Sync {
    async fn run_tool(&self, id: &str, input: &Value) -> Result<Value, WasiError>;
}

/// No-op host used while we bootstrap the runtime; returns Unsupported for all ids.
#[derive(Default, Clone)]
pub struct NoopHost;

#[async_trait]
impl ToolHost for NoopHost {
    async fn run_tool(&self, id: &str, _input: &Value) -> Result<Value, WasiError> {
        Err(WasiError::Unsupported(id.to_string()))
    }
}

/// Local host with a first built-in tool: http.fetch
#[derive(Clone)]
pub struct LocalHost {
    client: reqwest::Client,
    allowlist: Vec<String>,
}

impl LocalHost {
    pub fn new() -> Result<Self> {
        let timeout_s: u64 = std::env::var("ARW_HTTP_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        let mut cb = reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout_s));
        if std::env::var("ARW_EGRESS_PROXY_ENABLE").ok().as_deref() == Some("1") {
            let port: u16 = std::env::var("ARW_EGRESS_PROXY_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9080);
            let url = format!("http://127.0.0.1:{}", port);
            match reqwest::Proxy::all(&url) {
                Ok(p) => {
                    cb = cb.proxy(p);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(WasiError::Runtime(format!("proxy: {}", e))));
                }
            }
        }
        let client = cb.build().map_err(|e| WasiError::Runtime(e.to_string()))?;
        let allowlist: Vec<String> = std::env::var("ARW_NET_ALLOWLIST")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self { client, allowlist })
    }

    fn host_allowed(&self, host: &str) -> bool {
        if self.allowlist.is_empty() {
            return true;
        }
        let h = host.to_ascii_lowercase();
        self.allowlist.iter().any(|p| {
            let q = p.to_ascii_lowercase();
            h == q || h.ends_with(&format!(".{q}"))
        })
    }
}

#[async_trait]
impl ToolHost for LocalHost {
    async fn run_tool(&self, id: &str, input: &Value) -> Result<Value, WasiError> {
        match id {
            "http.fetch" => {
                let url_s = input
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WasiError::Runtime("missing url".into()))?;
                let url = url::Url::parse(url_s).map_err(|e| WasiError::Runtime(e.to_string()))?;
                let host = url.host_str().map(|s| s.to_string());
                let port = url.port().map(|p| p as i64);
                let scheme = url.scheme().to_string();
                // DNS guard: block DoH/DoT and dns-message payloads; then optional IP-literal guard
                if env_flag("ARW_DNS_GUARD_ENABLE", true) {
                    let doh_host = host.as_deref().map(is_doh_host).unwrap_or(false);
                    let dot_port = port == Some(853);
                    let is_doh_path = url.path().contains("/dns-query");
                    let is_dns_msg = input
                        .get("headers")
                        .and_then(|v| v.as_object())
                        .and_then(|h| h.get("accept").or_else(|| h.get("content-type")))
                        .and_then(|v| v.as_str())
                        .map(|s| s.contains("application/dns-message"))
                        .unwrap_or(false);
                    if doh_host || dot_port || is_doh_path || is_dns_msg {
                        return Err(WasiError::Denied {
                            reason: "dns_guard".into(),
                            dest_host: host.clone(),
                            dest_port: port,
                            protocol: Some(scheme),
                        });
                    }
                }
                // Optional: block IP-literal destinations
                if env_flag("ARW_EGRESS_BLOCK_IP_LITERALS", false) {
                    if let Some(h) = &host {
                        if h.parse::<std::net::IpAddr>().is_ok() {
                            return Err(WasiError::Denied {
                                reason: "ip_literal".into(),
                                dest_host: Some(h.clone()),
                                dest_port: port,
                                protocol: Some(scheme),
                            });
                        }
                    }
                }
                if let Some(h) = &host {
                    if !self.host_allowed(h) {
                        return Err(WasiError::Denied {
                            reason: "allowlist".into(),
                            dest_host: Some(h.clone()),
                            dest_port: port,
                            protocol: Some(scheme),
                        });
                    }
                }
                // Optional connector token injection
                let mut auth_token: Option<String> = None;
                let mut allowed_hosts_from_connector: Option<Vec<String>> = None;
                let mut connector_provider: Option<String> = None;
                if let Some(cid) = input.get("connector_id").and_then(|v| v.as_str()) {
                    let base = std::env::var("ARW_STATE_DIR").unwrap_or_else(|_| "state".into());
                    let cpath = std::path::Path::new(&base)
                        .join("connectors")
                        .join(format!("{}.json", cid));
                    if let Ok(bytes) = tokio::fs::read(&cpath).await {
                        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                            auth_token = v
                                .get("token")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string());
                            allowed_hosts_from_connector = v
                                .get("meta")
                                .and_then(|m| m.get("allowed_hosts"))
                                .and_then(|arr| arr.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                        .collect()
                                });
                            connector_provider = v
                                .get("provider")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
                // Provider-inferred default hosts when not explicitly set
                let inferred_hosts: Option<Vec<String>> = if allowed_hosts_from_connector.is_none()
                {
                    match connector_provider.as_deref() {
                        Some("github") => Some(vec!["api.github.com".into()]),
                        Some("slack") => Some(vec!["api.slack.com".into()]),
                        Some("notion") => Some(vec!["api.notion.com".into()]),
                        Some("google") => Some(vec!["www.googleapis.com".into()]),
                        Some("microsoft") | Some("graph") => {
                            Some(vec!["graph.microsoft.com".into()])
                        }
                        Some("dropbox") => Some(vec![
                            "api.dropboxapi.com".into(),
                            "content.dropboxapi.com".into(),
                        ]),
                        Some("box") => Some(vec!["api.box.com".into()]),
                        _ => None,
                    }
                } else {
                    None
                };
                let allow_hosts = allowed_hosts_from_connector
                    .as_ref()
                    .or(inferred_hosts.as_ref());
                if let (Some(h), Some(allow)) = (&host, allow_hosts) {
                    let hlow = h.to_ascii_lowercase();
                    let ok = allow.iter().any(|p| {
                        let plow = p.to_ascii_lowercase();
                        hlow == plow || hlow.ends_with(&format!(".{plow}"))
                    });
                    if !ok {
                        return Err(WasiError::Denied {
                            reason: "connector_host".into(),
                            dest_host: Some(h.clone()),
                            dest_port: port,
                            protocol: Some(scheme),
                        });
                    }
                }
                let method = input
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET")
                    .to_ascii_uppercase();
                let head_kb: usize = std::env::var("ARW_HTTP_BODY_HEAD_KB")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(64);
                let mut req = match method.as_str() {
                    "GET" => self.client.get(url.clone()),
                    "POST" => {
                        let mut rb = self.client.post(url.clone());
                        if let Some(ct) = input.get("content_type").and_then(|v| v.as_str()) {
                            rb = rb.header("content-type", ct);
                        }
                        if let Some(body) = input.get("body").and_then(|v| v.as_str()) {
                            rb = rb.body(body.to_string());
                        }
                        rb
                    }
                    _ => return Err(WasiError::Unsupported(format!("http method {}", method))),
                };
                if let Some(hdrs) = input.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in hdrs.iter() {
                        if let Some(sv) = v.as_str() {
                            if auth_token.is_some() && k.eq_ignore_ascii_case("authorization") {
                                continue;
                            }
                            req = req.header(k.as_str(), sv);
                        }
                    }
                }
                if let Some(tok) = &auth_token {
                    req = req.header("authorization", format!("Bearer {}", tok));
                }
                let resp = req
                    .send()
                    .await
                    .map_err(|e| WasiError::Runtime(e.to_string()))?;
                let status = resp.status().as_u16();
                let body = resp
                    .bytes()
                    .await
                    .map_err(|e| WasiError::Runtime(e.to_string()))?;
                let bytes_in = body.len() as i64;
                let head = &body[..std::cmp::min(body.len(), head_kb * 1024)];
                let head_b64 = base64::engine::general_purpose::STANDARD.encode(head);
                let out = serde_json::json!({
                    "status": status,
                    "url": url.as_str(),
                    "dest_host": host,
                    "dest_port": port,
                    "protocol": scheme,
                    "bytes_in": bytes_in,
                    "body_head_b64": head_b64
                });
                Ok(out)
            }
            "app.vscode.open" => {
                let p = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WasiError::Runtime("missing path".into()))?;
                let base = std::env::var("ARW_STATE_DIR").unwrap_or_else(|_| "state".into());
                let projects = std::path::Path::new(&base).join("projects");
                let full = if std::path::Path::new(p).is_absolute() {
                    std::path::PathBuf::from(p)
                } else {
                    projects.join(p)
                };
                let _ = std::fs::create_dir_all(full.parent().unwrap_or(&projects));
                match std::process::Command::new("code")
                    .arg(full.as_os_str())
                    .spawn()
                {
                    Ok(child) => Ok(serde_json::json!({"started": true, "pid": child.id()})),
                    Err(e) => Err(WasiError::Runtime(format!("spawn: {}", e))),
                }
            }
            "fs.patch" => {
                // Inputs: path (string), content (string, optional), pre_sha256 (optional), mkdirs (bool, default true)
                let path_s = input
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| WasiError::Runtime("missing path".into()))?;
                let content = input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .as_bytes()
                    .to_vec();
                let pre = input.get("pre_sha256").and_then(|v| v.as_str());
                let mkdirs = input
                    .get("mkdirs")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                // Resolve under ARW_STATE_DIR/projects for safety
                let base = std::env::var("ARW_STATE_DIR").unwrap_or_else(|_| "state".into());
                let base_path = std::path::Path::new(&base).join("projects");
                let full = std::path::Path::new(path_s);
                let target = if full.is_absolute() {
                    full.to_path_buf()
                } else {
                    base_path.join(full)
                };
                let _canon_base = std::fs::canonicalize(&base_path).unwrap_or(base_path.clone());
                let parent = target
                    .parent()
                    .ok_or_else(|| WasiError::Runtime("bad path".into()))?
                    .to_path_buf();
                if mkdirs {
                    std::fs::create_dir_all(&parent)
                        .map_err(|e| WasiError::Runtime(e.to_string()))?;
                }
                // If precondition provided, verify existing file sha256
                if let Some(expect) = pre {
                    if let Ok(existing) = std::fs::read(&target) {
                        let mut h = sha2::Sha256::new();
                        use sha2::Digest as _;
                        h.update(&existing);
                        let got = format!("{:x}", h.finalize());
                        if got != expect {
                            return Err(WasiError::Runtime("precondition sha mismatch".into()));
                        }
                    }
                }
                // Write atomically: temp then rename
                let mut h2 = sha2::Sha256::new();
                use sha2::Digest as _;
                h2.update(&content);
                let sha = format!("{:x}", h2.finalize());
                let tmp = parent.join(format!(".patch-{}.tmp", sha));
                std::fs::write(&tmp, &content).map_err(|e| WasiError::Runtime(e.to_string()))?;
                std::fs::rename(&tmp, &target).map_err(|e| WasiError::Runtime(e.to_string()))?;
                Ok(serde_json::json!({
                    "path": target.to_string_lossy(),
                    "bytes": content.len(),
                    "sha256": sha
                }))
            }
            _ => Err(WasiError::Unsupported(id.to_string())),
        }
    }
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
