use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use crate::{util, working_set, AppState};
use arw_memory_core::MemoryInsertOwned;
use arw_topics as topics;

const VALUE_PREVIEW_MAX_CHARS: usize = 240;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MemoryEmbeddingInput {
    pub vector: Vec<f32>,
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MemoryUpsertInput {
    pub id: Option<String>,
    pub lane: String,
    pub kind: Option<String>,
    pub key: Option<String>,
    pub value: Value,
    pub text: Option<String>,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub durability: Option<String>,
    pub trust: Option<f64>,
    pub privacy: Option<String>,
    pub ttl_s: Option<i64>,
    pub tags: Vec<String>,
    pub keywords: Vec<String>,
    pub embedding: Option<MemoryEmbeddingInput>,
    pub score: Option<f64>,
    pub prob: Option<f64>,
    pub entities: Value,
    pub source: Value,
    pub links: Value,
    pub extra: Value,
    pub dedupe: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryUpsertResult {
    pub id: String,
    pub record: Value,
    pub applied: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MemorySearchInput {
    pub query: Option<String>,
    pub lane: Option<String>,
    pub limit: Option<i64>,
    pub embedding: Option<MemoryEmbeddingInput>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryPackResult {
    pub items: Vec<Value>,
    pub seeds: Vec<Value>,
    pub expanded: Vec<Value>,
    pub summary: Value,
    pub spec: Value,
    pub diagnostics: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MemoryPackInput {
    pub query: Option<String>,
    pub embed: Option<Vec<f32>>,
    pub lanes: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub expand_per_seed: Option<usize>,
    pub diversity_lambda: Option<f32>,
    pub min_score: Option<f32>,
    pub project_id: Option<String>,
    pub lane_bonus: Option<f32>,
    pub scorer: Option<String>,
    pub expand_query: Option<bool>,
    pub expand_query_top_k: Option<usize>,
    pub slot_budgets: Option<BTreeMap<String, usize>>,
    pub include_sources: Option<bool>,
    pub debug: Option<bool>,
}

impl MemoryUpsertInput {
    fn normalize(mut self) -> Self {
        self.lane = self.lane.trim().to_string();
        if self.lane.is_empty() {
            self.lane = "episodic".to_string();
        }
        self.tags = normalize_tags(&self.tags);
        self.keywords = normalize_tags(&self.keywords);
        self.privacy = self
            .privacy
            .and_then(|p| {
                let trimmed = p.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .or_else(|| Some("private".to_string()));
        self.durability = self.durability.map(|d| d.trim().to_string());
        self
    }

    fn derive_text(&self) -> Option<String> {
        if let Some(text) = &self.text {
            if !text.trim().is_empty() {
                return Some(text.trim().to_string());
            }
        }
        self.value
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
    }

    pub fn into_insert_owned(self) -> MemoryInsertOwned {
        let normalized = self.normalize();
        let derived_text = normalized.derive_text();
        let (embedding, embed_hint) = match normalized.embedding {
            Some(MemoryEmbeddingInput { vector, hint }) => (Some(vector), hint),
            None => (None, None),
        };
        let entities = if normalized.entities.is_null() {
            None
        } else {
            Some(normalized.entities)
        };
        let source = if normalized.source.is_null() {
            None
        } else {
            Some(normalized.source)
        };
        let links = if normalized.links.is_null() {
            None
        } else {
            Some(normalized.links)
        };
        let extra = if normalized.extra.is_null() {
            None
        } else {
            Some(normalized.extra)
        };

        let tags = normalized.tags;
        let keywords = normalized.keywords;

        MemoryInsertOwned {
            id: normalized.id,
            lane: normalized.lane,
            kind: normalized.kind,
            key: normalized.key,
            value: normalized.value,
            embed: embedding,
            embed_hint,
            tags: if tags.is_empty() { None } else { Some(tags) },
            score: normalized.score,
            prob: normalized.prob,
            agent_id: normalized.agent_id,
            project_id: normalized.project_id,
            text: derived_text,
            durability: normalized.durability,
            trust: normalized.trust,
            privacy: normalized.privacy,
            ttl_s: normalized.ttl_s,
            keywords: if keywords.is_empty() {
                None
            } else {
                Some(keywords)
            },
            entities,
            source,
            links,
            extra,
            hash: None,
        }
    }
}

pub async fn upsert_memory(
    state: &AppState,
    input: MemoryUpsertInput,
    source: &str,
) -> Result<MemoryUpsertResult> {
    let dedupe = input.dedupe;
    let mut insert_owned = input.into_insert_owned();
    let hash = insert_owned.compute_hash();
    if insert_owned.hash.is_none() {
        insert_owned.hash = Some(hash.clone());
    }

    if dedupe {
        if let Some(existing) = state
            .kernel()
            .find_memory_by_hash_async(hash.clone())
            .await
            .context("lookup existing memory by hash")?
        {
            if let Some(id) = existing.get("id").and_then(|v| v.as_str()) {
                insert_owned.id = Some(id.to_string());
            }
        }
    }

    let id = state
        .kernel()
        .insert_memory_async(insert_owned.clone())
        .await
        .context("insert memory")?;

    let record = state
        .kernel()
        .get_memory_async(id.clone())
        .await
        .context("reload memory")?
        .ok_or_else(|| anyhow!("memory insert returned no record"))?;

    let mut record_event = build_memory_record_event(&record);
    util::attach_memory_ptr(&mut record_event);

    state
        .bus()
        .publish(topics::TOPIC_MEMORY_RECORD_PUT, &record_event);

    let applied_event = build_memory_applied_event(&record_event, source);
    state
        .bus()
        .publish(topics::TOPIC_MEMORY_APPLIED, &applied_event);

    Ok(MemoryUpsertResult {
        id,
        record: record_event,
        applied: applied_event,
    })
}

pub async fn search_memory(state: &AppState, params: MemorySearchInput) -> Result<Vec<Value>> {
    let limit = params.limit.unwrap_or(20).clamp(1, 200);
    let lane = params
        .lane
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let mode = params
        .mode
        .as_deref()
        .unwrap_or("hybrid")
        .to_ascii_lowercase();

    let kernel = state.kernel();
    let mut items = match mode.as_str() {
        "vector" => {
            let embed = params
                .embedding
                .as_ref()
                .map(|emb| emb.vector.clone())
                .unwrap_or_default();
            kernel
                .search_memory_by_embedding_async(embed, lane.clone(), limit)
                .await?
        }
        "lexical" => {
            let query = params
                .query
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_default();
            if query.is_empty() {
                kernel.list_recent_memory_async(lane.clone(), limit).await?
            } else {
                kernel
                    .fts_search_memory_async(query, lane.clone(), limit)
                    .await?
            }
        }
        _ => {
            let embed_vec = params.embedding.as_ref().map(|emb| emb.vector.clone());
            kernel
                .select_memory_hybrid_async(params.query.clone(), embed_vec, lane.clone(), limit)
                .await?
        }
    };

    for item in items.iter_mut() {
        util::attach_memory_ptr(item);
    }

    Ok(items)
}

pub async fn pack_memory(state: &AppState, input: MemoryPackInput) -> Result<MemoryPackResult> {
    let mut spec = working_set::WorkingSetSpec {
        query: input.query.clone(),
        embed: input.embed.clone(),
        lanes: input.lanes.unwrap_or_default(),
        limit: input.limit.unwrap_or(0),
        expand_per_seed: input.expand_per_seed.unwrap_or(0),
        diversity_lambda: input.diversity_lambda.unwrap_or(f32::NAN),
        min_score: input.min_score.unwrap_or(f32::NAN),
        project: input.project_id.clone(),
        lane_bonus: input.lane_bonus.unwrap_or(f32::NAN),
        scorer: input.scorer.clone(),
        expand_query: input.expand_query.unwrap_or(false),
        expand_query_top_k: input.expand_query_top_k.unwrap_or(0),
        slot_budgets: input.slot_budgets.unwrap_or_default(),
    };
    spec.normalize();

    let working = working_set::assemble(state, &spec)?;
    let spec_snapshot = spec.snapshot();

    let mut items = working.items;
    attach_memory_ptrs(&mut items);

    let include_sources = input.include_sources.unwrap_or(false);
    let mut seeds = if include_sources {
        working.seeds
    } else {
        Vec::new()
    };
    if include_sources {
        attach_memory_ptrs(&mut seeds);
    }
    let mut expanded = if include_sources {
        working.expanded
    } else {
        Vec::new()
    };
    if include_sources {
        attach_memory_ptrs(&mut expanded);
    }

    let diagnostics = if input.debug.unwrap_or(false) {
        Some(working.diagnostics)
    } else {
        None
    };

    let summary = working.summary.to_json();

    let event_payload = json!({
        "spec": spec_snapshot.clone(),
        "counts": {
            "items": items.len(),
            "seeds": seeds.len(),
            "expanded": expanded.len()
        },
        "summary": summary.clone()
    });
    state
        .bus()
        .publish(topics::TOPIC_MEMORY_PACK_JOURNALED, &event_payload);

    Ok(MemoryPackResult {
        items,
        seeds,
        expanded,
        summary,
        spec: spec_snapshot,
        diagnostics,
    })
}

pub fn attach_memory_ptrs(items: &mut [Value]) {
    for item in items.iter_mut() {
        util::attach_memory_ptr(item);
    }
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let trimmed = tag.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_string());
        }
    }
    out
}

fn build_memory_record_event(record: &Value) -> Value {
    let mut obj = record.as_object().cloned().unwrap_or_else(Map::new);
    if !obj.contains_key("tags") {
        obj.insert("tags".into(), Value::Array(Vec::new()));
    }
    Value::Object(obj)
}

fn build_memory_applied_event(record: &Value, source: &str) -> Value {
    let mut obj = record.as_object().cloned().unwrap_or_else(Map::new);
    obj.insert("source".into(), json!(source));
    if let Some(value) = obj.get("value").cloned() {
        if let Some((preview, truncated)) = preview_from_value(&value) {
            obj.insert("value_preview".into(), json!(preview));
            obj.insert("value_preview_truncated".into(), json!(truncated));
        }
        if let Ok(bytes) = serde_json::to_vec(&value) {
            obj.insert("value_bytes".into(), json!(bytes.len()));
        }
        obj.insert("value".into(), value);
    }
    if !obj.contains_key("applied_at") {
        if let Some(updated) = obj.get("updated").cloned() {
            obj.insert("applied_at".into(), updated);
        }
    }
    Value::Object(obj)
}

fn preview_from_value(value: &Value) -> Option<(String, bool)> {
    match value {
        Value::String(s) => Some(truncate_chars(s, VALUE_PREVIEW_MAX_CHARS)),
        _ => serde_json::to_string(value)
            .ok()
            .map(|s| truncate_chars(&s, VALUE_PREVIEW_MAX_CHARS)),
    }
}

fn truncate_chars(input: &str, limit: usize) -> (String, bool) {
    let mut out = String::new();
    let mut truncated = false;
    for (idx, ch) in input.chars().enumerate() {
        if idx >= limit {
            truncated = true;
            break;
        }
        out.push(ch);
    }
    if truncated {
        out.push('…');
    }
    (out, truncated)
}
