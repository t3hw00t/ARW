//! Core SQLite helpers backing ARW's memory overlay: schema migrations,
//! hybrid retrieval primitives, and lightweight ranking utilities.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use uuid::Uuid;

const SELECT_COLUMN_LIST: &[&str] = &[
    "id",
    "lane",
    "kind",
    "key",
    "value",
    "tags",
    "hash",
    "embed",
    "embed_hint",
    "score",
    "prob",
    "created",
    "updated",
    "agent_id",
    "project_id",
    "text",
    "durability",
    "trust",
    "privacy",
    "ttl_s",
    "keywords",
    "entities",
    "source",
    "links",
    "extra",
];

fn select_columns(prefix: Option<&str>) -> String {
    match prefix {
        Some(p) => SELECT_COLUMN_LIST
            .iter()
            .map(|col| format!("{p}.{col}"))
            .collect::<Vec<_>>()
            .join(","),
        None => SELECT_COLUMN_LIST.join(","),
    }
}

/// Lightweight wrapper around a `rusqlite::Connection` that exposes
/// memory-specific helpers (schema setup + CRUD/search primitives).
pub struct MemoryStore<'c> {
    conn: &'c Connection,
}

pub struct MemoryInsertArgs<'a> {
    pub id: Option<&'a str>,
    pub lane: &'a str,
    pub kind: Option<&'a str>,
    pub key: Option<&'a str>,
    pub value: &'a Value,
    pub embed: Option<&'a [f32]>,
    pub embed_hint: Option<&'a str>,
    pub tags: Option<&'a [String]>,
    pub score: Option<f64>,
    pub prob: Option<f64>,
    pub agent_id: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub text: Option<&'a str>,
    pub durability: Option<&'a str>,
    pub trust: Option<f64>,
    pub privacy: Option<&'a str>,
    pub ttl_s: Option<i64>,
    pub keywords: Option<&'a [String]>,
    pub entities: Option<&'a Value>,
    pub source: Option<&'a Value>,
    pub links: Option<&'a Value>,
    pub extra: Option<&'a Value>,
    pub hash: Option<String>,
}

impl<'a> MemoryInsertArgs<'a> {
    pub fn compute_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.lane.as_bytes());
        if let Some(kind) = self.kind {
            hasher.update(kind.as_bytes());
        }
        if let Some(key) = self.key {
            hasher.update(key.as_bytes());
        }
        if let Some(agent) = self.agent_id {
            hasher.update(agent.as_bytes());
        }
        if let Some(project) = self.project_id {
            hasher.update(project.as_bytes());
        }
        if let Some(text) = self.text {
            hasher.update(text.as_bytes());
        }
        if let Ok(bytes) = serde_json::to_vec(self.value) {
            hasher.update(bytes);
        }
        format!("{:x}", hasher.finalize())
    }
}

#[derive(Clone, Debug)]
pub struct MemoryInsertOwned {
    pub id: Option<String>,
    pub lane: String,
    pub kind: Option<String>,
    pub key: Option<String>,
    pub value: Value,
    pub embed: Option<Vec<f32>>,
    pub embed_hint: Option<String>,
    pub tags: Option<Vec<String>>,
    pub score: Option<f64>,
    pub prob: Option<f64>,
    pub agent_id: Option<String>,
    pub project_id: Option<String>,
    pub text: Option<String>,
    pub durability: Option<String>,
    pub trust: Option<f64>,
    pub privacy: Option<String>,
    pub ttl_s: Option<i64>,
    pub keywords: Option<Vec<String>>,
    pub entities: Option<Value>,
    pub source: Option<Value>,
    pub links: Option<Value>,
    pub extra: Option<Value>,
    pub hash: Option<String>,
}

impl MemoryInsertOwned {
    pub fn to_args(&self) -> MemoryInsertArgs<'_> {
        MemoryInsertArgs {
            id: self.id.as_deref(),
            lane: &self.lane,
            kind: self.kind.as_deref(),
            key: self.key.as_deref(),
            value: &self.value,
            embed: self.embed.as_deref(),
            embed_hint: self.embed_hint.as_deref(),
            tags: self.tags.as_ref().map(|v| v.as_slice()),
            score: self.score,
            prob: self.prob,
            agent_id: self.agent_id.as_deref(),
            project_id: self.project_id.as_deref(),
            text: self.text.as_deref(),
            durability: self.durability.as_deref(),
            trust: self.trust,
            privacy: self.privacy.as_deref(),
            ttl_s: self.ttl_s,
            keywords: self.keywords.as_ref().map(|v| v.as_slice()),
            entities: self.entities.as_ref(),
            source: self.source.as_ref(),
            links: self.links.as_ref(),
            extra: self.extra.as_ref(),
            hash: self.hash.clone(),
        }
    }

    pub fn compute_hash(&self) -> String {
        self.to_args().compute_hash()
    }
}

impl<'c> MemoryStore<'c> {
    pub fn new(conn: &'c Connection) -> Self {
        Self { conn }
    }

    pub fn migrate(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS memory_records (
              id TEXT PRIMARY KEY,
              lane TEXT NOT NULL,
              kind TEXT,
              key TEXT,
              value TEXT NOT NULL,
              tags TEXT,
              hash TEXT,
              embed TEXT,
              embed_hint TEXT,
              score REAL,
              prob REAL,
              agent_id TEXT,
              project_id TEXT,
              text TEXT,
              durability TEXT,
              trust REAL,
              privacy TEXT,
              ttl_s INTEGER,
              keywords TEXT,
              entities TEXT,
              source TEXT,
              links TEXT,
              extra TEXT,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_mem_lane ON memory_records(lane);
            CREATE INDEX IF NOT EXISTS idx_mem_key ON memory_records(key);
            CREATE INDEX IF NOT EXISTS idx_mem_hash ON memory_records(hash);
            CREATE INDEX IF NOT EXISTS idx_mem_agent_project ON memory_records(agent_id, project_id, updated DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
              id UNINDEXED,
              lane UNINDEXED,
              key,
              value,
              tags
            );

            CREATE TABLE IF NOT EXISTS memory_links (
              src_id TEXT NOT NULL,
              dst_id TEXT NOT NULL,
              rel TEXT NOT NULL,
              weight REAL,
              created TEXT NOT NULL,
              updated TEXT NOT NULL,
              PRIMARY KEY (src_id,dst_id,rel)
            );
            CREATE INDEX IF NOT EXISTS idx_mem_links_src ON memory_links(src_id);
            "#,
        )?;
        for ddl in [
            "ALTER TABLE memory_records ADD COLUMN embed_hint TEXT",
            "ALTER TABLE memory_records ADD COLUMN agent_id TEXT",
            "ALTER TABLE memory_records ADD COLUMN project_id TEXT",
            "ALTER TABLE memory_records ADD COLUMN text TEXT",
            "ALTER TABLE memory_records ADD COLUMN durability TEXT",
            "ALTER TABLE memory_records ADD COLUMN trust REAL",
            "ALTER TABLE memory_records ADD COLUMN privacy TEXT",
            "ALTER TABLE memory_records ADD COLUMN ttl_s INTEGER",
            "ALTER TABLE memory_records ADD COLUMN keywords TEXT",
            "ALTER TABLE memory_records ADD COLUMN entities TEXT",
            "ALTER TABLE memory_records ADD COLUMN source TEXT",
            "ALTER TABLE memory_records ADD COLUMN links TEXT",
            "ALTER TABLE memory_records ADD COLUMN extra TEXT",
        ] {
            let _ = conn.execute(ddl, []);
        }
        Ok(())
    }

    pub fn insert_memory(&self, args: &MemoryInsertArgs<'_>) -> Result<String> {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value_s = serde_json::to_string(args.value).unwrap_or_else(|_| "{}".to_string());
        let embed_s = args.embed.map(|v| {
            let arr: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", arr.join(","))
        });
        let hash = args.hash.clone().unwrap_or_else(|| args.compute_hash());
        let id = args
            .id
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let tags_joined = args.tags.map(|ts| ts.join(","));
        let keywords_joined = args.keywords.map(|kw| kw.join(","));
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_records(
                id,lane,kind,key,value,tags,hash,embed,embed_hint,score,prob,
                agent_id,project_id,text,durability,trust,privacy,ttl_s,keywords,entities,source,links,extra,created,updated
            ) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                id,
                args.lane,
                args.kind,
                args.key,
                value_s,
                tags_joined.clone(),
                hash,
                embed_s,
                args.embed_hint,
                args.score,
                args.prob,
                args.agent_id,
                args.project_id,
                args.text,
                args.durability,
                args.trust,
                args.privacy,
                args.ttl_s,
                keywords_joined,
                args.entities.and_then(|v| serde_json::to_string(v).ok()),
                args.source.and_then(|v| serde_json::to_string(v).ok()),
                args.links.and_then(|v| serde_json::to_string(v).ok()),
                args.extra.and_then(|v| serde_json::to_string(v).ok()),
                now,
                now,
            ],
        )?;
        let _ = self
            .conn
            .execute("DELETE FROM memory_fts WHERE id=?", params![id.as_str()]);
        let _ = self.conn.execute(
            "INSERT INTO memory_fts(id,lane,key,value,tags) VALUES(?,?,?,?,?)",
            params![
                id,
                args.lane,
                args.key.unwrap_or(""),
                value_s,
                tags_joined.unwrap_or_default(),
            ],
        );
        Ok(id)
    }

    pub fn search_memory(&self, query: &str, lane: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let like_q = format!("%{}%", query);
        if let Some(l) = lane {
            let sql = format!(
                "SELECT {cols} FROM memory_records
                 WHERE lane=? AND (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?)
                 ORDER BY updated DESC LIMIT ?",
                cols = select_columns(None)
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![l, like_q, like_q, like_q, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        } else {
            let sql = format!(
                "SELECT {cols} FROM memory_records
                 WHERE (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?)
                 ORDER BY updated DESC LIMIT ?",
                cols = select_columns(None)
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![like_q, like_q, like_q, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        }
        Ok(out)
    }

    pub fn fts_search_memory(
        &self,
        query: &str,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        if let Some(l) = lane {
            let sql = format!(
                "SELECT {cols}
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id
                 WHERE f.memory_fts MATCH ? AND f.lane=?
                 ORDER BY r.updated DESC LIMIT ?",
                cols = select_columns(Some("r"))
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![query, l, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        } else {
            let sql = format!(
                "SELECT {cols}
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id
                 WHERE f.memory_fts MATCH ?
                 ORDER BY r.updated DESC LIMIT ?",
                cols = select_columns(Some("r"))
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![query, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        }
        Ok(out)
    }

    pub fn search_memory_by_embedding(
        &self,
        embed: &[f32],
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        if embed.is_empty() {
            return Ok(Vec::new());
        }
        let sql = if lane.is_some() {
            format!(
                "SELECT {cols} FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 1000",
                cols = select_columns(None)
            )
        } else {
            format!(
                "SELECT {cols} FROM memory_records ORDER BY updated DESC LIMIT 1000",
                cols = select_columns(None)
            )
        };
        let mut stmt = self.conn.prepare(sql.as_str())?;
        let mut rows = if let Some(l) = lane {
            stmt.query(params![l])?
        } else {
            stmt.query([])?
        };
        let mut scored: Vec<(f32, Value)> = Vec::new();
        while let Some(r) = rows.next()? {
            let embed_s: Option<String> = r.get(7)?;
            if let Some(embed_str) = embed_s {
                if let Ok(embed_vec) = parse_embedding(&embed_str) {
                    if embed_vec.len() == embed.len() && !embed_vec.is_empty() {
                        if let Ok(sim) = cosine_similarity(embed, &embed_vec) {
                            let mut item = row_to_value_full(r)?;
                            if let Some(obj) = item.as_object_mut() {
                                obj.insert("sim".into(), json!(sim));
                            }
                            scored.push((sim, item));
                        }
                    }
                }
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        Ok(scored
            .into_iter()
            .take(limit as usize)
            .map(|(_, v)| v)
            .collect())
    }

    pub fn select_memory_hybrid(
        &self,
        query: Option<&str>,
        embed: Option<&[f32]>,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Value>> {
        let mut candidates: Vec<Value> = Vec::new();
        if let Some(qs) = query {
            if !qs.is_empty() {
                let sql = if lane.is_some() {
                    format!(
                        "SELECT {cols}
                         FROM memory_records r JOIN memory_fts f ON f.id=r.id
                         WHERE f.memory_fts MATCH ? AND f.lane=?
                         ORDER BY r.updated DESC LIMIT 400",
                        cols = select_columns(Some("r"))
                    )
                } else {
                    format!(
                        "SELECT {cols}
                         FROM memory_records r JOIN memory_fts f ON f.id=r.id
                         WHERE f.memory_fts MATCH ?
                         ORDER BY r.updated DESC LIMIT 400",
                        cols = select_columns(Some("r"))
                    )
                };
                let mut stmt = self.conn.prepare(&sql)?;
                let mut rows = if let Some(l) = lane {
                    stmt.query(params![qs, l])?
                } else {
                    stmt.query(params![qs])?
                };
                while let Some(r) = rows.next()? {
                    let mut record = row_to_value_full(r)?;
                    if let Some(obj) = record.as_object_mut() {
                        obj.insert("_fts_hit".into(), Value::Bool(true));
                    }
                    candidates.push(record);
                }
            }
        }
        if candidates.is_empty() {
            let sql = if lane.is_some() {
                format!(
                    "SELECT {cols} FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 400",
                    cols = select_columns(None)
                )
            } else {
                format!(
                    "SELECT {cols} FROM memory_records ORDER BY updated DESC LIMIT 400",
                    cols = select_columns(None)
                )
            };
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = if let Some(l) = lane {
                stmt.query(params![l])?
            } else {
                stmt.query([])?
            };
            while let Some(r) = rows.next()? {
                let mut record = row_to_value_full(r)?;
                if let Some(obj) = record.as_object_mut() {
                    obj.insert("_fts_hit".into(), Value::Bool(false));
                }
                candidates.push(record);
            }
        }
        let now = Utc::now();
        let mut scored: Vec<(f32, Value)> = Vec::new();
        for mut item in candidates {
            let mut sim = 0f32;
            if let (Some(embed_values), Some(e)) = (item.get("embed"), embed) {
                if let Some(arr) = embed_values.as_array() {
                    let mut v2: Vec<f32> = Vec::with_capacity(arr.len());
                    for v in arr.iter() {
                        if let Some(f) = v.as_f64() {
                            v2.push(f as f32);
                        }
                    }
                    if v2.len() == e.len() && !e.is_empty() {
                        sim = cosine_sim(e, &v2);
                    }
                }
            }
            let fts_hit = item
                .get("_fts_hit")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let recency = item
                .get("updated")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| {
                    let age = now
                        .signed_duration_since(t.with_timezone(&Utc))
                        .num_seconds()
                        .max(0) as f64;
                    let hl = 6.0f64 * 3600.0f64;
                    ((-age / hl).exp()) as f32
                })
                .unwrap_or(0.5);
            let util = item
                .get("score")
                .and_then(|v| v.as_f64())
                .map(|s| s.clamp(0.0, 1.0) as f32)
                .unwrap_or(0.0);
            let w_sim = 0.5f32;
            let w_fts = 0.2f32;
            let w_rec = 0.2f32;
            let w_util = 0.1f32;
            let fts_score = if fts_hit { 1.0 } else { 0.0 };
            let cscore = w_sim * sim + w_fts * fts_score + w_rec * recency + w_util * util;
            if let Some(obj) = item.as_object_mut() {
                obj.insert("cscore".into(), serde_json::json!(cscore));
                obj.insert("sim".into(), serde_json::json!(sim));
            }
            scored.push((cscore, item));
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
        Ok(scored
            .into_iter()
            .take(limit as usize)
            .map(|(_, v)| v)
            .collect())
    }

    pub fn insert_memory_link(
        &self,
        src_id: &str,
        dst_id: &str,
        rel: Option<&str>,
        weight: Option<f64>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let rel = rel.unwrap_or("");
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_links(src_id,dst_id,rel,weight,created,updated) VALUES(?,?,?,?,?,?)",
            params![src_id, dst_id, rel, weight, now, now],
        )?;
        Ok(())
    }

    pub fn list_memory_links(&self, src_id: &str, limit: i64) -> Result<Vec<Value>> {
        let mut stmt = self.conn.prepare(
            "SELECT dst_id,rel,weight,updated FROM memory_links WHERE src_id=? ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query(params![src_id, limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(serde_json::json!({
                "dst_id": r.get::<_, String>(0)?,
                "rel": r.get::<_, String>(1)?,
                "weight": r.get::<_, Option<f64>>(2)?,
                "updated": r.get::<_, String>(3)?,
            }));
        }
        Ok(out)
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<Value>> {
        let sql = format!(
            "SELECT {cols} FROM memory_records WHERE id=? LIMIT 1",
            cols = select_columns(None)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            Ok(Some(row_to_value_full(r)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_recent_memory(&self, lane: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        if let Some(l) = lane {
            let sql = format!(
                "SELECT {cols} FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT ?",
                cols = select_columns(None)
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![l, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value_full(r)?);
            }
        } else {
            let sql = format!(
                "SELECT {cols} FROM memory_records ORDER BY updated DESC LIMIT ?",
                cols = select_columns(None)
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let mut rows = stmt.query(params![limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value_full(r)?);
            }
        }
        Ok(out)
    }

    pub fn find_memory_by_hash(&self, hash: &str) -> Result<Option<Value>> {
        let sql = format!(
            "SELECT {cols} FROM memory_records WHERE hash=? LIMIT 1",
            cols = select_columns(None)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![hash])?;
        if let Some(r) = rows.next()? {
            Ok(Some(row_to_value_full(r)?))
        } else {
            Ok(None)
        }
    }
}

fn row_to_value(row: &rusqlite::Row<'_>) -> Result<Value> {
    row_to_value_common(row)
}

fn row_to_value_full(row: &rusqlite::Row<'_>) -> Result<Value> {
    row_to_value_common(row)
}

fn row_to_value_common(row: &rusqlite::Row<'_>) -> Result<Value> {
    let mut map = Map::new();
    map.insert("id".into(), json!(row.get::<_, String>(0)?));
    map.insert("lane".into(), json!(row.get::<_, String>(1)?));
    if let Some(kind) = row.get::<_, Option<String>>(2)? {
        map.insert("kind".into(), json!(kind));
    }
    if let Some(key) = row.get::<_, Option<String>>(3)? {
        map.insert("key".into(), json!(key));
    }

    let value_s: String = row.get(4)?;
    let value_v =
        serde_json::from_str::<Value>(&value_s).unwrap_or_else(|_| Value::Object(Map::new()));
    map.insert("value".into(), value_v);

    let tags_value = row
        .get::<_, Option<String>>(5)?
        .map(|s| split_list(&s))
        .unwrap_or_else(|| Vec::new());
    map.insert("tags".into(), Value::Array(tags_value));

    if let Some(hash) = row.get::<_, Option<String>>(6)? {
        map.insert("hash".into(), json!(hash));
    }

    if let Some(embed) = row.get::<_, Option<String>>(7)? {
        if let Ok(vec) = parse_embedding(&embed) {
            if !vec.is_empty() {
                map.insert("embed".into(), json!(vec));
            }
        }
    }
    if let Some(hint) = row.get::<_, Option<String>>(8)? {
        map.insert("embed_hint".into(), json!(hint));
    }

    if let Some(score) = row.get::<_, Option<f64>>(9)? {
        map.insert("score".into(), json!(score));
    }
    if let Some(prob) = row.get::<_, Option<f64>>(10)? {
        map.insert("prob".into(), json!(prob));
    }

    if let Some(created) = row.get::<_, Option<String>>(11)? {
        map.insert("created".into(), json!(created));
    }
    map.insert("updated".into(), json!(row.get::<_, String>(12)?));

    if let Some(agent) = row.get::<_, Option<String>>(13)? {
        map.insert("agent_id".into(), json!(agent));
    }
    if let Some(project) = row.get::<_, Option<String>>(14)? {
        map.insert("project_id".into(), json!(project));
    }
    if let Some(text) = row.get::<_, Option<String>>(15)? {
        map.insert("text".into(), json!(text));
    }
    if let Some(durability) = row.get::<_, Option<String>>(16)? {
        map.insert("durability".into(), json!(durability));
    }
    if let Some(trust) = row.get::<_, Option<f64>>(17)? {
        map.insert("trust".into(), json!(trust));
    }
    if let Some(privacy) = row.get::<_, Option<String>>(18)? {
        map.insert("privacy".into(), json!(privacy));
    }
    if let Some(ttl) = row.get::<_, Option<i64>>(19)? {
        map.insert("ttl_s".into(), json!(ttl));
    }

    let keywords_value = row
        .get::<_, Option<String>>(20)?
        .map(|s| split_list(&s))
        .unwrap_or_else(|| Vec::new());
    if !keywords_value.is_empty() {
        map.insert("keywords".into(), Value::Array(keywords_value));
    }

    if let Some(entities) = parse_json_string(row.get::<_, Option<String>>(21)?) {
        map.insert("entities".into(), entities);
    }
    if let Some(source) = parse_json_string(row.get::<_, Option<String>>(22)?) {
        map.insert("source".into(), source);
    }
    if let Some(links) = parse_json_string(row.get::<_, Option<String>>(23)?) {
        map.insert("links".into(), links);
    }
    if let Some(extra) = parse_json_string(row.get::<_, Option<String>>(24)?) {
        map.insert("extra".into(), extra);
    }

    Ok(Value::Object(map))
}

fn split_list(input: &str) -> Vec<Value> {
    input
        .split(',')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .map(|part| Value::String(part.to_string()))
        .collect()
}

fn parse_json_string(input: Option<String>) -> Option<Value> {
    input.and_then(|s| serde_json::from_str::<Value>(&s).ok())
}

fn parse_embedding(embed_s: &str) -> Result<Vec<f32>> {
    let trimmed = embed_s.trim_matches(['[', ']']);
    if trimmed.is_empty() {
        return Ok(vec![]);
    }
    let values = trimmed
        .split(',')
        .map(|s| s.trim().parse::<f32>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(values)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return Err(anyhow!("invalid embeddings for cosine similarity"));
    }
    let dot = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return Err(anyhow!("zero norm embeddings"));
    }
    Ok(dot / (norm_a * norm_b))
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0f32;
    let mut na = 0f32;
    let mut nb = 0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0f32 || nb == 0f32 {
        0f32
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use serde_json::json;

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        MemoryStore::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_get_memory() {
        let conn = setup_conn();
        let store = MemoryStore::new(&conn);
        let insert_owned = MemoryInsertOwned {
            id: None,
            lane: "episodic".to_string(),
            kind: Some("summary".to_string()),
            key: Some("key".to_string()),
            value: serde_json::json!({"text":"hello"}),
            embed: None,
            embed_hint: None,
            tags: Some(vec!["tag1".to_string()]),
            score: Some(0.9),
            prob: Some(0.8),
            agent_id: None,
            project_id: None,
            text: None,
            durability: None,
            trust: None,
            privacy: None,
            ttl_s: None,
            keywords: None,
            entities: None,
            source: None,
            links: None,
            extra: None,
            hash: None,
        };
        let args = insert_owned.to_args();
        let id = store.insert_memory(&args).unwrap();
        let fetched = store.get_memory(&id).unwrap().unwrap();
        assert_eq!(fetched["lane"], "episodic");
    }

    #[test]
    fn test_search_memory_by_embedding_yields_sim() {
        let conn = setup_conn();
        let store = MemoryStore::new(&conn);
        let insert_owned = MemoryInsertOwned {
            id: None,
            lane: "semantic".to_string(),
            kind: Some("fact".to_string()),
            key: Some("key".to_string()),
            value: json!({ "text": "vector memo" }),
            embed: Some(vec![1.0, 0.0]),
            embed_hint: None,
            tags: None,
            score: None,
            prob: None,
            agent_id: None,
            project_id: None,
            text: None,
            durability: None,
            trust: None,
            privacy: None,
            ttl_s: None,
            keywords: None,
            entities: None,
            source: None,
            links: None,
            extra: None,
            hash: None,
        };
        let args = insert_owned.to_args();
        let id = store.insert_memory(&args).unwrap();
        let hits = store
            .search_memory_by_embedding(&[1.0, 0.0], Some("semantic"), 1)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"], id);
        assert!(hits[0]["sim"].as_f64().unwrap() > 0.99);
    }

    #[test]
    fn test_fts_index_stays_in_sync_on_upsert() {
        let conn = setup_conn();
        let store = MemoryStore::new(&conn);

        let insert_owned = MemoryInsertOwned {
            id: Some("rec-1".to_string()),
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            key: Some("key".to_string()),
            value: json!("first note"),
            embed: None,
            embed_hint: None,
            tags: None,
            score: None,
            prob: None,
            agent_id: None,
            project_id: None,
            text: None,
            durability: None,
            trust: None,
            privacy: None,
            ttl_s: None,
            keywords: None,
            entities: None,
            source: None,
            links: None,
            extra: None,
            hash: None,
        };
        let args = insert_owned.to_args();
        let id = store.insert_memory(&args).unwrap();
        assert_eq!(id, "rec-1");

        let hits = store.fts_search_memory("first", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"], "rec-1");

        let insert_owned = MemoryInsertOwned {
            id: Some("rec-1".to_string()),
            lane: "semantic".to_string(),
            kind: Some("note".to_string()),
            key: Some("key".to_string()),
            value: json!("second memo"),
            embed: None,
            embed_hint: None,
            tags: None,
            score: None,
            prob: None,
            agent_id: None,
            project_id: None,
            text: None,
            durability: None,
            trust: None,
            privacy: None,
            ttl_s: None,
            keywords: None,
            entities: None,
            source: None,
            links: None,
            extra: None,
            hash: None,
        };
        let args_again = insert_owned.to_args();
        let id_again = store.insert_memory(&args_again).unwrap();
        assert_eq!(id_again, "rec-1");

        let old_hits = store.fts_search_memory("first", None, 10).unwrap();
        assert!(old_hits.is_empty());

        let new_hits = store.fts_search_memory("second", None, 10).unwrap();
        assert_eq!(new_hits.len(), 1);
        assert_eq!(new_hits[0]["id"], "rec-1");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_fts WHERE id = ?",
                params!["rec-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
