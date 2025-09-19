//! Core SQLite helpers backing ARW's memory overlay: schema migrations,
//! hybrid retrieval primitives, and lightweight ranking utilities.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use uuid::Uuid;

/// Lightweight wrapper around a `rusqlite::Connection` that exposes
/// memory-specific helpers (schema setup + CRUD/search primitives).
pub struct MemoryStore<'c> {
    conn: &'c Connection,
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
              score REAL,
              prob REAL,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_mem_lane ON memory_records(lane);
            CREATE INDEX IF NOT EXISTS idx_mem_key ON memory_records(key);
            CREATE INDEX IF NOT EXISTS idx_mem_hash ON memory_records(hash);

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
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_memory(
        &self,
        id_opt: Option<&str>,
        lane: &str,
        kind: Option<&str>,
        key: Option<&str>,
        value: &Value,
        embed: Option<&[f32]>,
        tags: Option<&[String]>,
        score: Option<f64>,
        prob: Option<f64>,
    ) -> Result<String> {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value_s = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
        let embed_s = embed.map(|v| {
            let arr: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", arr.join(","))
        });
        let mut hasher = Sha256::new();
        hasher.update(lane.as_bytes());
        if let Some(k) = kind {
            hasher.update(k.as_bytes());
        }
        if let Some(k) = key {
            hasher.update(k.as_bytes());
        }
        hasher.update(value_s.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let id = id_opt
            .map(|s| s.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let tags_joined = tags.map(|ts| ts.join(","));
        let tags_s = tags_joined.clone();
        self.conn.execute(
            "INSERT OR REPLACE INTO memory_records(id,lane,kind,key,value,tags,hash,embed,score,prob,created,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                id,
                lane,
                kind,
                key,
                value_s,
                tags_s,
                hash,
                embed_s,
                score,
                prob,
                now,
                now,
            ],
        )?;
        let _ = self.conn.execute(
            "INSERT INTO memory_fts(id,lane,key,value,tags) VALUES(?,?,?,?,?)",
            params![
                id,
                lane,
                key.unwrap_or(""),
                value_s.clone(),
                tags_joined.unwrap_or_default(),
            ],
        );
        Ok(id)
    }

    pub fn search_memory(&self, query: &str, lane: Option<&str>, limit: i64) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        let like_q = format!("%{}%", query);
        if let Some(l) = lane {
            let mut stmt = self.conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,score,prob,updated FROM memory_records \
                 WHERE lane=? AND (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?) \
                 ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![l, like_q, like_q, like_q, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,score,prob,updated FROM memory_records \
                 WHERE (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?) \
                 ORDER BY updated DESC LIMIT ?",
            )?;
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
            let mut stmt = self.conn.prepare(
                "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.score,r.prob,r.updated
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id
                 WHERE f.memory_fts MATCH ? AND f.lane=?
                 ORDER BY r.updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![query, l, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value(r)?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.score,r.prob,r.updated
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id
                 WHERE f.memory_fts MATCH ?
                 ORDER BY r.updated DESC LIMIT ?",
            )?;
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
            "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated \
             FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 1000"
        } else {
            "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated \
             FROM memory_records ORDER BY updated DESC LIMIT 1000"
        };
        let mut stmt = self.conn.prepare(sql)?;
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
                            let value_s: String = r.get(4)?;
                            let value_v = serde_json::from_str::<Value>(&value_s)
                                .unwrap_or_else(|_| Value::Object(Default::default()));
                            let item = serde_json::json!({
                                "id": r.get::<_, String>(0)?,
                                "lane": r.get::<_, String>(1)?,
                                "kind": r.get::<_, Option<String>>(2)?,
                                "key": r.get::<_, Option<String>>(3)?,
                                "value": value_v,
                                "tags": r.get::<_, Option<String>>(5)?,
                                "hash": r.get::<_, Option<String>>(6)?,
                                "score": r.get::<_, Option<f64>>(8)?,
                                "prob": r.get::<_, Option<f64>>(9)?,
                                "updated": r.get::<_, String>(10)?,
                                "sim": sim,
                            });
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
                let mut stmt = if lane.is_some() {
                    self.conn.prepare(
                        "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.embed,r.score,r.prob,r.updated \
                         FROM memory_records r JOIN memory_fts f ON f.id=r.id \
                         WHERE f.memory_fts MATCH ? AND f.lane=? \
                         ORDER BY r.updated DESC LIMIT 400",
                    )?
                } else {
                    self.conn.prepare(
                        "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.embed,r.score,r.prob,r.updated \
                         FROM memory_records r JOIN memory_fts f ON f.id=r.id \
                         WHERE f.memory_fts MATCH ? \
                         ORDER BY r.updated DESC LIMIT 400",
                    )?
                };
                let mut rows = if let Some(l) = lane {
                    stmt.query(params![qs, l])?
                } else {
                    stmt.query(params![qs])?
                };
                while let Some(r) = rows.next()? {
                    let value_s: String = r.get(4)?;
                    let value_v = serde_json::from_str::<Value>(&value_s)
                        .unwrap_or_else(|_| Value::Object(Default::default()));
                    candidates.push(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "lane": r.get::<_, String>(1)?,
                        "kind": r.get::<_, Option<String>>(2)?,
                        "key": r.get::<_, Option<String>>(3)?,
                        "value": value_v,
                        "tags": r.get::<_, Option<String>>(5)?,
                        "hash": r.get::<_, Option<String>>(6)?,
                        "embed": r.get::<_, Option<String>>(7)?,
                        "score": r.get::<_, Option<f64>>(8)?,
                        "prob": r.get::<_, Option<f64>>(9)?,
                        "updated": r.get::<_, String>(10)?,
                        "_fts_hit": true,
                    }));
                }
            }
        }
        if candidates.is_empty() {
            let mut stmt = if lane.is_some() {
                self.conn.prepare(
                    "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 400",
                )?
            } else {
                self.conn.prepare(
                    "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records ORDER BY updated DESC LIMIT 400",
                )?
            };
            let mut rows = if let Some(l) = lane {
                stmt.query(params![l])?
            } else {
                stmt.query([])?
            };
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<Value>(&value_s)
                    .unwrap_or_else(|_| Value::Object(Default::default()));
                candidates.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "lane": r.get::<_, String>(1)?,
                    "kind": r.get::<_, Option<String>>(2)?,
                    "key": r.get::<_, Option<String>>(3)?,
                    "value": value_v,
                    "tags": r.get::<_, Option<String>>(5)?,
                    "hash": r.get::<_, Option<String>>(6)?,
                    "embed": r.get::<_, Option<String>>(7)?,
                    "score": r.get::<_, Option<f64>>(8)?,
                    "prob": r.get::<_, Option<f64>>(9)?,
                    "updated": r.get::<_, String>(10)?,
                    "_fts_hit": false,
                }));
            }
        }
        let now = Utc::now();
        let mut scored: Vec<(f32, Value)> = Vec::new();
        for mut item in candidates {
            let mut sim = 0f32;
            if let (Some(es), Some(e)) = (item.get("embed").and_then(|v| v.as_str()), embed) {
                if let Ok(embed_vec) = serde_json::from_str::<Value>(es) {
                    if let Some(arr) = embed_vec.as_array() {
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
        let mut stmt = self.conn.prepare(
            "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE id=? LIMIT 1",
        )?;
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
            let mut stmt = self.conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![l, limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value_full(r)?);
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![limit])?;
            while let Some(r) = rows.next()? {
                out.push(row_to_value_full(r)?);
            }
        }
        Ok(out)
    }
}

fn row_to_value(row: &rusqlite::Row<'_>) -> Result<Value> {
    let value_s: String = row.get(4)?;
    let value_v = serde_json::from_str::<Value>(&value_s)
        .unwrap_or_else(|_| Value::Object(Default::default()));
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "lane": row.get::<_, String>(1)?,
        "kind": row.get::<_, Option<String>>(2)?,
        "key": row.get::<_, Option<String>>(3)?,
        "value": value_v,
        "tags": row.get::<_, Option<String>>(5)?,
        "hash": row.get::<_, Option<String>>(6)?,
        "score": row.get::<_, Option<f64>>(7)?,
        "prob": row.get::<_, Option<f64>>(8)?,
        "updated": row.get::<_, String>(9)?,
    }))
}

fn row_to_value_full(row: &rusqlite::Row<'_>) -> Result<Value> {
    let value_s: String = row.get(4)?;
    let value_v = serde_json::from_str::<Value>(&value_s)
        .unwrap_or_else(|_| Value::Object(Default::default()));
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "lane": row.get::<_, String>(1)?,
        "kind": row.get::<_, Option<String>>(2)?,
        "key": row.get::<_, Option<String>>(3)?,
        "value": value_v,
        "tags": row.get::<_, Option<String>>(5)?,
        "hash": row.get::<_, Option<String>>(6)?,
        "embed": row.get::<_, Option<String>>(7)?,
        "score": row.get::<_, Option<f64>>(8)?,
        "prob": row.get::<_, Option<f64>>(9)?,
        "updated": row.get::<_, String>(10)?,
    }))
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
        let id = store
            .insert_memory(
                None,
                "episodic",
                Some("summary"),
                Some("key"),
                &serde_json::json!({"text":"hello"}),
                None,
                Some(&["tag1".to_string()]),
                Some(0.9),
                Some(0.8),
            )
            .unwrap();
        let fetched = store.get_memory(&id).unwrap().unwrap();
        assert_eq!(fetched["lane"], "episodic");
    }

    #[test]
    fn test_search_memory_by_embedding_yields_sim() {
        let conn = setup_conn();
        let store = MemoryStore::new(&conn);
        let id = store
            .insert_memory(
                None,
                "semantic",
                Some("fact"),
                Some("key"),
                &json!({ "text": "vector memo" }),
                Some(&[1.0, 0.0]),
                None,
                None,
                None,
            )
            .unwrap();
        let hits = store
            .search_memory_by_embedding(&[1.0, 0.0], Some("semantic"), 1)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0]["id"], id);
        assert!(hits[0]["sim"].as_f64().unwrap() > 0.99);
    }
}
