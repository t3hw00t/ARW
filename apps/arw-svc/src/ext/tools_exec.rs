use moka::sync::Cache;
use once_cell::sync::OnceCell;
use serde_json::{json, Map, Value};
use sha2::Digest as _;
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Condvar, Mutex, RwLock,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct Entry {
    summary: &'static str,
    exec: fn(&Value) -> Result<Value, String>,
}

static REG: OnceCell<RwLock<HashMap<&'static str, Entry>>> = OnceCell::new();

fn reg() -> &'static RwLock<HashMap<&'static str, Entry>> {
    REG.get_or_init(|| {
        let mut map: HashMap<&'static str, Entry> = HashMap::new();
        // Built-in examples
        map.insert(
            "math.add",
            Entry {
                summary:
                    "Add two numbers: input {\"a\": number, \"b\": number} -> {\"sum\": number}",
                exec: |input| {
                    let a = input
                        .get("a")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'a'")?;
                    let b = input
                        .get("b")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'b'")?;
                    Ok(json!({"sum": a + b}))
                },
            },
        );
        map.insert(
            "time.now",
            Entry {
                summary: "UTC time in ms: input {} -> {\"now_ms\": number}",
                exec: |_input| {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis() as i64;
                    Ok(json!({"now_ms": now}))
                },
            },
        );
        RwLock::new(map)
    })
}

// ---- Action Cache (MVP scaffold) ----
// Map: action_key -> content_digest (sha256 hex of serialized output)
// In-memory index with TTL and capacity (W-TinyLFU via moka)
static ACTION_MEM: OnceCell<Cache<String, String>> = OnceCell::new();
fn cache_capacity() -> u64 {
    std::env::var("ARW_TOOLS_CACHE_CAP")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(2048)
}
fn cache_ttl() -> Duration {
    let secs = std::env::var("ARW_TOOLS_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(600);
    Duration::from_secs(secs.max(1))
}
fn action_mem() -> &'static Cache<String, String> {
    ACTION_MEM.get_or_init(|| {
        Cache::builder()
            .max_capacity(cache_capacity())
            .time_to_live(cache_ttl())
            .build()
    })
}

fn tools_cas_dir() -> PathBuf {
    super::paths::state_dir().join("tools").join("by-digest")
}

fn canonicalize_json(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            // Sort keys to achieve a stable representation
            let mut pairs: Vec<(&String, &Value)> = m.iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = Map::new();
            for (k, val) in pairs.into_iter() {
                out.insert(k.clone(), canonicalize_json(val));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        _ => v.clone(),
    }
}

fn compute_action_key(tool_id: &str, tool_ver: &str, input: &Value) -> String {
    // Compose a stable key from tool id@version, an environment/policy signature,
    // and a canonicalized representation of input.
    let mut hasher = sha2::Sha256::new();
    hasher.update(tool_id.as_bytes());
    hasher.update(b"@\0");
    hasher.update(tool_ver.as_bytes());
    hasher.update(b"\0");
    // Environment/policy signature: include non-secret markers that should bust cache
    // when policy or secret versions change. We avoid hashing actual secret values.
    // Recognized markers (optional):
    // - ARW_POLICY_VERSION, ARW_SECRETS_VERSION
    // - ARW_PROJECT_ID, ARW_NET_POSTURE
    // - ARW_TOOLS_CACHE_SALT (manual salt)
    // Additionally, include a compact hash of the gating snapshot (deny lists/contracts).
    fn env_signature() -> String {
        let mut pairs: Vec<(String, String)> = Vec::new();
        let add = |k: &str, pairs: &mut Vec<(String, String)>| {
            if let Ok(v) = std::env::var(k) {
                if !v.is_empty() {
                    pairs.push((k.to_string(), v));
                }
            }
        };
        // Known version markers and posture context
        for k in [
            "ARW_POLICY_VERSION",
            "ARW_SECRETS_VERSION",
            "ARW_PROJECT_ID",
            "ARW_NET_POSTURE",
            "ARW_TOOLS_CACHE_SALT",
        ] {
            add(k, &mut pairs);
        }
        // Back-compat aliases sometimes used in setups
        for k in ["ARW_POLICY_VER", "ARW_SECRETS_VER"] {
            add(k, &mut pairs);
        }
        // Include a short hash of gating snapshot (policy denies/contracts)
        // to invalidate caches when policy is updated at runtime.
        let gating_hash = {
            let snap = arw_core::gating::snapshot();
            let bytes = serde_json::to_vec(&snap).unwrap_or_default();
            let mut h = sha2::Sha256::new();
            h.update(&bytes);
            format!("{:x}", h.finalize())
        };
        pairs.push(("GATING".into(), gating_hash));
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = String::new();
        for (k, v) in pairs.into_iter() {
            out.push_str(&k);
            out.push('=');
            out.push_str(&v);
            out.push(';');
        }
        out
    }
    let env_sig = env_signature();
    hasher.update(b"env:\0");
    hasher.update(env_sig.as_bytes());
    hasher.update(b"\0");

    let canon = canonicalize_json(input);
    let bytes = serde_json::to_vec(&canon).unwrap_or_default();
    hasher.update(&bytes);
    format!("{:x}", hasher.finalize())
}

fn compute_digest(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn tool_version(id: &str) -> &'static str {
    for ti in arw_core::introspect_tools() {
        if ti.id == id {
            return ti.version;
        }
    }
    // Unknown in registry (builtin examples):
    "0.0.0"
}

// Singleflight to coalesce identical misses
struct SfEntry {
    inner: Mutex<SfInner>,
    cv: Condvar,
}
struct SfInner {
    done: bool,
    result: Option<Result<Value, String>>,
}
static SINGLEFLIGHT: OnceCell<Mutex<HashMap<String, Arc<SfEntry>>>> = OnceCell::new();
fn sf_map() -> &'static Mutex<HashMap<String, Arc<SfEntry>>> {
    SINGLEFLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}
fn sf_begin(key: &str) -> (Arc<SfEntry>, bool) {
    let mut m = sf_map().lock().unwrap();
    if let Some(e) = m.get(key) {
        return (Arc::clone(e), false);
    }
    let ent = Arc::new(SfEntry {
        inner: Mutex::new(SfInner {
            done: false,
            result: None,
        }),
        cv: Condvar::new(),
    });
    m.insert(key.to_string(), Arc::clone(&ent));
    (ent, true)
}
fn sf_end(key: &str) {
    let mut m = sf_map().lock().unwrap();
    m.remove(key);
}

pub fn run(id: &str, input: &Value) -> Result<Value, String> {
    let (out, _, _, _, _) = run_with_cache_stats(id, input)?;
    Ok(out)
}

// Returns (output, outcome, digest_opt, action_key, age_secs)
// outcome: "hit" | "miss" | "coalesced"
pub type ToolRunOutcome = (Value, &'static str, Option<String>, String, Option<u64>);
pub fn run_with_cache_stats(id: &str, input: &Value) -> Result<ToolRunOutcome, String> {
    let map = reg().read().unwrap();
    let ent = match map.get(id) {
        Some(e) => e,
        None => return Err(format!("unknown tool id: {}", id)),
    };
    let ver = tool_version(id);
    let key = compute_action_key(id, ver, input);

    // Fast path: in-memory index → disk CAS
    if let Some(digest) = action_mem().get(&key) {
        let path = tools_cas_dir().join(format!("{}.json", digest));
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
                CACHE_HIT.fetch_add(1, Ordering::Relaxed);
                // Age from file mtime (seconds)
                let age_secs = fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .map(|d| d.as_secs());
                return Ok((v, "hit", Some(digest), key, age_secs));
            }
        }
        // stale: continue to compute
    }

    // Coalesce identical misses
    let (sf, is_leader) = sf_begin(&key);
    if !is_leader {
        // Wait for leader to finish
        let mut guard = sf.inner.lock().unwrap();
        while !guard.done {
            guard = sf.cv.wait(guard).unwrap();
        }
        if let Some(res) = guard.result.clone() {
            drop(guard);
            let (v, d_opt) = match res {
                Ok(v) => {
                    // Compute digest for event; best-effort
                    let d = serde_json::to_vec(&v).ok().map(|b| compute_digest(&b));
                    (v, d)
                }
                Err(e) => return Err(e),
            };
            CACHE_COALESCED.fetch_add(1, Ordering::Relaxed);
            return Ok((v, "coalesced", d_opt, key, None));
        }
        // Should not happen; fall through to compute
    }

    // Leader: execute and store
    let res = (ent.exec)(input);
    let outcome: Result<(Value, Option<String>), String> = match res {
        Ok(out) => {
            let digest_opt = match serde_json::to_vec(&out) {
                Ok(bytes) => {
                    let digest = compute_digest(&bytes);
                    let dir = tools_cas_dir();
                    let _ = fs::create_dir_all(&dir);
                    let path = dir.join(format!("{}.json", &digest));
                    if !path.exists() {
                        if let Ok(mut f) = fs::File::create(&path) {
                            let _ = f.write_all(&bytes);
                        }
                    }
                    action_mem().insert(key.clone(), digest.clone());
                    Some(digest)
                }
                Err(_) => None,
            };
            Ok((out, digest_opt))
        }
        Err(e) => Err(e),
    };
    // Publish to followers and clean up
    let mut inner = sf.inner.lock().unwrap();
    match &outcome {
        Ok((v, _)) => {
            inner.result = Some(Ok(v.clone()));
        }
        Err(e) => {
            inner.result = Some(Err(e.clone()));
        }
    }
    inner.done = true;
    sf.cv.notify_all();
    drop(inner);
    sf_end(&key);

    match outcome {
        Ok((v, d)) => {
            CACHE_MISS.fetch_add(1, Ordering::Relaxed);
            Ok((v, "miss", d, key, Some(0)))
        }
        Err(e) => Err(e),
    }
}

// ---- Counters and stats ----
static CACHE_HIT: AtomicU64 = AtomicU64::new(0);
static CACHE_MISS: AtomicU64 = AtomicU64::new(0);
static CACHE_COALESCED: AtomicU64 = AtomicU64::new(0);

pub fn cache_stats_value() -> Value {
    json!({
        "hit": CACHE_HIT.load(Ordering::Relaxed),
        "miss": CACHE_MISS.load(Ordering::Relaxed),
        "coalesced": CACHE_COALESCED.load(Ordering::Relaxed),
        "capacity": cache_capacity(),
        "ttl_secs": cache_ttl().as_secs(),
        "entries": action_mem().entry_count() as u64,
    })
}

pub fn list() -> Vec<(&'static str, &'static str)> {
    let map = reg().read().unwrap();
    map.iter().map(|(k, v)| (*k, v.summary)).collect()
}
