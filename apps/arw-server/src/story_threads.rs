use crate::{memory_service, AppState};
use anyhow::{Context, Result};
use arw_memory_core::MemoryInsertOwned;
use arw_topics as topics;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rustc_hash::FxHashSet;
use serde_json::{json, Map, Value};
use std::cmp::Ordering;

pub const STORY_THREAD_LANE: &str = "story_thread";
const STORY_THREAD_KIND: &str = "story.thread";
const STORY_THREAD_SLOT: &str = "story_thread";
const REL_THREAD_MEMBER: &str = "thread.member";
const REL_THREAD_PARENT: &str = "thread.parent";
const MAX_THREAD_MEMBERS: usize = 16;
const DEFAULT_TOPIC_WEIGHT: f32 = 0.7;
const MIN_WEIGHT: f32 = 0.05;
const MAX_WEIGHT: f32 = 1.0;
const MAX_TOPICS_PER_MEMBER: usize = 6;

#[derive(Clone, Copy, Debug)]
pub enum StoryThreadSource {
    Manual,
    Cascade,
}

impl StoryThreadSource {
    pub fn as_str(self) -> &'static str {
        match self {
            StoryThreadSource::Manual => "manual",
            StoryThreadSource::Cascade => "cascade",
        }
    }

    fn fallback_relation(self) -> Option<String> {
        match self {
            StoryThreadSource::Manual => None,
            StoryThreadSource::Cascade => Some("thread.episode".to_string()),
        }
    }
}

pub async fn attach_topics_to_record(
    state: &AppState,
    record: &Value,
    topics: &[memory_service::MemoryTopicHint],
    source: StoryThreadSource,
    meta: Option<Value>,
) -> Result<()> {
    if topics.is_empty() {
        return Ok(());
    }
    let member = match ThreadMember::from_record(record) {
        Some(member) => member,
        None => return Ok(()),
    };
    let fallback_relation = source.fallback_relation();
    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut prepared: Vec<ThreadTopic> = Vec::new();
    for hint in topics {
        if let Some(topic) = ThreadTopic::from_hint(hint, fallback_relation.clone()) {
            if seen.insert(topic.slug.clone()) {
                prepared.push(topic);
            }
        }
    }
    if prepared.is_empty() {
        return Ok(());
    }
    let meta_value = meta.unwrap_or(Value::Null);
    for topic in prepared {
        let meta_clone = if meta_value.is_null() {
            None
        } else {
            Some(meta_value.clone())
        };
        upsert_thread(state, &topic, &member, source, meta_clone).await?;
    }
    Ok(())
}

pub fn derive_topics_from_summary(value: &Value) -> Vec<memory_service::MemoryTopicHint> {
    let mut hints: Vec<memory_service::MemoryTopicHint> = Vec::new();

    if let Some(primary) = value
        .get("abstract")
        .and_then(|v| v.get("primary_kinds"))
        .and_then(Value::as_array)
    {
        for kind in primary.iter().filter_map(Value::as_str) {
            hints.push(memory_service::MemoryTopicHint {
                name: kind.to_string(),
                weight: Some(0.85),
                relation: Some("thread.kind".to_string()),
            });
        }
    }

    if let Some(projects) = value.get("projects").and_then(Value::as_array) {
        for project in projects.iter().filter_map(Value::as_str) {
            hints.push(memory_service::MemoryTopicHint {
                name: project.to_string(),
                weight: Some(0.8),
                relation: Some("thread.project".to_string()),
            });
        }
    }

    if let Some(actors) = value.get("actors").and_then(Value::as_array) {
        for actor in actors.iter().filter_map(Value::as_str) {
            hints.push(memory_service::MemoryTopicHint {
                name: actor.to_string(),
                weight: Some(0.65),
                relation: Some("thread.actor".to_string()),
            });
        }
    }

    if let Some(tags) = value.get("tags").and_then(Value::as_array) {
        for tag in tags.iter().filter_map(Value::as_str) {
            if let Some(rest) = tag.strip_prefix("topic:") {
                hints.push(memory_service::MemoryTopicHint {
                    name: rest.replace('-', " "),
                    weight: Some(0.75),
                    relation: Some("thread.tag".to_string()),
                });
            }
        }
    }

    if hints.is_empty() {
        if let Some(text) = value
            .get("abstract")
            .and_then(|v| v.get("text"))
            .and_then(Value::as_str)
            .and_then(fallback_topic_from_text)
        {
            hints.push(memory_service::MemoryTopicHint {
                name: text,
                weight: Some(0.6),
                relation: None,
            });
        }
    }

    let mut normalized = memory_service::normalize_topic_hints(&hints);
    normalized.sort_by(|a, b| {
        weight_value(b)
            .partial_cmp(&weight_value(a))
            .unwrap_or(Ordering::Equal)
    });
    if normalized.len() > MAX_TOPICS_PER_MEMBER {
        normalized.truncate(MAX_TOPICS_PER_MEMBER);
    }
    normalized
}

struct ThreadTopic {
    label: String,
    slug: String,
    weight: f32,
    relation: Option<String>,
}

impl ThreadTopic {
    fn from_hint(
        hint: &memory_service::MemoryTopicHint,
        fallback_relation: Option<String>,
    ) -> Option<Self> {
        let slug = memory_service::slugify_topic(&hint.name)?;
        let relation = hint
            .relation
            .clone()
            .filter(|rel| !rel.is_empty())
            .or_else(|| fallback_relation.clone());
        Some(ThreadTopic {
            label: hint.name.clone(),
            slug,
            weight: hint.weight.unwrap_or(DEFAULT_TOPIC_WEIGHT),
            relation,
        })
    }

    fn relation_label(&self) -> &str {
        self.relation.as_deref().unwrap_or(REL_THREAD_MEMBER)
    }
}

struct ThreadMember {
    id: String,
    lane: String,
    kind: Option<String>,
    text: String,
    pointer: Option<Value>,
    score: Option<f64>,
    updated_iso: Option<String>,
    updated_dt: Option<DateTime<Utc>>,
    projects: Vec<String>,
}

impl ThreadMember {
    fn from_record(record: &Value) -> Option<Self> {
        let id = record.get("id")?.as_str()?.to_string();
        let lane = record
            .get("lane")
            .and_then(Value::as_str)
            .unwrap_or("episodic")
            .to_string();
        let kind = record
            .get("kind")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let text = pick_primary_text(record);
        let pointer = record.get("ptr").cloned();
        let score = record
            .get("score")
            .and_then(Value::as_f64)
            .or_else(|| record.get("prob").and_then(Value::as_f64));
        let updated_iso = record
            .get("updated")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        let updated_dt = updated_iso.as_deref().and_then(parse_time);
        let projects = extract_projects(record);
        Some(ThreadMember {
            id,
            lane,
            kind,
            text,
            pointer,
            score,
            updated_iso,
            updated_dt,
            projects,
        })
    }
}

async fn upsert_thread(
    state: &AppState,
    topic: &ThreadTopic,
    member: &ThreadMember,
    source: StoryThreadSource,
    meta: Option<Value>,
) -> Result<()> {
    let now = Utc::now();
    let now_iso = now.to_rfc3339_opts(SecondsFormat::Millis, true);
    let member_weight = compute_member_weight(topic.weight, member.score, member.updated_dt, now);

    let mut member_entry = Map::new();
    member_entry.insert("id".into(), json!(member.id));
    member_entry.insert("lane".into(), json!(member.lane));
    if let Some(kind) = member.kind.as_ref() {
        member_entry.insert("kind".into(), json!(kind));
    }
    member_entry.insert(
        "text".into(),
        Value::String(truncate_text(&member.text, 320)),
    );
    member_entry.insert("weight".into(), json!(member_weight));
    if let Some(score) = member.score {
        member_entry.insert("score".into(), json!(score));
    }
    if let Some(updated) = member.updated_iso.as_ref() {
        member_entry.insert("updated".into(), json!(updated));
    } else {
        member_entry.insert("updated".into(), json!(now_iso.clone()));
    }
    member_entry.insert("ingested_at".into(), json!(now_iso.clone()));
    member_entry.insert("source".into(), json!(source.as_str()));
    member_entry.insert("relation".into(), json!(topic.relation_label()));
    member_entry.insert("projects".into(), json!(member.projects.clone()));
    if let Some(ptr) = member.pointer.as_ref() {
        member_entry.insert("ptr".into(), ptr.clone());
    }
    if let Some(meta) = meta {
        member_entry.insert("meta".into(), meta);
    }

    let thread_id = format!("thread:{}", topic.slug);
    let existing = state
        .kernel()
        .get_memory_async(thread_id.clone())
        .await
        .context("load existing story thread")?;

    let mut members: Vec<Value> = existing
        .as_ref()
        .and_then(|record| {
            record
                .get("value")
                .and_then(|v| v.get("members"))
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_else(Vec::new);
    let member_id = member.id.as_str();
    members.retain(|entry| {
        entry
            .get("id")
            .and_then(Value::as_str)
            .map(|id| id != member_id)
            .unwrap_or(true)
    });
    members.insert(0, Value::Object(member_entry));
    if members.len() > MAX_THREAD_MEMBERS {
        members.truncate(MAX_THREAD_MEMBERS);
    }

    let mut project_set: FxHashSet<String> = existing
        .as_ref()
        .and_then(|record| {
            record
                .get("value")
                .and_then(|v| v.get("projects"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect::<FxHashSet<_>>()
                })
        })
        .unwrap_or_else(FxHashSet::default);
    for project in &member.projects {
        project_set.insert(project.clone());
    }
    let mut project_list: Vec<String> = project_set.iter().cloned().collect();
    project_list.sort();

    let mut extra_map: Map<String, Value> = existing
        .as_ref()
        .and_then(|record| record.get("extra"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new);

    let mut source_counts: Map<String, Value> = extra_map
        .get("source_counts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new);
    let source_key = source.as_str().to_string();
    let count = source_counts
        .get(&source_key)
        .and_then(Value::as_u64)
        .unwrap_or(0);
    source_counts.insert(source_key.clone(), json!(count + 1));
    extra_map.insert("source_counts".into(), Value::Object(source_counts));
    extra_map.insert("topic_label".into(), json!(topic.label));
    extra_map.insert("topic_key".into(), json!(topic.slug));
    extra_map.insert(
        "latest_member".into(),
        json!({
            "id": member.id,
            "weight": member_weight,
            "source": source.as_str(),
            "updated": member
                .updated_iso
                .clone()
                .unwrap_or_else(|| now_iso.clone()),
        }),
    );
    extra_map.insert("version".into(), json!(1));

    let mut tags: Vec<String> = existing
        .as_ref()
        .and_then(|record| record.get("tags").and_then(Value::as_array))
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(Vec::new);
    tags.push("thread".into());
    tags.push("story-thread".into());
    tags.push(format!("topic:{}", topic.slug));
    for project in &project_list {
        tags.push(format!("project:{project}"));
    }
    dedupe_strings(&mut tags);

    let mut keywords: Vec<String> = existing
        .as_ref()
        .and_then(|record| record.get("keywords").and_then(Value::as_array))
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(Vec::new);
    keywords.push(topic.label.clone());
    for part in topic.label.split_whitespace() {
        if !part.is_empty() {
            keywords.push(part.to_string());
        }
    }
    dedupe_strings(&mut keywords);

    let mut total_weight = 0.0_f32;
    let mut max_weight = MIN_WEIGHT;
    let mut min_weight = MAX_WEIGHT;
    for entry in &members {
        let weight = entry
            .get("weight")
            .and_then(Value::as_f64)
            .map(|w| w as f32)
            .unwrap_or(DEFAULT_TOPIC_WEIGHT);
        total_weight += weight;
        if weight > max_weight {
            max_weight = weight;
        }
        if weight < min_weight {
            min_weight = weight;
        }
    }
    if members.is_empty() {
        total_weight = member_weight;
        max_weight = member_weight;
        min_weight = member_weight;
    }
    let avg_weight = if members.is_empty() {
        member_weight
    } else {
        total_weight / (members.len() as f32)
    };

    let latest_preview = members
        .first()
        .and_then(|entry| entry.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary_text = truncate_text(
        &format_thread_summary(topic, members.len(), latest_preview, &project_list),
        360,
    );

    let mut value_obj = Map::new();
    value_obj.insert("slot".into(), json!(STORY_THREAD_SLOT));
    value_obj.insert("topic".into(), json!(topic.label));
    value_obj.insert("topic_key".into(), json!(topic.slug));
    value_obj.insert("members".into(), Value::Array(members.clone()));
    value_obj.insert(
        "projects".into(),
        Value::Array(project_list.iter().map(|p| json!(p)).collect()),
    );
    value_obj.insert(
        "weights".into(),
        json!({
            "latest": member_weight,
            "average": avg_weight,
            "max": max_weight,
            "total": total_weight,
        }),
    );
    value_obj.insert(
        "stats".into(),
        json!({
            "members": members.len(),
            "sources": source_key,
            "min_weight": min_weight,
        }),
    );
    value_obj.insert(
        "summary".into(),
        json!({
            "latest_member": member.id,
            "latest_text": truncate_text(latest_preview, 240),
            "latest_weight": member_weight,
            "updated": member
                .updated_iso
                .clone()
                .unwrap_or_else(|| now_iso.clone()),
        }),
    );
    value_obj.insert("updated".into(), json!(now_iso.clone()));

    let existing_agent = existing
        .as_ref()
        .and_then(|record| record.get("agent_id"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let existing_privacy = existing
        .as_ref()
        .and_then(|record| record.get("privacy"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let existing_durability = existing
        .as_ref()
        .and_then(|record| record.get("durability"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let existing_project_id = existing
        .as_ref()
        .and_then(|record| record.get("project_id"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());
    let existing_embed = existing
        .as_ref()
        .and_then(|record| record.get("embed"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_f64)
                .map(|f| f as f32)
                .collect::<Vec<f32>>()
        });
    let existing_embed_hint = existing
        .as_ref()
        .and_then(|record| record.get("embed_hint"))
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    let project_id = if project_list.len() == 1 {
        Some(project_list[0].clone())
    } else {
        existing_project_id
    };

    let privacy = if project_list.len() == 1 {
        "project".to_string()
    } else if project_list.is_empty() {
        existing_privacy.unwrap_or_else(|| "private".to_string())
    } else {
        "internal".to_string()
    };

    let durability = existing_durability.unwrap_or_else(|| "long".to_string());

    let text = summary_text;

    let insert_owned = MemoryInsertOwned {
        id: Some(thread_id.clone()),
        lane: STORY_THREAD_LANE.to_string(),
        kind: Some(STORY_THREAD_KIND.to_string()),
        key: Some(format!("thread:{}", topic.slug)),
        value: Value::Object(value_obj),
        embed: existing_embed,
        embed_hint: existing_embed_hint,
        tags: if tags.is_empty() { None } else { Some(tags) },
        score: Some(avg_weight as f64),
        prob: Some(avg_weight as f64),
        agent_id: existing_agent,
        project_id,
        persona_id: None,
        text: Some(text),
        durability: Some(durability),
        trust: Some(avg_weight as f64),
        privacy: Some(privacy),
        ttl_s: None,
        keywords: if keywords.is_empty() {
            None
        } else {
            Some(keywords)
        },
        entities: None,
        source: Some(json!({
            "kind": "story_thread",
            "source": source.as_str(),
        })),
        links: None,
        extra: Some(Value::Object(extra_map)),
        hash: None,
    };

    state
        .kernel()
        .insert_memory_with_record_async(insert_owned)
        .await
        .context("upsert story thread record")?;

    state.bus().publish(
        topics::TOPIC_STORY_THREAD_UPDATED,
        &json!({
            "thread_id": thread_id,
            "topic": topic.label,
            "topic_key": topic.slug,
            "member_id": member.id,
            "members": members.len(),
            "weight": member_weight,
            "source": source.as_str(),
        }),
    );

    state
        .kernel()
        .insert_memory_link_async(
            thread_id.clone(),
            member.id.clone(),
            Some(topic.relation_label().to_string()),
            Some(member_weight as f64),
        )
        .await
        .context("insert thread->member link")?;
    state
        .kernel()
        .insert_memory_link_async(
            member.id.clone(),
            thread_id,
            Some(REL_THREAD_PARENT.to_string()),
            Some(member_weight as f64),
        )
        .await
        .context("insert member->thread link")?;

    Ok(())
}

fn compute_member_weight(
    base: f32,
    score: Option<f64>,
    updated: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> f32 {
    let score_component = score.unwrap_or(0.6).clamp(0.0, 1.0) as f32;
    let recency_component = updated.map(|dt| recency_factor(now - dt)).unwrap_or(0.75);
    let weight = 0.5 * base + 0.3 * score_component + 0.2 * recency_component;
    weight.clamp(MIN_WEIGHT, MAX_WEIGHT)
}

fn recency_factor(delta: Duration) -> f32 {
    let hours = delta.num_hours();
    if hours <= 0 {
        1.0
    } else if hours <= 6 {
        0.95
    } else if hours <= 24 {
        0.9
    } else if hours <= 72 {
        0.8
    } else if hours <= 168 {
        0.7
    } else if hours <= 720 {
        0.55
    } else {
        0.45
    }
}

fn weight_value(hint: &memory_service::MemoryTopicHint) -> f32 {
    hint.weight.unwrap_or(DEFAULT_TOPIC_WEIGHT)
}

fn fallback_topic_from_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let words: Vec<&str> = trimmed.split_whitespace().take(5).collect();
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let mut truncated = text[..max_len].to_string();
    if let Some(idx) = truncated.rfind(' ') {
        truncated.truncate(idx);
    }
    truncated.push('…');
    truncated
}

fn format_thread_summary(
    topic: &ThreadTopic,
    members_len: usize,
    latest_text: &str,
    projects: &[String],
) -> String {
    let mut parts = vec![format!("Story thread \"{}\"", topic.label)];
    if !projects.is_empty() {
        parts.push(format!("projects {}", projects.join(", ")));
    }
    parts.push(format!(
        "{} item{}",
        members_len,
        if members_len == 1 { "" } else { "s" }
    ));
    let headline = parts.join(" · ");
    format!("{headline}: {latest_text}")
}

fn parse_time(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn dedupe_strings(items: &mut Vec<String>) {
    let mut seen: FxHashSet<String> = FxHashSet::default();
    items.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

fn extract_projects(record: &Value) -> Vec<String> {
    let mut set: FxHashSet<String> = FxHashSet::default();
    if let Some(project) = record.get("project_id").and_then(Value::as_str) {
        if !project.is_empty() {
            set.insert(project.to_string());
        }
    }
    if let Some(projects) = record
        .get("value")
        .and_then(|v| v.get("projects"))
        .and_then(Value::as_array)
    {
        for slot in projects.iter().filter_map(Value::as_str) {
            if !slot.is_empty() {
                set.insert(slot.to_string());
            }
        }
    }
    if let Some(project) = record
        .get("value")
        .and_then(|v| v.get("project"))
        .and_then(Value::as_str)
    {
        if !project.is_empty() {
            set.insert(project.to_string());
        }
    }
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    list
}

fn pick_primary_text(record: &Value) -> String {
    if let Some(text) = record.get("text").and_then(Value::as_str) {
        if !text.trim().is_empty() {
            return text.to_string();
        }
    }
    if let Some(text) = record
        .get("value")
        .and_then(|v| v.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.to_string();
        }
    }
    if let Some(text) = record
        .get("value")
        .and_then(|v| v.get("abstract"))
        .and_then(|v| v.get("text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.to_string();
        }
    }
    if let Some(text) = record
        .get("value")
        .and_then(|v| v.get("summary"))
        .and_then(|v| v.get("latest_text"))
        .and_then(Value::as_str)
    {
        if !text.trim().is_empty() {
            return text.to_string();
        }
    }
    record
        .get("value")
        .and_then(|v| v.get("slot"))
        .and_then(Value::as_str)
        .unwrap_or("story thread")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_service::MemoryTopicHint;

    #[test]
    fn slugify_topic_produces_hyphenated_lowercase() {
        assert_eq!(
            memory_service::slugify_topic("Launch Strategy 2026").as_deref(),
            Some("launch-strategy-2026")
        );
        assert_eq!(
            memory_service::slugify_topic("   Spaces   Only   ").as_deref(),
            Some("spaces-only")
        );
        assert_eq!(memory_service::slugify_topic("***"), None);
    }

    #[test]
    fn normalize_topic_hints_dedupes_and_clamps() {
        let hints = vec![
            MemoryTopicHint {
                name: "  Alpha Roadmap ".into(),
                weight: Some(1.4),
                relation: Some("thread.project".into()),
            },
            MemoryTopicHint {
                name: "alpha roadmap".into(),
                weight: Some(-0.2),
                relation: Some("thread.project".into()),
            },
            MemoryTopicHint {
                name: "  ".into(),
                weight: Some(0.5),
                relation: None,
            },
        ];
        let normalized = memory_service::normalize_topic_hints(&hints);
        assert_eq!(normalized.len(), 1);
        let hint = &normalized[0];
        assert_eq!(hint.name, "Alpha Roadmap");
        assert_eq!(hint.relation.as_deref(), Some("thread.project"));
        assert_eq!(hint.weight.unwrap(), 1.0);
        assert_eq!(
            memory_service::slugify_topic(&hint.name).as_deref(),
            Some("alpha-roadmap")
        );
    }

    #[test]
    fn derive_topics_from_summary_prefers_primary_kinds() {
        let summary = json!({
            "abstract": {
                "text": "3 events covering launch readiness",
                "primary_kinds": ["Launch Review", "Retro"]
            },
            "projects": ["atlas"],
            "actors": ["ops"],
            "tags": ["topic:launch-review"],
        });
        let derived = derive_topics_from_summary(&summary);
        assert!(!derived.is_empty());
        assert_eq!(derived[0].name, "Launch Review");
        assert!(derived.iter().any(|hint| hint.name == "Retro"));
        assert!(derived.iter().any(|hint| hint.name == "atlas"));
        assert!(derived.iter().any(|hint| hint.name == "ops"));
        assert_eq!(derived.len(), 4);
    }

    #[test]
    fn truncate_text_respects_word_boundaries() {
        let text = "Story thread narrative about the big launch milestone";
        let truncated = truncate_text(text, 20);
        assert!(truncated.len() <= 21);
        assert!(truncated.ends_with('…'));
    }
}
