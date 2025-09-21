use super::{ToolError, Value};
use once_cell::sync::{Lazy, OnceCell};
use regex::Regex;
use serde::Serialize;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use url::Url;

static GUARD_RETRIES: AtomicU64 = AtomicU64::new(0);
static GUARD_HTTP_ERRORS: AtomicU64 = AtomicU64::new(0);
static GUARD_CB_TRIPS: AtomicU64 = AtomicU64::new(0);

struct CircuitBreaker {
    fail_count: AtomicU64,
    open_until_ms: AtomicU64,
}

static CB: OnceCell<CircuitBreaker> = OnceCell::new();

static RE_EMAIL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}").expect("email regex")
});
static RE_AWS: Lazy<Regex> = Lazy::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws regex"));
static RE_GAPI: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"AIza[0-9A-Za-z\-_]{35}").expect("gapi regex"));
static RE_SLACK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"xox[baprs]-[0-9A-Za-z-]{10,}").expect("slack regex"));
static RE_URL: Lazy<Regex> = Lazy::new(|| Regex::new(r"https?://[^\\s)]+").expect("url regex"));

pub(crate) async fn run(input: &Value) -> Result<Value, ToolError> {
    let text = input
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::Invalid("missing 'text'".into()))?;

    if let Some(base) = std::env::var("ARW_GUARDRAILS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        let cb = CB.get_or_init(|| CircuitBreaker {
            fail_count: AtomicU64::new(0),
            open_until_ms: AtomicU64::new(0),
        });
        let now_ms = now_millis();
        let open_until = cb.open_until_ms.load(Ordering::Relaxed);
        if now_ms >= open_until {
            if let Some(remote) = guardrails_remote(text, input, &base, cb).await? {
                return Ok(remote);
            }
        }
    }

    guardrails_local(text)
}

pub(crate) fn metrics() -> Value {
    let cb = CB.get_or_init(|| CircuitBreaker {
        fail_count: AtomicU64::new(0),
        open_until_ms: AtomicU64::new(0),
    });
    let now = now_millis();
    let open_until = cb.open_until_ms.load(Ordering::Relaxed);
    json!({
        "retries": GUARD_RETRIES.load(Ordering::Relaxed),
        "http_errors": GUARD_HTTP_ERRORS.load(Ordering::Relaxed),
        "cb_trips": GUARD_CB_TRIPS.load(Ordering::Relaxed),
        "cb_open": (now < open_until) as u8,
        "cb_open_until_ms": open_until,
    })
}

async fn guardrails_remote(
    text: &str,
    input: &Value,
    base: &str,
    cb: &CircuitBreaker,
) -> Result<Option<Value>, ToolError> {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
    let timeout_ms: u64 = std::env::var("ARW_GUARDRAILS_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3_000);
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_millis(timeout_ms.max(1)))
            .build()
            .expect("guardrails client")
    });

    let url = format!("{}/check", base.trim_end_matches('/'));
    let mut body = json!({"text": text});
    if let Some(policy) = input.get("policy") {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("policy".into(), policy.clone());
        }
    }
    if let Some(rules) = input.get("rules") {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("rules".into(), rules.clone());
        }
    }

    let max_retries: u32 = std::env::var("ARW_GUARDRAILS_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

    let mut attempt: u32 = 0;
    let mut out_opt: Option<Value> = None;
    while attempt <= max_retries {
        let resp = client.post(&url).json(&body).send().await;
        match resp {
            Ok(rsp) if rsp.status().is_success() => match rsp.json::<Value>().await {
                Ok(v) => {
                    out_opt = Some(json!({
                        "ok": v.get("ok").and_then(|b| b.as_bool()).unwrap_or(true),
                        "score": v.get("score").cloned().unwrap_or(json!(0.0)),
                        "issues": v.get("issues").cloned().unwrap_or(json!([])),
                        "suggestions": v.get("suggestions").cloned().unwrap_or(json!([])),
                    }));
                    cb.fail_count.store(0, Ordering::Relaxed);
                    cb.open_until_ms.store(0, Ordering::Relaxed);
                    break;
                }
                Err(_) => {
                    GUARD_HTTP_ERRORS.fetch_add(1, Ordering::Relaxed);
                }
            },
            _ => {
                GUARD_HTTP_ERRORS.fetch_add(1, Ordering::Relaxed);
            }
        }
        attempt += 1;
        if attempt <= max_retries {
            GUARD_RETRIES.fetch_add(1, Ordering::Relaxed);
            let base_delay = 100u64 * (1u64 << (attempt - 1).min(4));
            let jitter = random_jitter(base_delay / 2);
            sleep(Duration::from_millis(base_delay + jitter)).await;
        }
    }

    if let Some(out) = out_opt {
        return Ok(Some(out));
    }

    let failures = cb.fail_count.fetch_add(1, Ordering::Relaxed) + 1;
    let threshold: u64 = std::env::var("ARW_GUARDRAILS_CB_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .max(1);
    if failures >= threshold {
        let cooldown_ms: u64 = std::env::var("ARW_GUARDRAILS_CB_COOLDOWN_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30_000);
        cb.open_until_ms
            .store(now_millis().saturating_add(cooldown_ms), Ordering::Relaxed);
        cb.fail_count.store(0, Ordering::Relaxed);
        GUARD_CB_TRIPS.fetch_add(1, Ordering::Relaxed);
    }

    Ok(None)
}

fn guardrails_local(text: &str) -> Result<Value, ToolError> {
    #[derive(Serialize)]
    struct Issue {
        code: String,
        severity: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        span: Option<(usize, usize)>,
    }

    let mut issues: Vec<Issue> = Vec::new();
    for m in RE_EMAIL.find_iter(text) {
        issues.push(Issue {
            code: "pii.email".into(),
            severity: "medium".into(),
            message: "Email address detected".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_AWS.find_iter(text) {
        issues.push(Issue {
            code: "secret.aws_access_key".into(),
            severity: "high".into(),
            message: "AWS access key pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_GAPI.find_iter(text) {
        issues.push(Issue {
            code: "secret.gcp_api_key".into(),
            severity: "high".into(),
            message: "Google API key pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }
    for m in RE_SLACK.find_iter(text) {
        issues.push(Issue {
            code: "secret.slack_token".into(),
            severity: "high".into(),
            message: "Slack token pattern".into(),
            span: Some((m.start(), m.end())),
        });
    }

    let allowlist: Vec<String> = std::env::var("ARW_GUARDRAILS_ALLOWLIST")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    for m in RE_URL.find_iter(text) {
        let url = m.as_str();
        if let Ok(parsed) = Url::parse(url) {
            let host = parsed.host_str().unwrap_or("").to_lowercase();
            if !allowlist.is_empty()
                && !allowlist
                    .iter()
                    .any(|h| host == *h || host.ends_with(&format!(".{h}")))
            {
                issues.push(Issue {
                    code: "egress.unlisted_host".into(),
                    severity: "medium".into(),
                    message: format!("URL host not in allowlist: {}", host),
                    span: Some((m.start(), m.end())),
                });
            }
        }
    }

    let markers = [
        "ignore previous",
        "disregard prior",
        "override instructions",
        "exfiltrate",
    ];
    let lower = text.to_ascii_lowercase();
    for pat in markers.iter() {
        if let Some(pos) = lower.find(pat) {
            issues.push(Issue {
                code: "prompt_injection.marker".into(),
                severity: "medium".into(),
                message: format!("Suspicious instruction: '{}'", pat),
                span: Some((pos, pos + pat.len())),
            });
        }
    }

    let mut score = 0.0;
    for issue in &issues {
        score += match issue.severity.as_str() {
            "high" => 3.0,
            "medium" => 1.0,
            _ => 0.5,
        };
    }
    let ok = issues.iter().all(|i| i.severity != "high");
    Ok(json!({
        "ok": ok,
        "score": score,
        "issues": issues,
        "suggestions": []
    }))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn random_jitter(cap: u64) -> u64 {
    if cap == 0 {
        return 0;
    }
    now_millis() % cap.max(1)
}
