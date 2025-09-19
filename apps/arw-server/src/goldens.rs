use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::{http_timeout, util, world};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GoldenItem {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub expect: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct GoldenSet {
    pub items: Vec<GoldenItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct EvalOptions {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<usize>,
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_format: Option<String>,
    #[serde(default)]
    pub include_provenance: Option<bool>,
    #[serde(default)]
    pub context_item_template: Option<String>,
    #[serde(default)]
    pub context_header: Option<String>,
    #[serde(default)]
    pub context_footer: Option<String>,
    #[serde(default)]
    pub joiner: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EvalResultItem {
    pub id: String,
    pub ok: bool,
    pub latency_ms: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EvalSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub avg_latency_ms: u64,
    pub items: Vec<EvalResultItem>,
    #[serde(default)]
    pub avg_ctx_tokens: u64,
    #[serde(default)]
    pub avg_ctx_items: u64,
}

fn goldens_dir() -> std::path::PathBuf {
    util::state_dir().join("goldens")
}

fn goldens_path(proj: &str) -> std::path::PathBuf {
    goldens_dir().join(format!("{}.json", proj))
}

pub async fn load(proj: &str) -> GoldenSet {
    let path = goldens_path(proj);
    match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice::<GoldenSet>(&bytes).unwrap_or_default(),
        Err(_) => GoldenSet::default(),
    }
}

pub async fn save(proj: &str, set: &GoldenSet) -> Result<(), String> {
    let dir = goldens_dir();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return Err(e.to_string());
    }
    let path = goldens_path(proj);
    save_json_atomically(&path, &serde_json::to_value(set).unwrap()).await
}

pub async fn evaluate_chat_items(
    set: &GoldenSet,
    opts: &EvalOptions,
    proj: Option<&str>,
) -> EvalSummary {
    let mut items = Vec::new();
    let mut passed = 0usize;
    let mut total_latency = 0u64;
    let limit = opts.limit.unwrap_or(usize::MAX);
    let vote_k = opts.vote_k.unwrap_or(1).clamp(1, 9);
    let mut ctx_tokens_acc = 0u64;
    let mut ctx_items_acc = 0u64;
    let mut ctx_count = 0u64;
    for item in set.items.iter().filter(|i| i.kind == "chat").take(limit) {
        let prompt = item
            .input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let (context, ctx_tokens, ctx_items) =
            assemble_context(prompt, opts, proj.unwrap_or("default"));
        if ctx_tokens > 0 {
            ctx_tokens_acc += ctx_tokens;
            ctx_items_acc += ctx_items;
            ctx_count += 1;
        }
        let mut full_prompt = String::new();
        if !context.is_empty() {
            if let Some(header) = opts.context_header.as_deref() {
                full_prompt.push_str(header);
                if !header.ends_with('\n') {
                    full_prompt.push('\n');
                }
            }
            full_prompt.push_str(&context);
            if let Some(footer) = opts.context_footer.as_deref() {
                if !full_prompt.ends_with('\n') {
                    full_prompt.push('\n');
                }
                full_prompt.push_str(footer);
            }
            if !full_prompt.ends_with('\n') {
                full_prompt.push('\n');
            }
        }
        full_prompt.push_str(prompt);
        let start = std::time::Instant::now();
        let reply = best_of_k(&full_prompt, vote_k, opts.temperature).await;
        let latency = start.elapsed().as_millis() as u64;
        let (ok, note) = match reply {
            Some(ref ans) => score_text(&item.expect, ans),
            None => (false, Some("no_reply".into())),
        };
        if ok {
            passed += 1;
        }
        total_latency = total_latency.saturating_add(latency);
        items.push(EvalResultItem {
            id: item.id.clone(),
            ok,
            latency_ms: latency,
            note,
        });
    }
    let total = items.len();
    let avg_latency_ms = if total > 0 {
        total_latency / (total as u64)
    } else {
        0
    };
    EvalSummary {
        total,
        passed,
        failed: total.saturating_sub(passed),
        avg_latency_ms,
        items,
        avg_ctx_tokens: if ctx_count > 0 {
            ctx_tokens_acc / ctx_count
        } else {
            0
        },
        avg_ctx_items: if ctx_count > 0 {
            ctx_items_acc / ctx_count
        } else {
            0
        },
    }
}

fn assemble_context(prompt: &str, opts: &EvalOptions, proj: &str) -> (String, u64, u64) {
    let mut entries = if let Some(k) = opts.retrieval_k {
        let k = k.clamp(1, 50);
        if let Some(lambda) = opts.mmr_lambda {
            world::select_top_claims_diverse(Some(proj), prompt, k, lambda)
        } else {
            world::select_top_claims(Some(proj), prompt, k)
        }
    } else {
        Vec::new()
    };
    let mut ctx_tokens = 0u64;
    let mut ctx_items = entries.len() as u64;
    if let Some(budget) = opts.context_budget_tokens {
        let cap_each = opts.context_item_budget_tokens.unwrap_or(256) as u64;
        ctx_tokens = pack_items_strict_budget(&mut entries, budget as u64, Some(cap_each));
        ctx_items = entries.len() as u64;
    }
    let rendered = render_context(&entries, opts);
    (rendered, ctx_tokens, ctx_items)
}

fn render_context(items: &[Value], opts: &EvalOptions) -> String {
    let fmt = opts
        .context_format
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "bullets".to_string());
    let include_prov = opts.include_provenance.unwrap_or(false);
    let joiner = opts.joiner.as_deref().unwrap_or("\n");
    match fmt.as_str() {
        "jsonl" => items
            .iter()
            .map(|v| {
                let mut obj = serde_json::Map::new();
                if let Some(id) = v.get("id") {
                    obj.insert("id".into(), id.clone());
                }
                if let Some(text) = v.get("text") {
                    obj.insert("text".into(), text.clone());
                }
                if include_prov {
                    if let Some(props) = v.get("props") {
                        obj.insert("props".into(), props.clone());
                    }
                }
                if let Some(conf) = v.get("confidence") {
                    obj.insert("confidence".into(), conf.clone());
                }
                serde_json::to_string(&Value::Object(obj)).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(joiner),
        "inline" => items
            .iter()
            .map(|v| {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let text = extract_text(v);
                format!("[{}] {}", id, text)
            })
            .collect::<Vec<_>>()
            .join(" · "),
        "custom" => {
            let tpl = opts
                .context_item_template
                .as_deref()
                .unwrap_or("- [{{id}}] {{text}}");
            items
                .iter()
                .map(|v| {
                    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let text = extract_text(v);
                    let conf = v
                        .get("confidence")
                        .and_then(|x| x.as_f64())
                        .map(|c| format!("{:.2}", c))
                        .unwrap_or_default();
                    let mut line = tpl
                        .replace("{{id}}", id)
                        .replace("{{text}}", &text)
                        .replace("{{summary}}", &text)
                        .replace("{{confidence}}", &conf);
                    if include_prov {
                        if let Some(Value::Object(props)) = v.get("props") {
                            if let Some(Value::String(pv)) = props.get("provenance") {
                                line = line.replace("{{provenance}}", pv);
                            }
                        }
                    }
                    line
                })
                .collect::<Vec<_>>()
                .join(joiner)
        }
        _ => items
            .iter()
            .map(|v| {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let text = extract_text(v);
                format!("- [{}] {}", id, text)
            })
            .collect::<Vec<_>>()
            .join(joiner),
    }
}

fn extract_text(v: &Value) -> String {
    if let Some(txt) = v.get("text").and_then(|x| x.as_str()) {
        txt.to_string()
    } else if let Some(summary) = v
        .get("props")
        .and_then(|p| p.get("summary"))
        .and_then(|s| s.as_str())
    {
        summary.to_string()
    } else {
        String::new()
    }
}

fn pack_items_strict_budget(
    items: &mut Vec<Value>,
    budget_tokens: u64,
    cap_each: Option<u64>,
) -> u64 {
    if budget_tokens == 0 || items.is_empty() {
        return 0;
    }
    let mut toks: Vec<u64> = items.iter().map(estimate_tokens_item).collect();
    let mut total: i64 = toks.iter().sum::<u64>() as i64;
    let budget = budget_tokens as i64;
    if total <= budget {
        return total as u64;
    }
    let cap_each = cap_each.unwrap_or_else(|| (budget_tokens / items.len() as u64).max(24));
    for (idx, it) in items.iter_mut().enumerate() {
        if toks[idx] > cap_each {
            trim_item_to_tokens(it, cap_each);
            toks[idx] = estimate_tokens_item(it);
        }
    }
    total = toks.iter().sum::<u64>() as i64;
    if total <= budget {
        return total as u64;
    }
    while total > budget && !items.is_empty() {
        items.pop();
        toks.pop();
        total = toks.iter().sum::<u64>() as i64;
    }
    total.max(0) as u64
}

fn trim_item_to_tokens(v: &mut Value, cap_tokens: u64) {
    if let Some(obj) = v.as_object_mut() {
        if let Some(Value::String(text)) = obj.get_mut("text") {
            trim_value_string_to_tokens(text, cap_tokens);
        }
        if let Some(Value::String(trace)) = obj.get_mut("trace") {
            trim_value_string_to_tokens(trace, cap_tokens / 2);
        }
        if let Some(Value::Object(props)) = obj.get_mut("props") {
            for key in ["summary", "note", "details", "desc"] {
                if let Some(Value::String(val)) = props.get_mut(key) {
                    trim_value_string_to_tokens(val, cap_tokens / 4);
                }
            }
        }
    }
}

fn trim_value_string_to_tokens(s: &mut String, cap_tokens: u64) {
    let max_chars = (cap_tokens * 4).max(24) as usize;
    if s.len() > max_chars {
        let mut trimmed = s.chars().take(max_chars).collect::<String>();
        trimmed.push('…');
        *s = trimmed;
    }
}

fn estimate_tokens_item(v: &Value) -> u64 {
    let mut total = 6u64;
    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
        total += estimate_tokens_str(id);
    }
    if let Some(text) = v.get("text").and_then(|x| x.as_str()) {
        total += estimate_tokens_str(text);
    }
    if let Some(trace) = v.get("trace").and_then(|x| x.as_str()) {
        total += estimate_tokens_str(trace);
    }
    if let Some(props) = v.get("props").and_then(|x| x.as_object()) {
        for value in props.values() {
            if let Some(s) = value.as_str() {
                total += estimate_tokens_str(s);
            }
        }
    }
    total
}

fn estimate_tokens_str(s: &str) -> u64 {
    (s.len() as u64).div_ceil(4)
}

async fn best_of_k(prompt: &str, vote_k: usize, temperature: Option<f64>) -> Option<String> {
    let mut best: Option<String> = None;
    let mut best_votes = 0;
    for _ in 0..vote_k {
        let reply = if let Some(r) = llama_reply(prompt, temperature).await {
            r
        } else if let Some(r) = openai_reply(prompt, temperature).await {
            r
        } else {
            synth_reply(prompt)
        };
        let votes = reply.split_whitespace().count();
        if votes > best_votes {
            best_votes = votes;
            best = Some(reply);
        }
    }
    best
}

fn synth_reply(msg: &str) -> String {
    format!("You said: {}", msg)
}

async fn llama_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let base = std::env::var("ARW_LLAMA_URL").ok()?;
    if base.trim().is_empty() {
        return None;
    }
    let url = format!("{}/completion", base.trim_end_matches('/'));
    let timeout = http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .ok()?;
    let mut body = json!({
        "prompt": prompt,
        "n_predict": 128,
        "cache_prompt": true
    });
    if let Some(t) = temperature {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("temperature".into(), json!(t));
        }
    }
    match client.post(url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok().and_then(|v| {
            v.get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
        }),
        _ => None,
    }
}

async fn openai_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let key = std::env::var("ARW_OPENAI_API_KEY").ok()?;
    if key.trim().is_empty() {
        return None;
    }
    let url = "https://api.openai.com/v1/chat/completions";
    let timeout = http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .ok()?;
    let mut body = json!({
        "model": std::env::var("ARW_OPENAI_MODEL").ok().unwrap_or_else(|| "gpt-4o-mini".into()),
        "messages": [{"role":"user", "content": prompt}]
    });
    if let Some(t) = temperature {
        if let Some(obj) = body.as_object_mut() {
            obj.insert("temperature".into(), json!(t));
        }
    }
    match client.post(url).bearer_auth(key).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok().and_then(|v| {
            v.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c0| c0.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        }),
        _ => None,
    }
}

fn http_timeout_secs() -> u64 {
    http_timeout::get_secs()
}

fn score_text(expect: &Value, answer: &str) -> (bool, Option<String>) {
    if let Some(sub) = expect.get("contains").and_then(|x| x.as_str()) {
        return (answer.contains(sub), Some(format!("contains('{}')", sub)));
    }
    if let Some(eq) = expect.get("equals").and_then(|x| x.as_str()) {
        return (answer.trim() == eq.trim(), Some("equals".into()));
    }
    if let Some(regex) = expect.get("regex").and_then(|x| x.as_str()) {
        if let Ok(re) = Regex::new(regex) {
            return (re.is_match(answer), Some(format!("regex('{}')", regex)));
        }
    }
    (false, Some("no_expectation".into()))
}

async fn save_json_atomically(path: &std::path::Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return Err(e.to_string());
        }
    }
    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    match tokio::fs::rename(&tmp, path).await {
        Ok(_) => Ok(()),
        Err(_) => {
            tokio::fs::remove_file(path).await.ok();
            match tokio::fs::rename(&tmp, path).await {
                Ok(_) => Ok(()),
                Err(e) => {
                    tokio::fs::remove_file(&tmp).await.ok();
                    Err(e.to_string())
                }
            }
        }
    }
}
