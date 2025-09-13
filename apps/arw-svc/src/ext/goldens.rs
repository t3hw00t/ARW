use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// Minimal, config-first goldens store and evaluator

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoldenItem {
    pub id: String,
    pub kind: String, // e.g., "chat", "data", "action"
    #[serde(default)]
    pub input: Value, // shape depends on kind
    #[serde(default)]
    pub expect: Value, // {contains|regex|equals: string} etc.
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoldenSet {
    pub items: Vec<GoldenItem>,
}

pub async fn load(proj: &str) -> GoldenSet {
    let path = crate::ext::paths::state_dir()
        .join("goldens")
        .join(format!("{}.json", proj));
    match crate::ext::io::load_json_file_async(&path).await {
        Some(v) => serde_json::from_value::<GoldenSet>(v).unwrap_or_default(),
        None => GoldenSet::default(),
    }
}

pub async fn save(proj: &str, set: &GoldenSet) -> Result<(), String> {
    let dir = crate::ext::paths::state_dir().join("goldens");
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return Err(e.to_string());
    }
    let path = dir.join(format!("{}.json", proj));
    crate::ext::io::save_json_file_async(&path, &serde_json::to_value(set).unwrap())
        .await
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvalOptions {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub vote_k: Option<usize>, // simple self-consistency
    // Retrieval/selection knobs (optional)
    #[serde(default)]
    pub retrieval_k: Option<usize>,
    #[serde(default)]
    pub mmr_lambda: Option<f64>,
    #[serde(default)]
    pub compression_aggr: Option<f64>,
    // Strict budget knobs
    #[serde(default)]
    pub context_budget_tokens: Option<usize>,
    #[serde(default)]
    pub context_item_budget_tokens: Option<usize>,
    // Context formatting
    #[serde(default)]
    pub context_format: Option<String>, // bullets|jsonl|inline|custom
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalResultItem {
    pub id: String,
    pub ok: bool,
    pub latency_ms: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

fn synth_reply(msg: &str) -> String {
    format!("You said: {}", msg)
}

async fn llama_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let base = std::env::var("ARW_LLAMA_URL").ok()?; // e.g., http://127.0.0.1:8080
    if base.trim().is_empty() {
        return None;
    }
    let url = format!("{}/completion", base.trim_end_matches('/'));
    let timeout_s: u64 = crate::dyn_timeout::current_http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_s))
        .build()
        .ok()?;
    let mut body = json!({
        "prompt": prompt,
        "n_predict": 128,
        "cache_prompt": true
    });
    if let Some(t) = temperature {
        if let Some(o) = body.as_object_mut() {
            o.insert("temperature".into(), json!(t));
        }
    }
    match client.post(url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(v) => v
                .get("content")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            Err(_) => None,
        },
        _ => None,
    }
}

async fn openai_reply(prompt: &str, temperature: Option<f64>) -> Option<String> {
    let key = std::env::var("ARW_OPENAI_API_KEY").ok()?;
    if key.trim().is_empty() {
        return None;
    }
    let url = "https://api.openai.com/v1/chat/completions";
    let timeout_s: u64 = crate::dyn_timeout::current_http_timeout_secs();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_s))
        .build()
        .ok()?;
    let mut body = json!({
        "model": std::env::var("ARW_OPENAI_MODEL").ok().unwrap_or_else(|| "gpt-4o-mini".into()),
        "messages": [ {"role":"user", "content": prompt} ]
    });
    if let Some(t) = temperature {
        if let Some(o) = body.as_object_mut() {
            o.insert("temperature".into(), json!(t));
        }
    }
    match client.post(url).bearer_auth(key).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(v) => v
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c0| c0.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            Err(_) => None,
        },
        _ => None,
    }
}

fn estimate_tokens_str(s: &str) -> u64 {
    (s.len() as u64).div_ceil(4)
}

fn estimate_tokens_item(v: &Value) -> u64 {
    let mut t = 6;
    if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
        t += estimate_tokens_str(id);
    }
    if let Some(s) = v.get("text").and_then(|x| x.as_str()) {
        t += estimate_tokens_str(s);
    }
    if let Some(s) = v.get("trace").and_then(|x| x.as_str()) {
        t += estimate_tokens_str(s);
    }
    if let Some(props) = v.get("props").and_then(|x| x.as_object()) {
        for (_k, vv) in props.iter() {
            if let Some(s) = vv.as_str() {
                t += estimate_tokens_str(s);
            }
        }
    }
    t
}

fn trim_value_string_to_tokens(val: &mut String, max_tokens: u64) {
    let max_chars = (max_tokens * 4).max(24) as usize;
    if val.len() > max_chars {
        let mut t = val.chars().take(max_chars).collect::<String>();
        t.push('…');
        *val = t;
    }
}

fn trim_item_to_tokens(v: &mut Value, cap_tokens: u64) {
    if let Some(o) = v.as_object_mut() {
        if let Some(Value::String(s)) = o.get_mut("text") {
            trim_value_string_to_tokens(s, cap_tokens);
        }
        if let Some(Value::String(s)) = o.get_mut("trace") {
            trim_value_string_to_tokens(s, cap_tokens / 2);
        }
        if let Some(Value::Object(props)) = o.get_mut("props") {
            for k in ["summary", "note", "details", "desc"] {
                if let Some(Value::String(s)) = props.get_mut(k) {
                    trim_value_string_to_tokens(s, cap_tokens / 4);
                }
            }
        }
    }
}

fn pack_items_strict_budget(
    items: &mut Vec<Value>,
    budget_tokens: u64,
    per_item_cap: Option<u64>,
) -> u64 {
    if budget_tokens == 0 || items.is_empty() {
        return 0;
    }
    // Initial estimate
    let mut toks: Vec<u64> = items.iter().map(estimate_tokens_item).collect();
    let mut total: i64 = toks.iter().sum::<u64>() as i64;
    let budget = budget_tokens as i64;
    if total <= budget {
        return total as u64;
    }
    // Step 1: trim to per-item caps
    let cap_each = per_item_cap.unwrap_or_else(|| (budget_tokens / (items.len() as u64)).max(24));
    for (i, it) in items.iter_mut().enumerate() {
        if toks[i] > cap_each {
            trim_item_to_tokens(it, cap_each);
            toks[i] = estimate_tokens_item(it);
        }
    }
    total = toks.iter().sum::<u64>() as i64;
    if total <= budget {
        return total as u64;
    }
    // Step 2: drop from end (lowest utility proxy)
    while total > budget && !items.is_empty() {
        items.pop();
        toks.pop();
        total = toks.iter().sum::<u64>() as i64;
    }
    total.max(0) as u64
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
        "jsonl" => {
            let lines: Vec<String> = items
                .iter()
                .map(|v| {
                    let mut o = serde_json::Map::new();
                    if let Some(id) = v.get("id") {
                        o.insert("id".into(), id.clone());
                    }
                    if let Some(t) = v.get("text") {
                        o.insert("text".into(), t.clone());
                    }
                    if let Some(props) = v.get("props") {
                        if include_prov {
                            o.insert("props".into(), props.clone());
                        }
                    }
                    if let Some(c) = v.get("confidence") {
                        o.insert("confidence".into(), c.clone());
                    }
                    serde_json::to_string(&Value::Object(o)).unwrap_or_default()
                })
                .collect();
            lines.join(joiner)
        }
        "inline" => {
            let parts: Vec<String> = items
                .iter()
                .map(|v| {
                    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let txt = v
                        .get("text")
                        .and_then(|x| x.as_str())
                        .or_else(|| {
                            v.get("props")
                                .and_then(|p| p.get("summary"))
                                .and_then(|x| x.as_str())
                        })
                        .unwrap_or("");
                    format!("[{}] {}", id, txt)
                })
                .collect();
            parts.join(" · ")
        }
        "custom" => {
            let tpl = opts
                .context_item_template
                .as_deref()
                .unwrap_or("- [{{id}}] {{text}}");
            let mut out: Vec<String> = Vec::with_capacity(items.len());
            for v in items.iter() {
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let txt = v
                    .get("text")
                    .and_then(|x| x.as_str())
                    .or_else(|| {
                        v.get("props")
                            .and_then(|p| p.get("summary"))
                            .and_then(|x| x.as_str())
                    })
                    .unwrap_or("");
                let conf = v
                    .get("confidence")
                    .and_then(|x| x.as_f64())
                    .map(|c| format!("{:.2}", c))
                    .unwrap_or_default();
                let mut line = tpl
                    .replace("{{id}}", id)
                    .replace("{{text}}", txt)
                    .replace("{{summary}}", txt)
                    .replace("{{confidence}}", &conf);
                if include_prov {
                    if let Some(props) = v.get("props").and_then(|p| p.as_object()) {
                        if let Some(Value::String(pv)) = props.get("provenance").cloned() {
                            line = line.replace("{{provenance}}", &pv);
                        }
                    }
                }
                out.push(line);
            }
            out.join(joiner)
        }
        _ => {
            // bullets (default)
            let lines: Vec<String> = items
                .iter()
                .map(|v| {
                    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let txt = v
                        .get("text")
                        .and_then(|x| x.as_str())
                        .or_else(|| {
                            v.get("props")
                                .and_then(|p| p.get("summary"))
                                .and_then(|x| x.as_str())
                        })
                        .unwrap_or("");
                    format!("- [{}] {}", id, txt)
                })
                .collect();
            lines.join(joiner)
        }
    }
}

fn score_text(expect: &Value, answer: &str) -> (bool, Option<String>) {
    if let Some(s) = expect.get("contains").and_then(|x| x.as_str()) {
        return (answer.contains(s), Some(format!("contains('{}')", s)));
    }
    if let Some(s) = expect.get("equals").and_then(|x| x.as_str()) {
        return (answer.trim() == s.trim(), Some("equals".into()));
    }
    if let Some(r) = expect.get("regex").and_then(|x| x.as_str()) {
        if let Ok(re) = regex::Regex::new(r) {
            return (re.is_match(answer), Some(format!("regex('{}')", r)));
        }
    }
    (false, Some("no_expectation".into()))
}

pub async fn evaluate_chat_items(
    set: &GoldenSet,
    opts: &EvalOptions,
    proj: Option<&str>,
) -> EvalSummary {
    let mut items: Vec<EvalResultItem> = Vec::new();
    let mut passed = 0usize;
    let mut total_latency: u64 = 0;
    let limit = opts.limit.unwrap_or(usize::MAX);
    let vote_k = opts.vote_k.unwrap_or(1).clamp(1, 9);
    let mut ctx_tokens_acc: u64 = 0;
    let mut ctx_items_acc: u64 = 0;
    let mut ctx_count: u64 = 0;
    for it in set.items.iter().filter(|i| i.kind == "chat").take(limit) {
        let prompt = it
            .input
            .get("prompt")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        // Optional retrieval: assemble small context from world claims
        let full_prompt = if let Some(k) = opts.retrieval_k {
            let kk = k.clamp(1, 50);
            let mut items: Vec<serde_json::Value> = if let Some(lambda) = opts.mmr_lambda {
                crate::ext::world::select_top_claims_diverse(proj, prompt, kk, lambda).await
            } else {
                crate::ext::world::select_top_claims(proj, prompt, kk).await
            };
            // Optional compression of context
            if let Some(aggr) = opts.compression_aggr {
                let a = aggr.clamp(0.0, 1.0);
                if a > 0.0 {
                    fn trim_map(m: &mut serde_json::Map<String, Value>, key: &str, a: f64) {
                        if let Some(Value::String(s)) = m.get_mut(key) {
                            let len = s.len();
                            if len > 0 {
                                let keep = ((len as f64) * (1.0 - 0.6 * a)).max(24.0) as usize;
                                if len > keep {
                                    let mut t = s.chars().take(keep).collect::<String>();
                                    t.push('…');
                                    *s = t;
                                }
                            }
                        }
                    }
                    for v in items.iter_mut() {
                        if let Some(o) = v.as_object_mut() {
                            trim_map(o, "text", a);
                            trim_map(o, "trace", a);
                            if let Some(Value::Object(props)) = o.get_mut("props") {
                                for k in ["note", "summary", "details", "desc"] {
                                    trim_map(props, k, a);
                                }
                            }
                        }
                    }
                }
            }
            // Strict budget: pack items by token budget if provided
            if let Some(b) = opts.context_budget_tokens.filter(|v| *v > 0) {
                let per = opts.context_item_budget_tokens.map(|x| x as u64);
                let used = pack_items_strict_budget(&mut items, b as u64, per);
                let _ = used; // used for avg below
            }
            // accumulate simple averages for context size
            let used_now: u64 = items.iter().map(estimate_tokens_item).sum();
            ctx_tokens_acc = ctx_tokens_acc.saturating_add(used_now);
            ctx_items_acc = ctx_items_acc.saturating_add(items.len() as u64);
            ctx_count = ctx_count.saturating_add(1);
            if items.is_empty() {
                prompt.to_string()
            } else {
                let ctx = render_context(&items, opts);
                let header = opts
                    .context_header
                    .clone()
                    .unwrap_or_else(|| "You may use the following context.".to_string());
                let footer = opts.context_footer.clone().unwrap_or_default();
                if footer.is_empty() {
                    format!("{}\n{}\n\nQuestion: {}\nAnswer:", header, ctx, prompt)
                } else {
                    format!(
                        "{}\n{}\n{}\n\nQuestion: {}\nAnswer:",
                        header, ctx, footer, prompt
                    )
                }
            }
        } else {
            prompt.to_string()
        };
        let t0 = std::time::Instant::now();
        // Simple self-consistency: take majority vote of best-effort replies
        let mut votes: Vec<String> = Vec::with_capacity(vote_k);
        for _ in 0..vote_k {
            if let Some(ans) = llama_reply(&full_prompt, opts.temperature).await {
                votes.push(ans);
            } else if let Some(ans) = openai_reply(&full_prompt, opts.temperature).await {
                votes.push(ans);
            } else {
                votes.push(synth_reply(&full_prompt));
            }
        }
        // Tally
        use std::collections::HashMap;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for v in &votes {
            *counts.entry(v.trim().to_string()).or_insert(0) += 1;
        }
        let best = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1))
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| votes[0].clone());
        let dt = t0.elapsed().as_millis() as u64;
        let (ok, note) = score_text(&it.expect, &best);
        if ok {
            passed += 1;
        }
        total_latency = total_latency.saturating_add(dt);
        items.push(EvalResultItem {
            id: it.id.clone(),
            ok,
            latency_ms: dt,
            note,
        });
    }
    let total = items.len();
    let failed = total.saturating_sub(passed);
    let avg_latency_ms = if total > 0 {
        total_latency / (total as u64)
    } else {
        0
    };
    let (avg_ctx_tokens, avg_ctx_items) = if ctx_count > 0 {
        (ctx_tokens_acc / ctx_count, ctx_items_acc / ctx_count)
    } else {
        (0, 0)
    };
    EvalSummary {
        total,
        passed,
        failed,
        avg_latency_ms,
        items,
        avg_ctx_tokens,
        avg_ctx_items,
    }
}
