use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Kernel {
    db_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventRow {
    pub id: i64,
    pub time: String,
    pub kind: String,
    pub actor: Option<String>,
    pub proj: Option<String>,
    pub corr_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActionRow {
    pub id: String,
    pub kind: String,
    pub input: serde_json::Value,
    pub policy_ctx: Option<serde_json::Value>,
    pub idem_key: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created: String,
    pub updated: String,
}

impl Kernel {
    pub fn open(dir: &Path) -> Result<Self> {
        let db_path = dir.join("events.sqlite");
        let need_init = !db_path.exists();
        let conn = Connection::open(&db_path)?;
        // Pragmas tuned for async server usage
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Busy timeout (default 5000ms; override with ARW_SQLITE_BUSY_MS)
        let busy_ms: u64 = std::env::var("ARW_SQLITE_BUSY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);
        conn.busy_timeout(std::time::Duration::from_millis(busy_ms))?;
        // Cache size: negative = KB units. Default ~= 20MB (20000 KB pages)
        let cache_pages: i64 = std::env::var("ARW_SQLITE_CACHE_PAGES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-20000);
        let _ = conn.pragma_update(None, "cache_size", cache_pages);
        // Keep temp tables in memory
        let _ = conn.pragma_update(None, "temp_store", "MEMORY");
        // mmap_size in bytes (default 128MB), skip on platforms not supporting it
        if let Some(mb) = std::env::var("ARW_SQLITE_MMAP_MB")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
        {
            let bytes: i64 = mb.max(0) * 1024 * 1024;
            let _ = conn.pragma_update(None, "mmap_size", bytes);
        }
        if need_init {
            Self::init_schema(&conn)?;
        }
        Ok(Self { db_path })
    }

    fn init_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS events (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              time TEXT NOT NULL,
              kind TEXT NOT NULL,
              actor TEXT,
              proj TEXT,
              corr_id TEXT,
              payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
            CREATE INDEX IF NOT EXISTS idx_events_time ON events(time);
            CREATE INDEX IF NOT EXISTS idx_events_corr ON events(corr_id);

            CREATE TABLE IF NOT EXISTS artifacts (
              sha256 TEXT PRIMARY KEY,
              mime TEXT,
              bytes BLOB,
              meta TEXT
            );

            CREATE TABLE IF NOT EXISTS actions (
              id TEXT PRIMARY KEY,
              kind TEXT NOT NULL,
              input TEXT NOT NULL,
              policy_ctx TEXT,
              idem_key TEXT,
              state TEXT,
              output TEXT,
              error TEXT,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );

            -- Contribution ledger: append-only accounting of work/resources
            CREATE TABLE IF NOT EXISTS contributions (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              time TEXT NOT NULL,
              subject TEXT NOT NULL,     -- who (node/user/agent)
              kind TEXT NOT NULL,        -- e.g., compute.cpu, compute.gpu, task.submit, task.complete
              qty REAL NOT NULL,         -- numeric quantity
              unit TEXT NOT NULL,        -- ms, tok, task, byte
              corr_id TEXT,
              proj TEXT,
              meta TEXT                  -- JSON blob
            );
            CREATE INDEX IF NOT EXISTS idx_contrib_subject ON contributions(subject);
            CREATE INDEX IF NOT EXISTS idx_contrib_time ON contributions(time);

            -- Leases: capability grants with TTL and optional budget
            CREATE TABLE IF NOT EXISTS leases (
              id TEXT PRIMARY KEY,
              subject TEXT NOT NULL,
              capability TEXT NOT NULL,
              scope TEXT,
              ttl_until TEXT NOT NULL,
              budget REAL,
              policy_ctx TEXT,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_leases_subject ON leases(subject);
            CREATE INDEX IF NOT EXISTS idx_leases_cap ON leases(capability);

            -- Egress ledger: normalized, append-only record of network egress decisions and attribution
            CREATE TABLE IF NOT EXISTS egress_ledger (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              time TEXT NOT NULL,
              decision TEXT NOT NULL,       -- allow | deny | error
              reason TEXT,
              dest_host TEXT,
              dest_port INTEGER,
              protocol TEXT,               -- http|https|tcp|udp
              bytes_in INTEGER,
              bytes_out INTEGER,
              corr_id TEXT,
              proj TEXT,
              posture TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_egress_time ON egress_ledger(time);

            -- Memory records: abstract, multi-lane store with simple search fields
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

            -- Memory FTS (contentless): index key/value/tags for fast retrieval
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
              id UNINDEXED,
              lane UNINDEXED,
              key,
              value,
              tags
            );

            -- Memory links: graph edges between records
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

            -- Config snapshots: persisted effective config for Patch Engine
            CREATE TABLE IF NOT EXISTS config_snapshots (
              id TEXT PRIMARY KEY,
              config TEXT NOT NULL,
              created TEXT NOT NULL
            );

            -- Orchestrator jobs: training mini-agents and coordination tasks
            CREATE TABLE IF NOT EXISTS orchestrator_jobs (
              id TEXT PRIMARY KEY,
              status TEXT NOT NULL,
              goal TEXT,
              data TEXT,
              progress REAL,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_orch_status ON orchestrator_jobs(status);

            -- Logic Units: persisted manifests
            CREATE TABLE IF NOT EXISTS logic_units (
              id TEXT PRIMARY KEY,
              manifest TEXT NOT NULL,
              status TEXT NOT NULL,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    fn conn(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    pub fn append_event(&self, env: &arw_events::Envelope) -> Result<i64> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "INSERT INTO events(time,kind,actor,proj,corr_id,payload) VALUES (?,?,?,?,?,?)",
        )?;
        let payload = serde_json::to_string(&env.payload).unwrap_or("{}".to_string());
        stmt.execute(params![
            env.time,
            env.kind,
            None::<String>,
            None::<String>,
            env.payload
                .get("corr_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            payload,
        ])?;
        Ok(conn.last_insert_rowid())
    }

    pub fn recent_events(&self, limit: i64, after_id: Option<i64>) -> Result<Vec<EventRow>> {
        let conn = self.conn()?;
        let mut stmt_after;
        let mut stmt_all;
        let mut rows = if let Some(aid) = after_id {
            stmt_after = conn.prepare(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events WHERE id>? ORDER BY id ASC LIMIT ?",
            )?;
            stmt_after.query(params![aid, limit])?
        } else {
            stmt_all = conn.prepare(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events ORDER BY id DESC LIMIT ?",
            )?;
            stmt_all.query(params![limit])?
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let time: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let actor: Option<String> = row.get(3)?;
            let proj: Option<String> = row.get(4)?;
            let corr_id: Option<String> = row.get(5)?;
            let payload_s: String = row.get(6)?;
            let payload = serde_json::from_str(&payload_s).unwrap_or(serde_json::json!({}));
            out.push(EventRow {
                id,
                time,
                kind,
                actor,
                proj,
                corr_id,
                payload,
            });
        }
        // Ensure ascending order for replay
        if after_id.is_none() {
            out.reverse();
        }
        Ok(out)
    }

    pub async fn cas_put(
        bytes: &[u8],
        mime: Option<&str>,
        meta: Option<&serde_json::Value>,
        dir: &Path,
    ) -> Result<String> {
        use sha2::Digest as _;
        let mut h = sha2::Sha256::new();
        h.update(bytes);
        let sha = format!("{:x}", h.finalize());
        let cas_dir = dir.join("blobs");
        tokio::fs::create_dir_all(&cas_dir).await.ok();
        let path = cas_dir.join(format!("{}.bin", sha));
        if tokio::fs::metadata(&path).await.is_err() {
            tokio::fs::write(&path, bytes).await?;
        }
        let meta_path = cas_dir.join(format!("{}.json", sha));
        let meta_obj = serde_json::json!({"mime": mime, "meta": meta});
        tokio::fs::write(&meta_path, serde_json::to_vec(&meta_obj)?)
            .await
            .ok();
        Ok(sha)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn insert_action(
        &self,
        id: &str,
        kind: &str,
        input: &serde_json::Value,
        policy_ctx: Option<&serde_json::Value>,
        idem_key: Option<&str>,
        state: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let input_s = serde_json::to_string(input).unwrap_or("{}".to_string());
        let policy_s = policy_ctx.map(|v| serde_json::to_string(v).unwrap_or("{}".to_string()));
        conn.execute(
            "INSERT OR REPLACE INTO actions(id,kind,input,policy_ctx,idem_key,state,created,updated) VALUES(?,?,?,?,?,?,?,?)",
            params![
                id,
                kind,
                input_s,
                policy_s,
                idem_key,
                state,
                now,
                now
            ],
        )?;
        Ok(())
    }

    pub fn find_action_by_idem(&self, idem: &str) -> Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id FROM actions WHERE idem_key=? LIMIT 1")?;
        let id_opt: Option<String> = stmt.query_row([idem], |row| row.get(0)).optional()?;
        Ok(id_opt)
    }

    pub fn get_action(&self, id: &str) -> Result<Option<ActionRow>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,kind,input,policy_ctx,idem_key,state,output,error,created,updated FROM actions WHERE id=? LIMIT 1",
        )?;
        let res: Result<ActionRow, _> = stmt.query_row([id], |row| {
            let input_s: String = row.get(2)?;
            let policy_s: Option<String> = row.get(3)?;
            let input_v = serde_json::from_str(&input_s).unwrap_or(serde_json::json!({}));
            let policy_v =
                policy_s.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
            Ok(ActionRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                input: input_v,
                policy_ctx: policy_v,
                idem_key: row.get(4)?,
                state: row.get(5)?,
                output: row
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                error: row.get(7)?,
                created: row.get(8)?,
                updated: row.get(9)?,
            })
        });
        match res {
            Ok(a) => Ok(Some(a)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_action_state(&self, id: &str, state: &str) -> Result<bool> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let n = conn.execute(
            "UPDATE actions SET state=?, updated=? WHERE id=?",
            params![state, now, id],
        )?;
        Ok(n > 0)
    }

    pub fn update_action_result(
        &self,
        id: &str,
        output: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let out_s = output.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        let n = conn.execute(
            "UPDATE actions SET output=COALESCE(?,output), error=COALESCE(?,error), updated=? WHERE id=?",
            params![out_s, error, now, id],
        )?;
        Ok(n > 0)
    }

    pub fn list_actions(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,kind,state,created,updated FROM actions ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "kind": r.get::<_, String>(1)?,
                "state": r.get::<_, String>(2)?,
                "created": r.get::<_, String>(3)?,
                "updated": r.get::<_, String>(4)?,
            }));
        }
        Ok(out)
    }

    pub fn count_actions_by_state(&self, state: &str) -> Result<i64> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT COUNT(1) FROM actions WHERE state=?")?;
        let n: i64 = stmt.query_row([state], |row| row.get(0))?;
        Ok(n)
    }

    pub fn dequeue_one_queued(&self) -> Result<Option<(String, String, serde_json::Value)>> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut stmt = conn.prepare(
            "UPDATE actions SET state='running', updated=? WHERE id = (
                 SELECT id FROM actions WHERE state='queued' ORDER BY created LIMIT 1
             ) RETURNING id, kind, input",
        )?;
        let mut rows = stmt.query(params![now])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let kind: String = row.get(1)?;
            let input_s: String = row.get(2)?;
            let input_v = serde_json::from_str(&input_s).unwrap_or(serde_json::json!({}));
            return Ok(Some((id, kind, input_v)));
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_lease(
        &self,
        id: &str,
        subject: &str,
        capability: &str,
        scope: Option<&str>,
        ttl_until: &str,
        budget: Option<f64>,
        policy_ctx: Option<&serde_json::Value>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let policy_s = policy_ctx.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        conn.execute(
            "INSERT OR REPLACE INTO leases(id,subject,capability,scope,ttl_until,budget,policy_ctx,created,updated) VALUES(?,?,?,?,?,?,?,?,?)",
            params![id, subject, capability, scope, ttl_until, budget, policy_s, now, now],
        )?;
        Ok(())
    }

    pub fn list_leases(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,subject,capability,scope,ttl_until,budget,policy_ctx,created,updated FROM leases ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let policy_s: Option<String> = r.get(6)?;
            let policy_v = policy_s
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .unwrap_or(serde_json::json!({}));
            out.push(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "subject": r.get::<_, String>(1)?,
                "capability": r.get::<_, String>(2)?,
                "scope": r.get::<_, Option<String>>(3)?,
                "ttl_until": r.get::<_, String>(4)?,
                "budget": r.get::<_, Option<f64>>(5)?,
                "policy": policy_v,
                "created": r.get::<_, String>(7)?,
                "updated": r.get::<_, String>(8)?,
            }));
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_contribution(
        &self,
        subject: &str,
        kind: &str,
        qty: f64,
        unit: &str,
        corr_id: Option<&str>,
        proj: Option<&str>,
        meta: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let meta_s = meta.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        conn.execute(
            "INSERT INTO contributions(time,subject,kind,qty,unit,corr_id,proj,meta) VALUES(?,?,?,?,?,?,?,?)",
            params![now, subject, kind, qty, unit, corr_id, proj, meta_s],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_contributions(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,time,subject,kind,qty,unit,corr_id,proj,meta FROM contributions ORDER BY id DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let meta_s: Option<String> = r.get(8)?;
            let meta_v = meta_s
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .unwrap_or(serde_json::json!({}));
            out.push(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "time": r.get::<_, String>(1)?,
                "subject": r.get::<_, String>(2)?,
                "kind": r.get::<_, String>(3)?,
                "qty": r.get::<_, f64>(4)?,
                "unit": r.get::<_, String>(5)?,
                "corr_id": r.get::<_, Option<String>>(6)?,
                "proj": r.get::<_, Option<String>>(7)?,
                "meta": meta_v,
            }));
        }
        Ok(out)
    }

    pub fn find_valid_lease(
        &self,
        subject: &str,
        capability: &str,
    ) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut stmt = conn.prepare(
            "SELECT id,subject,capability,scope,ttl_until,budget,policy_ctx,created,updated FROM leases \
             WHERE subject=? AND capability=? AND ttl_until > ? ORDER BY ttl_until DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![subject, capability, now])?;
        if let Some(r) = rows.next()? {
            let policy_s: Option<String> = r.get(6)?;
            let policy_v = policy_s
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .unwrap_or(serde_json::json!({}));
            let v = serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "subject": r.get::<_, String>(1)?,
                "capability": r.get::<_, String>(2)?,
                "scope": r.get::<_, Option<String>>(3)?,
                "ttl_until": r.get::<_, String>(4)?,
                "budget": r.get::<_, Option<f64>>(5)?,
                "policy": policy_v,
                "created": r.get::<_, String>(7)?,
                "updated": r.get::<_, String>(8)?,
            });
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }

    pub async fn find_valid_lease_async(
        &self,
        subject: &str,
        capability: &str,
    ) -> Result<Option<serde_json::Value>> {
        let k = self.clone();
        let s = subject.to_string();
        let c = capability.to_string();
        tokio::task::spawn_blocking(move || k.find_valid_lease(&s, &c))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_egress(
        &self,
        decision: &str,
        reason: Option<&str>,
        dest_host: Option<&str>,
        dest_port: Option<i64>,
        protocol: Option<&str>,
        bytes_in: Option<i64>,
        bytes_out: Option<i64>,
        corr_id: Option<&str>,
        proj: Option<&str>,
        posture: Option<&str>,
    ) -> Result<i64> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        conn.execute(
            "INSERT INTO egress_ledger(time,decision,reason,dest_host,dest_port,protocol,bytes_in,bytes_out,corr_id,proj,posture) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
            params![
                now,
                decision,
                reason,
                dest_host,
                dest_port,
                protocol,
                bytes_in,
                bytes_out,
                corr_id,
                proj,
                posture
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_egress(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,time,decision,reason,dest_host,dest_port,protocol,bytes_in,bytes_out,corr_id,proj,posture FROM egress_ledger ORDER BY id DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "time": r.get::<_, String>(1)?,
                "decision": r.get::<_, String>(2)?,
                "reason": r.get::<_, Option<String>>(3)?,
                "dest_host": r.get::<_, Option<String>>(4)?,
                "dest_port": r.get::<_, Option<i64>>(5)?,
                "protocol": r.get::<_, Option<String>>(6)?,
                "bytes_in": r.get::<_, Option<i64>>(7)?,
                "bytes_out": r.get::<_, Option<i64>>(8)?,
                "corr_id": r.get::<_, Option<String>>(9)?,
                "proj": r.get::<_, Option<String>>(10)?,
                "posture": r.get::<_, Option<String>>(11)?,
            }));
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_memory(
        &self,
        id_opt: Option<&str>,
        lane: &str,
        kind: Option<&str>,
        key: Option<&str>,
        value: &serde_json::Value,
        embed: Option<&[f32]>,
        tags: Option<&[String]>,
        score: Option<f64>,
        prob: Option<f64>,
    ) -> Result<String> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let value_s = serde_json::to_string(value).unwrap_or("{}".to_string());
        let embed_s = embed.map(|v| {
            let arr: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", arr.join(","))
        });
        // Compute a stable hash of lane+kind+key+value
        use sha2::Digest as _;
        let mut h = sha2::Sha256::new();
        h.update(lane.as_bytes());
        if let Some(k) = kind {
            h.update(k.as_bytes());
        }
        if let Some(k) = key {
            h.update(k.as_bytes());
        }
        h.update(value_s.as_bytes());
        let hash = format!("{:x}", h.finalize());
        let id = id_opt
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let tags_s = tags.map(|ts| ts.join(","));
        conn.execute(
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
        // Upsert into FTS index
        let _ = conn.execute(
            "INSERT INTO memory_fts(id,lane,key,value,tags) VALUES(?,?,?,?,?)",
            params![
                id,
                lane,
                key.unwrap_or(""),
                value_s,
                match tags {
                    Some(ts) => ts.join(","),
                    None => String::new(),
                }
            ],
        );
        Ok(id)
    }

    pub fn search_memory(
        &self,
        q: &str,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let like = format!("%{}%", q);
        let mut out = Vec::new();
        if let Some(l) = lane {
            let mut stmt = conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,score,prob,updated FROM memory_records \
                 WHERE lane=? AND (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?) \
                 ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![l, like, like, like, limit])?;
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "lane": r.get::<_, String>(1)?,
                    "kind": r.get::<_, Option<String>>(2)?,
                    "key": r.get::<_, Option<String>>(3)?,
                    "value": value_v,
                    "tags": r.get::<_, Option<String>>(5)?,
                    "hash": r.get::<_, Option<String>>(6)?,
                    "score": r.get::<_, Option<f64>>(7)?,
                    "prob": r.get::<_, Option<f64>>(8)?,
                    "updated": r.get::<_, String>(9)?,
                }));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,score,prob,updated FROM memory_records \
                 WHERE (COALESCE(key,'') LIKE ? OR COALESCE(value,'') LIKE ? OR COALESCE(tags,'') LIKE ?) \
                 ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![like, like, like, limit])?;
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "lane": r.get::<_, String>(1)?,
                    "kind": r.get::<_, Option<String>>(2)?,
                    "key": r.get::<_, Option<String>>(3)?,
                    "value": value_v,
                    "tags": r.get::<_, Option<String>>(5)?,
                    "hash": r.get::<_, Option<String>>(6)?,
                    "score": r.get::<_, Option<f64>>(7)?,
                    "prob": r.get::<_, Option<f64>>(8)?,
                    "updated": r.get::<_, String>(9)?,
                }));
            }
        }
        Ok(out)
    }

    pub fn fts_search_memory(
        &self,
        q: &str,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut out = Vec::new();
        if let Some(l) = lane {
            let mut stmt = conn.prepare(
                "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.score,r.prob,r.updated \
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id \
                 WHERE f.memory_fts MATCH ? AND f.lane=? \
                 LIMIT ?",
            )?;
            let mut rows = stmt.query(params![q, l, limit])?;
            while let Some(rw) = rows.next()? {
                let value_s: String = rw.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": rw.get::<_, String>(0)?,
                    "lane": rw.get::<_, String>(1)?,
                    "kind": rw.get::<_, Option<String>>(2)?,
                    "key": rw.get::<_, Option<String>>(3)?,
                    "value": value_v,
                    "tags": rw.get::<_, Option<String>>(5)?,
                    "hash": rw.get::<_, Option<String>>(6)?,
                    "score": rw.get::<_, Option<f64>>(7)?,
                    "prob": rw.get::<_, Option<f64>>(8)?,
                    "updated": rw.get::<_, String>(9)?,
                }));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.score,r.prob,r.updated \
                 FROM memory_records r JOIN memory_fts f ON f.id=r.id \
                 WHERE f.memory_fts MATCH ? \
                 LIMIT ?",
            )?;
            let mut rows = stmt.query(params![q, limit])?;
            while let Some(rw) = rows.next()? {
                let value_s: String = rw.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": rw.get::<_, String>(0)?,
                    "lane": rw.get::<_, String>(1)?,
                    "kind": rw.get::<_, Option<String>>(2)?,
                    "key": rw.get::<_, Option<String>>(3)?,
                    "value": value_v,
                    "tags": rw.get::<_, Option<String>>(5)?,
                    "hash": rw.get::<_, Option<String>>(6)?,
                    "score": rw.get::<_, Option<f64>>(7)?,
                    "prob": rw.get::<_, Option<f64>>(8)?,
                    "updated": rw.get::<_, String>(9)?,
                }));
            }
        }
        Ok(out)
    }

    pub fn search_memory_by_embedding(
        &self,
        embed: &[f32],
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        // Naive scan: consider recent records with non-null embed and same length vector.
        let conn = self.conn()?;
        let mut out: Vec<(f32, serde_json::Value)> = Vec::new();
        let sql = if lane.is_some() {
            "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 1000"
        } else {
            "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records ORDER BY updated DESC LIMIT 1000"
        };
        let mut stmt = conn.prepare(sql)?;
        let mut rows = if let Some(l) = lane {
            stmt.query([l])?
        } else {
            stmt.query([])?
        };
        while let Some(r) = rows.next()? {
            let embed_s: Option<String> = r.get(7)?;
            if let Some(es) = embed_s {
                if let Ok(vec_json) = serde_json::from_str::<serde_json::Value>(&es) {
                    if let Some(arr) = vec_json.as_array() {
                        let mut v2: Vec<f32> = Vec::with_capacity(arr.len());
                        for v in arr.iter() {
                            if let Some(f) = v.as_f64() {
                                v2.push(f as f32);
                            }
                        }
                        if v2.len() == embed.len() && !embed.is_empty() {
                            let sim = Self::cosine_sim(embed, &v2);
                            let value_s: String = r.get(4)?;
                            let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                                .unwrap_or(serde_json::json!({}));
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
                            out.push((sim, item));
                        }
                    }
                }
            }
        }
        // sort by sim desc, take limit
        out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let items: Vec<serde_json::Value> = out
            .into_iter()
            .take(limit as usize)
            .map(|(_, v)| v)
            .collect();
        Ok(items)
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

    pub fn select_memory_hybrid(
        &self,
        q: Option<&str>,
        embed: Option<&[f32]>,
        lane: Option<&str>,
        k: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        // Candidate set via FTS when q present, else recent by updated
        let mut candidates: Vec<serde_json::Value> = Vec::new();
        if let Some(qs) = q {
            if !qs.is_empty() {
                let mut stmt = if lane.is_some() {
                    conn.prepare(
                        "SELECT r.id,r.lane,r.kind,r.key,r.value,r.tags,r.hash,r.embed,r.score,r.prob,r.updated \
                         FROM memory_records r JOIN memory_fts f ON f.id=r.id \
                         WHERE f.memory_fts MATCH ? AND f.lane=? \
                         ORDER BY r.updated DESC LIMIT 400",
                    )?
                } else {
                    conn.prepare(
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
                    let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                        .unwrap_or(serde_json::json!({}));
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
        // If no FTS candidates or to enrich, fetch recent
        if candidates.is_empty() {
            let mut stmt = if lane.is_some() {
                conn.prepare(
                    "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT 400",
                )?
            } else {
                conn.prepare("SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records ORDER BY updated DESC LIMIT 400")?
            };
            let mut rows = if let Some(l) = lane {
                stmt.query(params![l])?
            } else {
                stmt.query([])?
            };
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
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
        // Score and sort
        let now = chrono::Utc::now();
        let mut scored: Vec<(f32, serde_json::Value)> = Vec::new();
        for mut item in candidates {
            let mut sim = 0f32;
            if let (Some(es), Some(e)) = (item.get("embed").and_then(|v| v.as_str()), embed) {
                if let Ok(vec_json) = serde_json::from_str::<serde_json::Value>(es) {
                    if let Some(arr) = vec_json.as_array() {
                        let mut v2: Vec<f32> = Vec::with_capacity(arr.len());
                        for v in arr.iter() {
                            if let Some(f) = v.as_f64() {
                                v2.push(f as f32);
                            }
                        }
                        if v2.len() == e.len() && !e.is_empty() {
                            sim = Self::cosine_sim(e, &v2);
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
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| {
                    let age = now
                        .signed_duration_since(t.with_timezone(&chrono::Utc))
                        .num_seconds()
                        .max(0) as f64;
                    let hl = 3600f64 * 6f64; // 6h half-life
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
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let items: Vec<serde_json::Value> = scored
            .into_iter()
            .take(k as usize)
            .map(|(_, v)| v)
            .collect();
        Ok(items)
    }

    pub fn insert_memory_link(
        &self,
        src_id: &str,
        dst_id: &str,
        rel: Option<&str>,
        weight: Option<f64>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let relv = rel.unwrap_or("");
        conn.execute(
            "INSERT OR REPLACE INTO memory_links(src_id,dst_id,rel,weight,created,updated) VALUES(?,?,?,?,?,?)",
            params![src_id, dst_id, relv, weight, now, now],
        )?;
        Ok(())
    }

    pub fn list_memory_links(&self, src_id: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT dst_id,rel,weight,updated FROM memory_links WHERE src_id=? ORDER BY updated DESC LIMIT ?")?;
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

    pub fn get_memory(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE id=? LIMIT 1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            let value_s: String = r.get(4)?;
            let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                .unwrap_or(serde_json::json!({}));
            Ok(Some(serde_json::json!({
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
            })))
        } else {
            Ok(None)
        }
    }

    pub fn list_recent_memory(
        &self,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut out = Vec::new();
        if let Some(l) = lane {
            let mut stmt = conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records WHERE lane=? ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![l, limit])?;
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
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
                }));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id,lane,kind,key,value,tags,hash,embed,score,prob,updated FROM memory_records ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![limit])?;
            while let Some(r) = rows.next()? {
                let value_s: String = r.get(4)?;
                let value_v = serde_json::from_str::<serde_json::Value>(&value_s)
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
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
                }));
            }
        }
        Ok(out)
    }

    // ---------- Config snapshots ----------
    pub fn insert_config_snapshot(&self, config: &serde_json::Value) -> Result<String> {
        let conn = self.conn()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let cfg = serde_json::to_string(config).unwrap_or("{}".into());
        conn.execute(
            "INSERT INTO config_snapshots(id,config,created) VALUES(?,?,?)",
            params![id, cfg, now],
        )?;
        Ok(id)
    }

    pub fn get_config_snapshot(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT config FROM config_snapshots WHERE id=? LIMIT 1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            let cfg_s: String = r.get(0)?;
            let v =
                serde_json::from_str::<serde_json::Value>(&cfg_s).unwrap_or(serde_json::json!({}));
            Ok(Some(v))
        } else {
            Ok(None)
        }
    }

    pub fn list_config_snapshots(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id,created FROM config_snapshots ORDER BY created DESC LIMIT ?")?;
        let mut rows = stmt.query(params![limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(serde_json::json!({"id": r.get::<_, String>(0)?, "created": r.get::<_, String>(1)?}));
        }
        Ok(out)
    }

    // ---------- Orchestrator jobs ----------
    pub fn insert_orchestrator_job(
        &self,
        goal: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<String> {
        let conn = self.conn()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let data_s = data.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        conn.execute(
            "INSERT INTO orchestrator_jobs(id,status,goal,data,progress,created,updated) VALUES(?,?,?,?,?,?,?)",
            params![id, "queued", goal, data_s, 0.0f64, now, now],
        )?;
        Ok(id)
    }

    pub fn update_orchestrator_job(
        &self,
        id: &str,
        status: Option<&str>,
        progress: Option<f64>,
    ) -> Result<bool> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut set_parts: Vec<&str> = Vec::new();
        if status.is_some() {
            set_parts.push("status=?");
        }
        if progress.is_some() {
            set_parts.push("progress=?");
        }
        set_parts.push("updated=?");
        let sql = format!(
            "UPDATE orchestrator_jobs SET {} WHERE id=?",
            set_parts.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(s) = status {
            params_vec.push(rusqlite::types::Value::from(s.to_string()));
        }
        if let Some(p) = progress {
            params_vec.push(rusqlite::types::Value::from(p));
        }
        params_vec.push(rusqlite::types::Value::from(now.clone()));
        params_vec.push(rusqlite::types::Value::from(id.to_string()));
        let n = stmt.execute(rusqlite::params_from_iter(params_vec))?;
        Ok(n > 0)
    }

    pub fn list_orchestrator_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,status,goal,progress,created,updated FROM orchestrator_jobs ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            out.push(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "status": r.get::<_, String>(1)?,
                "goal": r.get::<_, Option<String>>(2)?,
                "progress": r.get::<_, Option<f64>>(3)?,
                "created": r.get::<_, String>(4)?,
                "updated": r.get::<_, String>(5)?,
            }));
        }
        Ok(out)
    }

    // ---------- Logic Units ----------
    pub fn insert_logic_unit(
        &self,
        id: &str,
        manifest: &serde_json::Value,
        status: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mf_s = serde_json::to_string(manifest).unwrap_or("{}".into());
        conn.execute(
            "INSERT OR REPLACE INTO logic_units(id,manifest,status,created,updated) VALUES(?,?,?,?,?)",
            params![id, mf_s, status, now, now],
        )?;
        Ok(())
    }

    pub fn list_logic_units(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id,manifest,status,created,updated FROM logic_units ORDER BY updated DESC LIMIT ?")?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let mf_s: String = r.get(1)?;
            let mf_v =
                serde_json::from_str::<serde_json::Value>(&mf_s).unwrap_or(serde_json::json!({}));
            out.push(serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "manifest": mf_v,
                "status": r.get::<_, String>(2)?,
                "created": r.get::<_, String>(3)?,
                "updated": r.get::<_, String>(4)?,
            }));
        }
        Ok(out)
    }

    // ---------------- Async wrappers (spawn_blocking) ----------------
    // These helpers offload rusqlite work from async executors.

    pub async fn append_event_async(&self, env: &arw_events::Envelope) -> Result<i64> {
        let k = self.clone();
        let env = env.clone();
        tokio::task::spawn_blocking(move || k.append_event(&env))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn recent_events_async(
        &self,
        limit: i64,
        after_id: Option<i64>,
    ) -> Result<Vec<EventRow>> {
        let k = self.clone();
        tokio::task::spawn_blocking(move || k.recent_events(limit, after_id))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn count_actions_by_state_async(&self, state: &str) -> Result<i64> {
        let k = self.clone();
        let s = state.to_string();
        tokio::task::spawn_blocking(move || k.count_actions_by_state(&s))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn find_action_by_idem_async(&self, idem: &str) -> Result<Option<String>> {
        let k = self.clone();
        let s = idem.to_string();
        tokio::task::spawn_blocking(move || k.find_action_by_idem(&s))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn insert_action_async(
        &self,
        id: &str,
        kind: &str,
        input: &serde_json::Value,
        policy_ctx: Option<&serde_json::Value>,
        idem_key: Option<&str>,
        state: &str,
    ) -> Result<()> {
        let k = self.clone();
        let id = id.to_string();
        let kind = kind.to_string();
        let input = input.clone();
        let policy_ctx = policy_ctx.cloned();
        let idem_key = idem_key.map(|s| s.to_string());
        let state_s = state.to_string();
        tokio::task::spawn_blocking(move || {
            k.insert_action(
                &id,
                &kind,
                &input,
                policy_ctx.as_ref(),
                idem_key.as_deref(),
                &state_s,
            )
        })
        .await
        .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn get_action_async(&self, id: &str) -> Result<Option<ActionRow>> {
        let k = self.clone();
        let s = id.to_string();
        tokio::task::spawn_blocking(move || k.get_action(&s))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn set_action_state_async(&self, id: &str, state: &str) -> Result<bool> {
        let k = self.clone();
        let id_s = id.to_string();
        let st = state.to_string();
        tokio::task::spawn_blocking(move || k.set_action_state(&id_s, &st))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn append_contribution_async(
        &self,
        subject: &str,
        kind: &str,
        qty: f64,
        unit: &str,
        corr_id: Option<&str>,
        proj: Option<&str>,
        meta: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let k = self.clone();
        let subject = subject.to_string();
        let kind = kind.to_string();
        let unit = unit.to_string();
        let corr_id = corr_id.map(|s| s.to_string());
        let proj = proj.map(|s| s.to_string());
        let meta = meta.cloned();
        tokio::task::spawn_blocking(move || {
            k.append_contribution(
                &subject,
                &kind,
                qty,
                &unit,
                corr_id.as_deref(),
                proj.as_deref(),
                meta.as_ref(),
            )
        })
        .await
        .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn list_contributions_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let k = self.clone();
        tokio::task::spawn_blocking(move || k.list_contributions(limit))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn list_actions_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let k = self.clone();
        tokio::task::spawn_blocking(move || k.list_actions(limit))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }

    pub async fn list_egress_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let k = self.clone();
        tokio::task::spawn_blocking(move || k.list_egress(limit))
            .await
            .map_err(|e| anyhow!("join error: {}", e))?
    }
}
