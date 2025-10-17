use anyhow::{anyhow, Result};
use arw_memory_core::{MemoryInsertArgs, MemoryInsertOwned, MemoryStore};
use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, VecDeque};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use uuid::Uuid;

pub use arw_memory_core::{MemoryGcCandidate, MemoryGcReason};

#[derive(Clone)]
pub struct Kernel {
    db_path: PathBuf,
    pragmas: Arc<KernelPragmas>,
    pool: Arc<PoolShared>,
    checkpoint: Option<Arc<CheckpointCtl>>,
    autotune: Option<Arc<AutotuneCtl>>,
    blocking: BlockingPool,
}

pub struct KernelSession {
    conn: ManagedConnection,
}

#[derive(Clone)]
struct KernelPragmas {
    journal_mode: String,
    synchronous: String,
    busy_timeout_ms: u64,
    cache_pages: i64,
    temp_store: String,
    mmap_bytes: Option<i64>,
}

struct PoolShared {
    state: Mutex<PoolState>,
    wait_stats: Mutex<WaitStats>,
    cvar: Condvar,
    target_size: AtomicUsize,
    min_size: usize,
    max_ceiling: usize,
}

struct PoolState {
    conns: Vec<Connection>,
    created: usize,
}

#[derive(Default)]
struct WaitStats {
    count: u64,
    total_ms: f64,
}

struct ManagedConnection {
    conn: Option<Connection>,
    pool: Arc<PoolShared>,
}

struct CheckpointCtl {
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

struct AutotuneCtl {
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<thread::JoinHandle<()>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaEntry {
    pub id: String,
    pub owner_kind: String,
    pub owner_ref: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub archetype: Option<String>,
    #[serde(default)]
    pub traits: JsonValue,
    #[serde(default)]
    pub preferences: JsonValue,
    #[serde(default)]
    pub worldview: JsonValue,
    #[serde(default)]
    pub vibe_profile: JsonValue,
    #[serde(default)]
    pub calibration: JsonValue,
    pub updated: String,
    pub version: i64,
}

#[derive(Debug, Clone)]
pub struct PersonaEntryUpsert {
    pub id: String,
    pub owner_kind: String,
    pub owner_ref: String,
    pub name: Option<String>,
    pub archetype: Option<String>,
    pub traits: JsonValue,
    pub preferences: JsonValue,
    pub worldview: JsonValue,
    pub vibe_profile: JsonValue,
    pub calibration: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaProposal {
    pub proposal_id: String,
    pub persona_id: String,
    pub submitted_by: String,
    pub diff: JsonValue,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub telemetry_scope: JsonValue,
    #[serde(default)]
    pub leases_required: JsonValue,
    pub status: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Clone)]
pub struct PersonaProposalCreate {
    pub persona_id: String,
    pub submitted_by: String,
    pub diff: JsonValue,
    pub rationale: Option<String>,
    pub telemetry_scope: JsonValue,
    pub leases_required: JsonValue,
}

#[derive(Debug, Clone)]
pub struct PersonaProposalStatusUpdate {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaHistoryEntry {
    pub id: i64,
    pub persona_id: String,
    #[serde(default)]
    pub proposal_id: Option<String>,
    pub diff: JsonValue,
    #[serde(default)]
    pub applied_by: Option<String>,
    pub applied_at: String,
}

#[derive(Debug, Clone)]
pub struct PersonaHistoryAppend {
    pub persona_id: String,
    pub proposal_id: Option<String>,
    pub diff: JsonValue,
    pub applied_by: Option<String>,
}

fn parse_json_or_default(raw: Option<String>, default_value: JsonValue) -> JsonValue {
    match raw {
        Some(raw) => serde_json::from_str::<JsonValue>(&raw).unwrap_or(default_value),
        None => default_value,
    }
}

fn merge_json(base: &mut JsonValue, patch: &JsonValue) {
    use serde_json::Value;
    match (base, patch) {
        (Value::Object(base_map), Value::Object(patch_map)) => {
            for (key, value) in patch_map {
                match (base_map.get_mut(key), value) {
                    (Some(base_child), Value::Object(_)) => {
                        merge_json(base_child, value);
                    }
                    (_, Value::Null) => {
                        base_map.insert(key.clone(), Value::Null);
                    }
                    _ => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_slot, patch_value) => {
            *base_slot = patch_value.clone();
        }
    }
}

impl PoolShared {
    fn record_metrics(&self, state: &PoolState) {
        #[cfg(feature = "metrics")]
        {
            let available = state.conns.len() as f64;
            let total = state.created as f64;
            let in_use = total - available;
            metrics::gauge!("arw_kernel_pool_available").set(available);
            metrics::gauge!("arw_kernel_pool_total").set(total);
            metrics::gauge!("arw_kernel_pool_in_use").set(in_use);
        }
        #[cfg(not(feature = "metrics"))]
        let _ = state;
    }

    fn record_wait(&self, waited: Duration) {
        {
            let mut stats = self
                .wait_stats
                .lock()
                .expect("pool wait stats mutex poisoned");
            stats.count = stats.count.saturating_add(1);
            stats.total_ms += waited.as_secs_f64() * 1000.0;
        }
        #[cfg(feature = "metrics")]
        {
            metrics::counter!("arw_kernel_pool_wait_total").increment(1);
            metrics::histogram!("arw_kernel_pool_wait_ms").record(waited.as_secs_f64() * 1000.0);
        }
    }

    fn shrink_to(&self, target: usize) {
        let mut guard = self.state.lock().expect("pool mutex poisoned");
        while guard.created > target {
            if guard.conns.pop().is_some() {
                guard.created -= 1;
            } else {
                break;
            }
        }
        self.record_metrics(&guard);
        drop(guard);
        self.cvar.notify_all();
    }
}

impl Deref for ManagedConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().expect("connection already taken")
    }
}

impl DerefMut for ManagedConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn.as_mut().expect("connection already taken")
    }
}

impl Drop for ManagedConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut guard = self.pool.state.lock().expect("pool mutex poisoned");
            guard.conns.push(conn);
            let target = self.pool.target_size.load(Ordering::Relaxed);
            while guard.created > target {
                if guard.conns.pop().is_some() {
                    guard.created -= 1;
                } else {
                    break;
                }
            }
            self.pool.record_metrics(&guard);
            drop(guard);
            self.pool.cvar.notify_one();
        } else {
            let mut guard = self.pool.state.lock().expect("pool mutex poisoned");
            if guard.created > 0 {
                guard.created -= 1;
            }
            let target = self.pool.target_size.load(Ordering::Relaxed);
            while guard.created > target {
                if guard.conns.pop().is_some() {
                    guard.created -= 1;
                } else {
                    break;
                }
            }
            self.pool.record_metrics(&guard);
            drop(guard);
            self.pool.cvar.notify_one();
        }
    }
}

impl CheckpointCtl {
    fn new(stop: Arc<AtomicBool>, handle: thread::JoinHandle<()>) -> Self {
        Self {
            stop,
            handle: Mutex::new(Some(handle)),
        }
    }
}

impl Drop for CheckpointCtl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self
            .handle
            .lock()
            .expect("checkpoint join mutex poisoned")
            .take()
        {
            let _ = handle.join();
        }
    }
}

impl AutotuneCtl {
    fn new(stop: Arc<AtomicBool>, handle: thread::JoinHandle<()>) -> Self {
        Self {
            stop,
            handle: Mutex::new(Some(handle)),
        }
    }
}

impl Drop for AutotuneCtl {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self
            .handle
            .lock()
            .expect("autotune join mutex poisoned")
            .take()
        {
            let _ = handle.join();
        }
    }
}

type BlockingJob = Box<dyn FnOnce() + Send + 'static>;

#[derive(Clone)]
struct BlockingPool {
    state: Arc<BlockingPoolState>,
}

struct BlockingPoolState {
    queue: Mutex<VecDeque<BlockingJob>>,
    cvar: Condvar,
    shutdown: AtomicBool,
    workers: Mutex<Vec<thread::JoinHandle<()>>>,
}

#[derive(Debug)]
enum BlockingError {
    ShuttingDown,
    WorkerExited,
}

impl std::fmt::Display for BlockingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockingError::ShuttingDown => write!(f, "blocking pool shutting down"),
            BlockingError::WorkerExited => write!(f, "blocking pool worker exited unexpectedly"),
        }
    }
}

impl std::error::Error for BlockingError {}

impl BlockingPool {
    fn new(size: usize) -> Result<Self> {
        let target = size.max(1);
        let state = Arc::new(BlockingPoolState {
            queue: Mutex::new(VecDeque::new()),
            cvar: Condvar::new(),
            shutdown: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        });
        for idx in 0..target {
            let worker_state = Arc::clone(&state);
            let handle = thread::Builder::new()
                .name(format!("arw-kernel-blocking-{idx}"))
                .spawn(move || BlockingPoolState::worker_loop(worker_state))
                .map_err(|e| anyhow!("failed to spawn kernel blocking worker: {e}"))?;
            state
                .workers
                .lock()
                .expect("blocking pool workers mutex poisoned")
                .push(handle);
        }
        Ok(Self { state })
    }

    async fn run<F, R>(&self, job: F) -> Result<R>
    where
        F: FnOnce() -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.state
            .enqueue(Box::new(move || {
                let res = job();
                let _ = tx.send(res);
            }))
            .map_err(|e| anyhow!(e))?;
        rx.await.map_err(|_| anyhow!(BlockingError::WorkerExited))?
    }
}

impl BlockingPoolState {
    fn worker_loop(state: Arc<Self>) {
        loop {
            let job_opt = {
                let mut guard = state
                    .queue
                    .lock()
                    .expect("blocking pool queue mutex poisoned");
                loop {
                    if let Some(job) = guard.pop_front() {
                        let depth = guard.len();
                        state.record_depth(depth);
                        break Some(job);
                    }
                    if state.shutdown.load(Ordering::Acquire) {
                        break None;
                    }
                    guard = state
                        .cvar
                        .wait(guard)
                        .expect("blocking pool condvar poisoned");
                }
            };
            match job_opt {
                Some(job) => {
                    state.record_dequeued();
                    job()
                }
                None => {
                    state.record_depth(0);
                    break;
                }
            }
        }
    }

    fn enqueue(&self, job: BlockingJob) -> Result<(), BlockingError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(BlockingError::ShuttingDown);
        }
        let mut guard = self
            .queue
            .lock()
            .expect("blocking pool queue mutex poisoned");
        if self.shutdown.load(Ordering::Acquire) {
            return Err(BlockingError::ShuttingDown);
        }
        guard.push_back(job);
        let depth = guard.len();
        drop(guard);
        self.record_depth(depth);
        self.record_enqueued();
        self.cvar.notify_one();
        Ok(())
    }

    fn record_depth(&self, depth: usize) {
        #[cfg(feature = "metrics")]
        metrics::gauge!("arw_kernel_blocking_queue_depth").set(depth as f64);
        #[cfg(not(feature = "metrics"))]
        let _ = depth;
    }

    fn record_enqueued(&self) {
        #[cfg(feature = "metrics")]
        metrics::counter!("arw_kernel_blocking_enqueued").increment(1);
    }

    fn record_dequeued(&self) {
        #[cfg(feature = "metrics")]
        metrics::counter!("arw_kernel_blocking_dequeued").increment(1);
    }
}

impl Drop for BlockingPoolState {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        self.cvar.notify_all();
        let mut handles = self
            .workers
            .lock()
            .expect("blocking pool workers mutex poisoned");
        while let Some(handle) = handles.pop() {
            let _ = handle.join();
        }
    }
}

impl KernelPragmas {
    fn from_env() -> Self {
        let busy_timeout_ms: u64 = std::env::var("ARW_SQLITE_BUSY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);
        let cache_pages: i64 = std::env::var("ARW_SQLITE_CACHE_PAGES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(-20000);
        let mmap_bytes = std::env::var("ARW_SQLITE_MMAP_MB")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .map(|mb| mb.max(0) * 1024 * 1024);
        Self {
            journal_mode: "WAL".to_string(),
            synchronous: "NORMAL".to_string(),
            busy_timeout_ms,
            cache_pages,
            temp_store: "MEMORY".to_string(),
            mmap_bytes,
        }
    }
}

fn blocking_worker_count() -> usize {
    std::env::var("ARW_KERNEL_BLOCKING_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 16))
                .unwrap_or(4)
        })
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResearchWatcherItem {
    pub id: String,
    pub source: Option<String>,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub status: String,
    pub note: Option<String>,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StagingAction {
    pub id: String,
    pub action_kind: String,
    pub action_input: serde_json::Value,
    pub project: Option<String>,
    pub requested_by: Option<String>,
    pub evidence: Option<serde_json::Value>,
    pub status: String,
    pub decision: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub action_id: Option<String>,
    pub created: String,
    pub updated: String,
}

impl Kernel {
    pub fn open(dir: &Path) -> Result<Self> {
        let db_path = dir.join("events.sqlite");
        let need_init = !db_path.exists();
        let pragmas = Arc::new(KernelPragmas::from_env());
        let pool_min_size = std::env::var("ARW_SQLITE_POOL_MIN")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(2);
        let pool_max_ceiling = std::env::var("ARW_SQLITE_POOL_MAX")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(32)
            .max(pool_min_size);
        let initial_target = std::env::var("ARW_SQLITE_POOL_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(8)
            .clamp(pool_min_size, pool_max_ceiling);
        let conn = Connection::open(&db_path)?;
        Kernel::apply_pragmas(&conn, &pragmas)?;
        if need_init {
            Self::init_schema(&conn)?;
        }
        let pool = Arc::new(PoolShared {
            state: Mutex::new(PoolState {
                conns: vec![conn],
                created: 1,
            }),
            wait_stats: Mutex::new(WaitStats::default()),
            cvar: Condvar::new(),
            target_size: AtomicUsize::new(initial_target),
            min_size: pool_min_size,
            max_ceiling: pool_max_ceiling,
        });
        {
            let guard = pool.state.lock().expect("pool mutex poisoned");
            pool.record_metrics(&guard);
        }
        let blocking = BlockingPool::new(blocking_worker_count())?;
        let mut kernel = Self {
            db_path,
            pragmas,
            pool,
            checkpoint: None,
            autotune: None,
            blocking,
        };
        if let Ok(secs) = std::env::var("ARW_SQLITE_CHECKPOINT_SEC") {
            if let Ok(interval) = secs.parse::<u64>() {
                if interval > 0 {
                    let _ = kernel.start_checkpoint_loop(Duration::from_secs(interval));
                }
            }
        }
        if std::env::var("ARW_SQLITE_POOL_AUTOTUNE")
            .map(|v| v != "0")
            .unwrap_or(false)
        {
            let interval = std::env::var("ARW_SQLITE_POOL_AUTOTUNE_INTERVAL_SEC")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|v| *v > 0)
                .map(Duration::from_secs)
                .unwrap_or_else(|| Duration::from_secs(30));
            let wait_threshold_ms = std::env::var("ARW_SQLITE_POOL_AUTOTUNE_WAIT_MS")
                .ok()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| *v > 0.0)
                .unwrap_or(50.0);
            let _ = kernel.start_autotune_loop(interval, wait_threshold_ms);
        }
        Ok(kernel)
    }

    fn start_checkpoint_loop(&mut self, interval: Duration) -> Result<()> {
        if interval.is_zero() || self.checkpoint.is_some() {
            return Ok(());
        }
        let stop_flag = Arc::new(AtomicBool::new(false));
        let pool_weak: Weak<PoolShared> = Arc::downgrade(&self.pool);
        let db_path = self.db_path.clone();
        let pragmas = self.pragmas.clone();
        let stop_clone = stop_flag.clone();
        let handle = thread::Builder::new()
            .name("arw-kernel-checkpoint".into())
            .spawn(move || loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(interval);
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                let Some(pool) = pool_weak.upgrade() else {
                    break;
                };
                match Kernel::checkout_connection(&db_path, &pragmas, &pool) {
                    Ok(conn) => {
                        #[cfg(feature = "metrics")]
                        metrics::counter!("arw_kernel_checkpoint_runs").increment(1);
                        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                    }
                    Err(_) => {
                        #[cfg(feature = "metrics")]
                        metrics::counter!("arw_kernel_checkpoint_failures").increment(1);
                    }
                }
            })
            .map_err(|e| anyhow!("failed to spawn checkpoint thread: {e}"))?;
        self.checkpoint = Some(Arc::new(CheckpointCtl::new(stop_flag, handle)));
        Ok(())
    }

    fn start_autotune_loop(&mut self, interval: Duration, wait_threshold_ms: f64) -> Result<()> {
        if interval.is_zero() || wait_threshold_ms <= 0.0 || self.autotune.is_some() {
            return Ok(());
        }
        let stop_flag = Arc::new(AtomicBool::new(false));
        let pool_weak: Weak<PoolShared> = Arc::downgrade(&self.pool);
        let stop_clone = stop_flag.clone();
        let handle = thread::Builder::new()
            .name("arw-kernel-autotune".into())
            .spawn(move || loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(interval);
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                let Some(pool) = pool_weak.upgrade() else {
                    break;
                };
                let avg_wait = {
                    let mut stats = pool
                        .wait_stats
                        .lock()
                        .expect("pool wait stats mutex poisoned");
                    let avg = if stats.count > 0 {
                        stats.total_ms / stats.count as f64
                    } else {
                        0.0
                    };
                    stats.count = 0;
                    stats.total_ms = 0.0;
                    avg
                };
                let target = pool.target_size.load(Ordering::Relaxed);
                if avg_wait > wait_threshold_ms && target < pool.max_ceiling {
                    let new_target = (target + 1).min(pool.max_ceiling);
                    pool.target_size.store(new_target, Ordering::Relaxed);
                    #[cfg(feature = "metrics")]
                    metrics::counter!("arw_kernel_pool_autotune_grow").increment(1);
                    continue;
                }
                if avg_wait <= wait_threshold_ms * 0.25 {
                    let available = {
                        let guard = pool.state.lock().expect("pool mutex poisoned");
                        let available = guard.conns.len();
                        pool.record_metrics(&guard);
                        available
                    };
                    let current_target = pool.target_size.load(Ordering::Relaxed);
                    if available >= 2 && current_target > pool.min_size {
                        let new_target = current_target.saturating_sub(1).max(pool.min_size);
                        if new_target < current_target {
                            pool.target_size.store(new_target, Ordering::Relaxed);
                            pool.shrink_to(new_target);
                            #[cfg(feature = "metrics")]
                            metrics::counter!("arw_kernel_pool_autotune_shrink").increment(1);
                        }
                    }
                }
            })
            .map_err(|e| anyhow!("failed to spawn pool autotune thread: {e}"))?;
        self.autotune = Some(Arc::new(AutotuneCtl::new(stop_flag, handle)));
        Ok(())
    }

    fn apply_pragmas(conn: &Connection, pragmas: &KernelPragmas) -> rusqlite::Result<()> {
        conn.pragma_update(None, "journal_mode", &pragmas.journal_mode)?;
        conn.pragma_update(None, "synchronous", &pragmas.synchronous)?;
        conn.busy_timeout(std::time::Duration::from_millis(pragmas.busy_timeout_ms))?;
        let _ = conn.pragma_update(None, "cache_size", pragmas.cache_pages);
        let _ = conn.pragma_update(None, "temp_store", &pragmas.temp_store);
        if let Some(bytes) = pragmas.mmap_bytes {
            let _ = conn.pragma_update(None, "mmap_size", bytes);
        }
        Ok(())
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
            CREATE INDEX IF NOT EXISTS idx_actions_state_created ON actions(state, created);
            CREATE INDEX IF NOT EXISTS idx_actions_updated ON actions(updated);
            CREATE INDEX IF NOT EXISTS idx_actions_idem ON actions(idem_key);

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

            CREATE TABLE IF NOT EXISTS research_watcher_items (
              id TEXT PRIMARY KEY,
              source TEXT,
              source_id TEXT,
              title TEXT,
              summary TEXT,
              url TEXT,
              payload TEXT,
              status TEXT NOT NULL,
              note TEXT,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_research_watcher_source_id ON research_watcher_items(source_id);

            CREATE TABLE IF NOT EXISTS staging_actions (
              id TEXT PRIMARY KEY,
              action_kind TEXT NOT NULL,
              action_input TEXT NOT NULL,
              project TEXT,
              requested_by TEXT,
              evidence TEXT,
              status TEXT NOT NULL,
              decision TEXT,
              decided_by TEXT,
              decided_at TEXT,
              action_id TEXT,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_staging_actions_status ON staging_actions(status);

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
              posture TEXT,
              meta TEXT                     -- JSON blob with extended metadata
            );
            CREATE INDEX IF NOT EXISTS idx_egress_time ON egress_ledger(time);

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

            -- Personas: worldview, traits, and empathy telemetry (feature-gated consumers)
            CREATE TABLE IF NOT EXISTS persona_entries (
              id TEXT PRIMARY KEY,
              owner_kind TEXT NOT NULL,      -- workspace | project | agent
              owner_ref TEXT NOT NULL,       -- identifier for scope (workspace id, project id, etc.)
              name TEXT,
              archetype TEXT,
              traits TEXT,
              preferences TEXT,
              worldview TEXT,
              vibe_profile TEXT,
              calibration TEXT,
              updated TEXT NOT NULL,
              version INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_persona_owner ON persona_entries(owner_kind, owner_ref);

            CREATE TABLE IF NOT EXISTS persona_proposals (
              proposal_id TEXT PRIMARY KEY,
              persona_id TEXT NOT NULL,
              submitted_by TEXT NOT NULL,
              diff TEXT NOT NULL,
              rationale TEXT,
              telemetry_scope TEXT,
              leases_required TEXT,
              status TEXT NOT NULL,
              created TEXT NOT NULL,
              updated TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_persona_proposals_status ON persona_proposals(status);

            CREATE TABLE IF NOT EXISTS persona_history (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              persona_id TEXT NOT NULL,
              proposal_id TEXT,
              diff TEXT NOT NULL,
              applied_by TEXT,
              applied_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_persona_history_persona ON persona_history(persona_id);
            "#,
        )?;
        // Backfill optional columns for older installations (ignore errors if already present)
        let _ = conn.execute("ALTER TABLE egress_ledger ADD COLUMN meta TEXT", []);
        MemoryStore::migrate(conn)?;
        Ok(())
    }

    fn conn(&self) -> Result<ManagedConnection> {
        Self::checkout_connection(&self.db_path, &self.pragmas, &self.pool)
    }

    pub fn session(&self) -> Result<KernelSession> {
        Ok(KernelSession { conn: self.conn()? })
    }

    async fn run_blocking<F, R>(&self, job: F) -> Result<R>
    where
        F: FnOnce(Kernel) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let kernel = self.clone();
        self.blocking.run(move || job(kernel)).await
    }

    fn checkout_connection(
        db_path: &Path,
        pragmas: &Arc<KernelPragmas>,
        pool: &Arc<PoolShared>,
    ) -> Result<ManagedConnection> {
        let mut guard = pool.state.lock().expect("pool mutex poisoned");
        let mut wait_start: Option<Instant> = None;
        loop {
            if let Some(conn) = guard.conns.pop() {
                pool.record_metrics(&guard);
                drop(guard);
                if let Some(start) = wait_start {
                    pool.record_wait(start.elapsed());
                }
                return Ok(ManagedConnection {
                    conn: Some(conn),
                    pool: pool.clone(),
                });
            }
            let target = pool.target_size.load(Ordering::Relaxed);
            if guard.created < target {
                guard.created += 1;
                pool.record_metrics(&guard);
                drop(guard);
                let conn = Connection::open(db_path)?;
                if let Err(e) = Kernel::apply_pragmas(&conn, pragmas) {
                    let mut guard = pool.state.lock().expect("pool mutex poisoned");
                    if guard.created > 0 {
                        guard.created -= 1;
                    }
                    pool.record_metrics(&guard);
                    drop(guard);
                    pool.cvar.notify_one();
                    return Err(anyhow!(e));
                }
                if let Some(start) = wait_start {
                    pool.record_wait(start.elapsed());
                }
                return Ok(ManagedConnection {
                    conn: Some(conn),
                    pool: pool.clone(),
                });
            }
            if wait_start.is_none() {
                wait_start = Some(Instant::now());
            }
            guard = pool.cvar.wait(guard).expect("pool condvar poisoned");
        }
    }

    fn map_event_row(row: &rusqlite::Row) -> rusqlite::Result<EventRow> {
        let id: i64 = row.get(0)?;
        let time: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let actor: Option<String> = row.get(3)?;
        let proj: Option<String> = row.get(4)?;
        let corr_id: Option<String> = row.get(5)?;
        let payload_s: String = row.get(6)?;
        let payload = serde_json::from_str(&payload_s).unwrap_or_else(|_| serde_json::json!({}));
        Ok(EventRow {
            id,
            time,
            kind,
            actor,
            proj,
            corr_id,
            payload,
        })
    }

    pub fn append_event(&self, env: &arw_events::Envelope) -> Result<i64> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare_cached(
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
            stmt_after = conn.prepare_cached(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events WHERE id>? ORDER BY id ASC LIMIT ?",
            )?;
            stmt_after.query(params![aid, limit])?
        } else {
            stmt_all = conn.prepare_cached(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events ORDER BY id DESC LIMIT ?",
            )?;
            stmt_all.query(params![limit])?
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Self::map_event_row(row)?);
        }
        // Ensure ascending order for replay
        if after_id.is_none() {
            out.reverse();
        }
        Ok(out)
    }

    pub fn events_by_corr_id(&self, corr_id: &str, limit: Option<i64>) -> Result<Vec<EventRow>> {
        let conn = self.conn()?;
        let mut stmt_limit;
        let mut stmt_all;
        let mut rows = if let Some(limit) = limit {
            stmt_limit = conn.prepare_cached(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events WHERE corr_id = ? ORDER BY id ASC LIMIT ?",
            )?;
            stmt_limit.query(params![corr_id, limit])?
        } else {
            stmt_all = conn.prepare_cached(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events WHERE corr_id = ? ORDER BY id ASC",
            )?;
            stmt_all.query(params![corr_id])?
        };
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Self::map_event_row(row)?);
        }
        Ok(out)
    }

    pub fn events_by_corr_ids(
        &self,
        corr_ids: &[String],
        limit: Option<i64>,
    ) -> Result<HashMap<String, Vec<EventRow>>> {
        let mut ids: Vec<String> = corr_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn()?;
        let placeholders = ids
            .iter()
            .map(|_| "?".to_string())
            .collect::<Vec<_>>()
            .join(",");
        let base_sql = format!(
            "SELECT id,time,kind,actor,proj,corr_id,payload,\n                    ROW_NUMBER() OVER (PARTITION BY corr_id ORDER BY id ASC) AS rn\n             FROM events\n             WHERE corr_id IN ({})",
            placeholders
        );
        let sql = if limit.is_some() {
            format!(
                "SELECT id,time,kind,actor,proj,corr_id,payload\n                 FROM ({base})\n                 WHERE rn <= ?\n                 ORDER BY corr_id ASC, id ASC",
                base = base_sql
            )
        } else {
            format!(
                "SELECT id,time,kind,actor,proj,corr_id,payload\n                 FROM ({base})\n                 ORDER BY corr_id ASC, id ASC",
                base = base_sql
            )
        };
        let mut params: Vec<Value> = ids.iter().map(|id| Value::from(id.clone())).collect();
        if let Some(limit) = limit {
            params.push(Value::from(limit));
        }
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params.iter()))?;
        let mut grouped: HashMap<String, Vec<EventRow>> = HashMap::with_capacity(ids.len());
        while let Some(row) = rows.next()? {
            let event = Self::map_event_row(row)?;
            if let Some(corr) = event.corr_id.clone() {
                grouped.entry(corr).or_default().push(event);
            }
        }
        Ok(grouped)
    }

    pub fn tail_events(&self, limit: i64, prefixes: &[String]) -> Result<(Vec<EventRow>, i64)> {
        let conn = self.conn()?;
        let sanitized: Vec<String> = prefixes
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let conditions: Vec<String> = (0..sanitized.len())
            .map(|_| "kind LIKE ?".to_string())
            .collect();
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" OR "))
        };
        let like_params: Vec<Value> = sanitized
            .iter()
            .map(|p| Value::from(format!("{}%", p)))
            .collect();
        let count_sql = if where_clause.is_empty() {
            "SELECT COUNT(*) FROM events".to_string()
        } else {
            format!("SELECT COUNT(*) FROM events {}", where_clause)
        };
        let total: i64 =
            conn.query_row(&count_sql, params_from_iter(like_params.iter()), |row| {
                row.get(0)
            })?;
        if limit <= 0 {
            return Ok((Vec::new(), total));
        }
        let mut query_params = like_params.clone();
        query_params.push(Value::from(limit));
        let select_sql = if where_clause.is_empty() {
            "SELECT id,time,kind,actor,proj,corr_id,payload FROM events \
             ORDER BY id DESC LIMIT ?"
                .to_string()
        } else {
            format!(
                "SELECT id,time,kind,actor,proj,corr_id,payload FROM events {} ORDER BY id DESC LIMIT ?",
                where_clause
            )
        };
        let mut stmt = conn.prepare(&select_sql)?;
        let mut rows = stmt.query(params_from_iter(query_params.iter()))?;
        let mut out_desc = Vec::new();
        while let Some(row) = rows.next()? {
            out_desc.push(Self::map_event_row(row)?);
        }
        out_desc.reverse();
        Ok((out_desc, total))
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

    pub fn delete_actions_by_state(&self, state: &str) -> Result<u64> {
        let conn = self.conn()?;
        let n = conn.execute("DELETE FROM actions WHERE state=?", params![state])?;
        Ok(n as u64)
    }

    pub async fn delete_actions_by_state_async(&self, state: &str) -> Result<u64> {
        let state = state.to_string();
        self.run_blocking(move |k| k.delete_actions_by_state(&state))
            .await
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
        let opts = ActionListOptions {
            limit,
            ..Default::default()
        };
        self.list_actions_filtered(&opts)
    }

    pub fn list_actions_filtered(
        &self,
        opts: &ActionListOptions,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut sql = String::from("SELECT id,kind,state,created,updated FROM actions");
        let mut clauses: Vec<&str> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        if let Some(state) = opts
            .state
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            clauses.push("state = ?");
            params.push(Value::Text(state.to_string()))
        }

        if let Some(prefix) = opts
            .kind_prefix
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            clauses.push("kind LIKE ?");
            params.push(Value::Text(format!("{}%", prefix)));
        }

        if let Some(since) = opts
            .updated_since
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            clauses.push("updated >= ?");
            params.push(Value::Text(since.to_string()));
        }

        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }

        sql.push_str(" ORDER BY updated DESC LIMIT ?");
        params.push(Value::Integer(opts.clamped_limit()));

        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
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
        let mut stmt = conn.prepare_cached("SELECT COUNT(1) FROM actions WHERE state=?")?;
        let n: i64 = stmt.query_row([state], |row| row.get(0))?;
        Ok(n)
    }

    pub fn dequeue_one_queued(&self) -> Result<Option<(String, String, serde_json::Value)>> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut stmt = conn.prepare_cached(
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

    // ---------- Research watcher ----------

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_research_watcher_item(
        &self,
        source: Option<&str>,
        source_id: Option<&str>,
        title: Option<&str>,
        summary: Option<&str>,
        url: Option<&str>,
        payload: Option<&serde_json::Value>,
    ) -> Result<String> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let payload_s = payload.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        let existing_id: Option<String> = if let Some(src_id) = source_id {
            conn.query_row(
                "SELECT id FROM research_watcher_items WHERE source_id = ? LIMIT 1",
                params![src_id],
                |r| r.get(0),
            )
            .optional()?
        } else {
            None
        };
        let (id, existed) = if let Some(existing) = existing_id {
            (existing, true)
        } else {
            (uuid::Uuid::new_v4().to_string(), false)
        };
        if existed {
            conn.execute(
                "UPDATE research_watcher_items SET source=?, title=?, summary=?, url=?, payload=?, updated=? WHERE id=?",
                params![source, title, summary, url, payload_s, now, id],
            )?;
        } else {
            conn.execute(
                "INSERT INTO research_watcher_items(id,source,source_id,title,summary,url,payload,status,note,created,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?)",
                params![
                    id,
                    source,
                    source_id,
                    title,
                    summary,
                    url,
                    payload_s,
                    "pending",
                    Option::<String>::None,
                    now.clone(),
                    now
                ],
            )?;
        }
        Ok(id)
    }

    pub fn list_research_watcher_items(
        &self,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let mut out = Vec::new();
        if let Some(stat) = status {
            let mut stmt = conn.prepare(
                "SELECT id,source,source_id,title,summary,url,payload,status,note,created,updated FROM research_watcher_items WHERE status=? ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![stat, limit])?;
            while let Some(r) = rows.next()? {
                let payload_s: Option<String> = r.get(6)?;
                let payload_v = payload_s
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source": r.get::<_, Option<String>>(1)?,
                    "source_id": r.get::<_, Option<String>>(2)?,
                    "title": r.get::<_, Option<String>>(3)?,
                    "summary": r.get::<_, Option<String>>(4)?,
                    "url": r.get::<_, Option<String>>(5)?,
                    "payload": payload_v,
                    "status": r.get::<_, String>(7)?,
                    "note": r.get::<_, Option<String>>(8)?,
                    "created": r.get::<_, String>(9)?,
                    "updated": r.get::<_, String>(10)?
                }));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id,source,source_id,title,summary,url,payload,status,note,created,updated FROM research_watcher_items ORDER BY updated DESC LIMIT ?",
            )?;
            let mut rows = stmt.query([limit])?;
            while let Some(r) = rows.next()? {
                let payload_s: Option<String> = r.get(6)?;
                let payload_v = payload_s
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "source": r.get::<_, Option<String>>(1)?,
                    "source_id": r.get::<_, Option<String>>(2)?,
                    "title": r.get::<_, Option<String>>(3)?,
                    "summary": r.get::<_, Option<String>>(4)?,
                    "url": r.get::<_, Option<String>>(5)?,
                    "payload": payload_v,
                    "status": r.get::<_, String>(7)?,
                    "note": r.get::<_, Option<String>>(8)?,
                    "created": r.get::<_, String>(9)?,
                    "updated": r.get::<_, String>(10)?
                }));
            }
        }
        Ok(out)
    }

    pub fn update_research_watcher_status(
        &self,
        id: &str,
        status: &str,
        note: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let n = conn.execute(
            "UPDATE research_watcher_items SET status=?, note=?, updated=? WHERE id=?",
            params![status, note, now, id],
        )?;
        Ok(n > 0)
    }

    pub fn get_research_watcher_item(&self, id: &str) -> Result<Option<ResearchWatcherItem>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,source,source_id,title,summary,url,payload,status,note,created,updated FROM research_watcher_items WHERE id=? LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(r) = rows.next()? {
            let payload_s: Option<String> = r.get(6)?;
            let payload_v =
                payload_s.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
            Ok(Some(ResearchWatcherItem {
                id: r.get(0)?,
                source: r.get(1)?,
                source_id: r.get(2)?,
                title: r.get(3)?,
                summary: r.get(4)?,
                url: r.get(5)?,
                payload: payload_v,
                status: r.get(7)?,
                note: r.get(8)?,
                created: r.get(9)?,
                updated: r.get(10)?,
            }))
        } else {
            Ok(None)
        }
    }

    // ---------- Staging actions ----------

    #[allow(clippy::too_many_arguments)]
    pub fn insert_staging_action(
        &self,
        action_kind: &str,
        action_input: &serde_json::Value,
        project: Option<&str>,
        requested_by: Option<&str>,
        evidence: Option<&serde_json::Value>,
    ) -> Result<String> {
        let conn = self.conn()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let input_s = serde_json::to_string(action_input).unwrap_or("{}".into());
        let evidence_s = evidence.map(|v| serde_json::to_string(v).unwrap_or("{}".into()));
        conn.execute(
            "INSERT INTO staging_actions(id,action_kind,action_input,project,requested_by,evidence,status,decision,decided_by,decided_at,action_id,created,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                id,
                action_kind,
                input_s,
                project,
                requested_by,
                evidence_s,
                "pending",
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None,
                now.clone(),
                now
            ],
        )?;
        Ok(id)
    }

    pub fn list_staging_actions(
        &self,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let mut out = Vec::new();
        if let Some(stat) = status {
            let mut stmt = conn.prepare(
                "SELECT id,action_kind,action_input,project,requested_by,evidence,status,decision,decided_by,decided_at,action_id,created,updated FROM staging_actions WHERE status=? ORDER BY created ASC LIMIT ?",
            )?;
            let mut rows = stmt.query(params![stat, limit])?;
            while let Some(r) = rows.next()? {
                let input_s: String = r.get(2)?;
                let evidence_s: Option<String> = r.get(5)?;
                let input_v = serde_json::from_str::<serde_json::Value>(&input_s)
                    .unwrap_or(serde_json::json!({}));
                let evidence_v = evidence_s
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "action_kind": r.get::<_, String>(1)?,
                    "action_input": input_v,
                    "project": r.get::<_, Option<String>>(3)?,
                    "requested_by": r.get::<_, Option<String>>(4)?,
                    "evidence": evidence_v,
                    "status": r.get::<_, String>(6)?,
                    "decision": r.get::<_, Option<String>>(7)?,
                    "decided_by": r.get::<_, Option<String>>(8)?,
                    "decided_at": r.get::<_, Option<String>>(9)?,
                    "action_id": r.get::<_, Option<String>>(10)?,
                    "created": r.get::<_, String>(11)?,
                    "updated": r.get::<_, String>(12)?
                }));
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id,action_kind,action_input,project,requested_by,evidence,status,decision,decided_by,decided_at,action_id,created,updated FROM staging_actions ORDER BY created ASC LIMIT ?",
            )?;
            let mut rows = stmt.query([limit])?;
            while let Some(r) = rows.next()? {
                let input_s: String = r.get(2)?;
                let evidence_s: Option<String> = r.get(5)?;
                let input_v = serde_json::from_str::<serde_json::Value>(&input_s)
                    .unwrap_or(serde_json::json!({}));
                let evidence_v = evidence_s
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .unwrap_or(serde_json::json!({}));
                out.push(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "action_kind": r.get::<_, String>(1)?,
                    "action_input": input_v,
                    "project": r.get::<_, Option<String>>(3)?,
                    "requested_by": r.get::<_, Option<String>>(4)?,
                    "evidence": evidence_v,
                    "status": r.get::<_, String>(6)?,
                    "decision": r.get::<_, Option<String>>(7)?,
                    "decided_by": r.get::<_, Option<String>>(8)?,
                    "decided_at": r.get::<_, Option<String>>(9)?,
                    "action_id": r.get::<_, Option<String>>(10)?,
                    "created": r.get::<_, String>(11)?,
                    "updated": r.get::<_, String>(12)?
                }));
            }
        }
        Ok(out)
    }

    pub fn get_staging_action(&self, id: &str) -> Result<Option<StagingAction>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,action_kind,action_input,project,requested_by,evidence,status,decision,decided_by,decided_at,action_id,created,updated FROM staging_actions WHERE id=? LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(r) = rows.next()? {
            let input_s: String = r.get(2)?;
            let evidence_s: Option<String> = r.get(5)?;
            let input_v = serde_json::from_str::<serde_json::Value>(&input_s)
                .unwrap_or(serde_json::json!({}));
            let evidence_v =
                evidence_s.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
            Ok(Some(StagingAction {
                id: r.get(0)?,
                action_kind: r.get(1)?,
                action_input: input_v,
                project: r.get(3)?,
                requested_by: r.get(4)?,
                evidence: evidence_v,
                status: r.get(6)?,
                decision: r.get(7)?,
                decided_by: r.get(8)?,
                decided_at: r.get(9)?,
                action_id: r.get(10)?,
                created: r.get(11)?,
                updated: r.get(12)?,
            }))
        } else {
            Ok(None)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_staging_action_status(
        &self,
        id: &str,
        status: &str,
        decision: Option<&str>,
        decided_by: Option<&str>,
        decided_at: Option<&str>,
        action_id: Option<&str>,
    ) -> Result<bool> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let decided_ts = decided_at.map(|s| s.to_string());
        let n = conn.execute(
            "UPDATE staging_actions SET status=?, decision=?, decided_by=?, decided_at=COALESCE(?,decided_at), action_id=?, updated=? WHERE id=?",
            params![status, decision, decided_by, decided_ts, action_id, now, id],
        )?;
        Ok(n > 0)
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
        let s = subject.to_string();
        let c = capability.to_string();
        self.run_blocking(move |k| k.find_valid_lease(&s, &c)).await
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
        meta: Option<&serde_json::Value>,
    ) -> Result<i64> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let meta_s = meta.and_then(|v| serde_json::to_string(v).ok());
        conn.execute(
            "INSERT INTO egress_ledger(time,decision,reason,dest_host,dest_port,protocol,bytes_in,bytes_out,corr_id,proj,posture,meta) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)",
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
                posture,
                meta_s
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_egress(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,time,decision,reason,dest_host,dest_port,protocol,bytes_in,bytes_out,corr_id,proj,posture,meta FROM egress_ledger ORDER BY id DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let meta: Option<String> = r.get(12)?;
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
                "meta": meta.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
            }));
        }
        Ok(out)
    }

    pub fn insert_memory(&self, args: &MemoryInsertArgs<'_>) -> Result<String> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.insert_memory(args)
    }

    pub fn insert_memory_with_record(
        &self,
        args: &MemoryInsertArgs<'_>,
    ) -> Result<(String, serde_json::Value)> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.insert_memory_with_record(args)
    }

    pub fn search_memory(
        &self,
        q: &str,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.search_memory(q, lane, limit)
    }

    pub fn fts_search_memory(
        &self,
        q: &str,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.fts_search_memory(q, lane, limit)
    }

    pub fn search_memory_by_embedding(
        &self,
        embed: &[f32],
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.search_memory_by_embedding(embed, lane, limit)
    }

    pub fn select_memory_hybrid(
        &self,
        q: Option<&str>,
        embed: Option<&[f32]>,
        lane: Option<&str>,
        k: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.select_memory_hybrid(q, embed, lane, k)
    }

    pub fn insert_memory_link(
        &self,
        src_id: &str,
        dst_id: &str,
        rel: Option<&str>,
        weight: Option<f64>,
    ) -> Result<()> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.insert_memory_link(src_id, dst_id, rel, weight)
    }

    pub fn list_memory_links(&self, src_id: &str, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.list_memory_links(src_id, limit)
    }

    pub fn list_memory_links_many(
        &self,
        src_ids: &[String],
        limit_per: i64,
    ) -> Result<HashMap<String, Vec<serde_json::Value>>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.list_memory_links_many(src_ids, limit_per)
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.get_memory(id)
    }

    pub fn get_memory_many(&self, ids: &[String]) -> Result<HashMap<String, serde_json::Value>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.get_memory_many(ids)
    }

    pub fn find_memory_by_hash(&self, hash: &str) -> Result<Option<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.find_memory_by_hash(hash)
    }

    pub fn backfill_embed_blobs(&self, batch_limit: usize) -> Result<usize> {
        if batch_limit == 0 {
            return Ok(0);
        }
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.backfill_embed_blobs(batch_limit)
    }

    pub fn pending_embed_backfill(&self) -> Result<u64> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.pending_embed_backfill()
    }

    pub fn expired_memory_candidates(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.expired_candidates(now, limit)
    }

    pub fn lane_overflow_candidates(
        &self,
        lane: &str,
        cap: usize,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.lane_overflow_candidates(lane, cap, limit)
    }

    pub fn delete_memory_records(&self, ids: &[String]) -> Result<usize> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.delete_records(ids)
    }

    pub fn list_recent_memory(
        &self,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let store = MemoryStore::new(&conn);
        store.list_recent_memory(lane, limit)
    }

    pub fn pool_wait_stats(&self) -> (u64, f64) {
        let stats = self
            .pool
            .wait_stats
            .lock()
            .expect("pool wait stats mutex poisoned");
        (stats.count, stats.total_ms)
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
        data_patch: Option<&serde_json::Value>,
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
        let mut merged_data: Option<String> = None;
        if let Some(patch) = data_patch {
            if patch.is_object() {
                let existing: Option<String> = conn.query_row(
                    "SELECT data FROM orchestrator_jobs WHERE id=? LIMIT 1",
                    [id],
                    |row| row.get(0),
                )?;
                let mut base = existing
                    .as_ref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if !base.is_object() {
                    base = serde_json::json!({});
                }
                if let serde_json::Value::Object(ref mut base_map) = base {
                    if let serde_json::Value::Object(ref patch_map) = patch {
                        for (key, value) in patch_map.iter() {
                            base_map.insert(key.clone(), value.clone());
                        }
                    }
                }
                merged_data = Some(serde_json::to_string(&base).unwrap_or_else(|_| "{}".into()));
            } else if !patch.is_null() {
                merged_data =
                    Some(serde_json::to_string(patch).unwrap_or_else(|_| "{}".to_string()));
            }
            if merged_data.is_some() {
                set_parts.push("data=?");
            }
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
        if let Some(data) = merged_data {
            params_vec.push(rusqlite::types::Value::from(data));
        }
        params_vec.push(rusqlite::types::Value::from(now.clone()));
        params_vec.push(rusqlite::types::Value::from(id.to_string()));
        let n = stmt.execute(rusqlite::params_from_iter(params_vec))?;
        Ok(n > 0)
    }

    pub fn list_orchestrator_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id,status,goal,data,progress,created,updated FROM orchestrator_jobs ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let status_raw: String = r.get::<_, String>(1)?;
            let (status_slug, status_label) = Self::normalize_orchestrator_status(&status_raw);
            let mut payload = serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "status": status_raw,
                "status_slug": status_slug,
                "status_label": status_label,
                "goal": r.get::<_, Option<String>>(2)?,
                "progress": r.get::<_, Option<f64>>(4)?,
                "created": r.get::<_, String>(5)?,
                "updated": r.get::<_, String>(6)?,
            });
            let data_raw: Option<String> = r.get(3)?;
            if let Some(data_raw) = data_raw {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_raw) {
                    if !val.is_null() {
                        if let serde_json::Value::Object(ref mut map) = payload {
                            map.insert("data".into(), val);
                        }
                    }
                }
            }
            out.push(payload);
        }
        Ok(out)
    }

    fn normalize_orchestrator_status(value: &str) -> (&'static str, &'static str) {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "queued" | "pending" | "waiting" => ("queued", "Queued"),
            "running" | "in_progress" | "in-progress" | "started" | "active" => {
                ("running", "Running")
            }
            "completed" | "complete" | "finished" | "done" | "success" | "succeeded" => {
                ("completed", "Completed")
            }
            "failed" | "error" | "errored" | "fail" | "failure" => ("failed", "Failed"),
            "cancelled" | "canceled" | "aborted" | "stopped" => ("cancelled", "Cancelled"),
            "unknown" | "" => ("unknown", "Unknown"),
            other if other.starts_with("run") => ("running", "Running"),
            other if other.starts_with("queue") => ("queued", "Queued"),
            other if other.starts_with("wait") => ("queued", "Queued"),
            other if other.starts_with("fail") => ("failed", "Failed"),
            other if other.starts_with("cancel") => ("cancelled", "Cancelled"),
            other if other.starts_with("complete") => ("completed", "Completed"),
            _ => ("unknown", "Unknown"),
        }
    }

    // ---------- Personas ----------

    pub fn upsert_persona_entry(&self, upsert: PersonaEntryUpsert) -> Result<PersonaEntry> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let existing_version: Option<i64> = conn
            .query_row(
                "SELECT version FROM persona_entries WHERE id=? LIMIT 1",
                [&upsert.id],
                |row| row.get(0),
            )
            .optional()?;
        let version = existing_version.unwrap_or(0).saturating_add(1);

        let traits_s = serde_json::to_string(&upsert.traits).unwrap_or_else(|_| "{}".into());
        let preferences_s =
            serde_json::to_string(&upsert.preferences).unwrap_or_else(|_| "{}".into());
        let worldview_s = serde_json::to_string(&upsert.worldview).unwrap_or_else(|_| "{}".into());
        let vibe_profile_s =
            serde_json::to_string(&upsert.vibe_profile).unwrap_or_else(|_| "{}".into());
        let calibration_s =
            serde_json::to_string(&upsert.calibration).unwrap_or_else(|_| "{}".into());

        conn.execute(
            "INSERT INTO persona_entries \
                (id, owner_kind, owner_ref, name, archetype, traits, preferences, worldview, vibe_profile, calibration, updated, version) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET \
                owner_kind=excluded.owner_kind, \
                owner_ref=excluded.owner_ref, \
                name=excluded.name, \
                archetype=excluded.archetype, \
                traits=excluded.traits, \
                preferences=excluded.preferences, \
                worldview=excluded.worldview, \
                vibe_profile=excluded.vibe_profile, \
                calibration=excluded.calibration, \
                updated=excluded.updated, \
                version=excluded.version",
            params![
                upsert.id,
                upsert.owner_kind,
                upsert.owner_ref,
                upsert.name,
                upsert.archetype,
                traits_s,
                preferences_s,
                worldview_s,
                vibe_profile_s,
                calibration_s,
                now,
                version
            ],
        )?;

        self.get_persona_entry(&upsert.id)?
            .ok_or_else(|| anyhow!("persona entry not found after upsert"))
    }

    pub fn get_persona_entry(&self, id: &str) -> Result<Option<PersonaEntry>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, owner_kind, owner_ref, name, archetype, traits, preferences, worldview, vibe_profile, calibration, updated, version \
             FROM persona_entries WHERE id=? LIMIT 1",
        )?;
        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::map_persona_entry_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_persona_entries(
        &self,
        owner_filter: Option<(&str, &str)>,
        limit: i64,
    ) -> Result<Vec<PersonaEntry>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let mut entries = Vec::new();
        match owner_filter {
            Some((owner_kind, owner_ref)) => {
                let mut stmt = conn.prepare(
                    "SELECT id, owner_kind, owner_ref, name, archetype, traits, preferences, worldview, vibe_profile, calibration, updated, version \
                     FROM persona_entries \
                     WHERE owner_kind=? AND owner_ref=? \
                     ORDER BY updated DESC LIMIT ?",
                )?;
                let mut rows = stmt.query(params![owner_kind, owner_ref, limit])?;
                while let Some(row) = rows.next()? {
                    entries.push(Self::map_persona_entry_row(row)?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT id, owner_kind, owner_ref, name, archetype, traits, preferences, worldview, vibe_profile, calibration, updated, version \
                     FROM persona_entries \
                     ORDER BY updated DESC LIMIT ?",
                )?;
                let mut rows = stmt.query([limit])?;
                while let Some(row) = rows.next()? {
                    entries.push(Self::map_persona_entry_row(row)?);
                }
            }
        };
        Ok(entries)
    }

    pub fn insert_persona_proposal(&self, create: PersonaProposalCreate) -> Result<String> {
        let conn = self.conn()?;
        let proposal_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let diff_s = serde_json::to_string(&create.diff).unwrap_or_else(|_| "[]".into());
        let telemetry_scope_s =
            serde_json::to_string(&create.telemetry_scope).unwrap_or_else(|_| "{}".into());
        let leases_required_s =
            serde_json::to_string(&create.leases_required).unwrap_or_else(|_| "[]".into());

        conn.execute(
            "INSERT INTO persona_proposals \
                (proposal_id, persona_id, submitted_by, diff, rationale, telemetry_scope, leases_required, status, created, updated) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                proposal_id,
                create.persona_id,
                create.submitted_by,
                diff_s,
                create.rationale,
                telemetry_scope_s,
                leases_required_s,
                "pending",
                &now,
                &now
            ],
        )?;
        Ok(proposal_id)
    }

    pub fn get_persona_proposal(&self, proposal_id: &str) -> Result<Option<PersonaProposal>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT proposal_id, persona_id, submitted_by, diff, rationale, telemetry_scope, leases_required, status, created, updated \
             FROM persona_proposals WHERE proposal_id=? LIMIT 1",
        )?;
        let mut rows = stmt.query([proposal_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::map_persona_proposal_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_persona_proposal_status(
        &self,
        proposal_id: &str,
        update: PersonaProposalStatusUpdate,
    ) -> Result<bool> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let affected = conn.execute(
            "UPDATE persona_proposals SET status=?, updated=? WHERE proposal_id=?",
            params![update.status, now, proposal_id],
        )?;
        Ok(affected > 0)
    }

    pub fn list_persona_proposals(
        &self,
        persona_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
    ) -> Result<Vec<PersonaProposal>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let mut proposals = Vec::new();
        let mut query = String::from(
            "SELECT proposal_id, persona_id, submitted_by, diff, rationale, telemetry_scope, leases_required, status, created, updated \
             FROM persona_proposals",
        );
        let mut conditions: Vec<&str> = Vec::new();
        if persona_id.is_some() {
            conditions.push("persona_id=?");
        }
        if status.is_some() {
            conditions.push("status=?");
        }
        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" AND "));
        }
        query.push_str(" ORDER BY updated DESC LIMIT ?");

        let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(pid) = persona_id {
            params_vec.push(Value::from(pid.to_string()));
        }
        if let Some(status_val) = status {
            params_vec.push(Value::from(status_val.to_string()));
        }
        params_vec.push(limit.into());

        let mut stmt = conn.prepare(&query)?;
        let mut rows = stmt.query(params_from_iter(params_vec))?;
        while let Some(row) = rows.next()? {
            proposals.push(Self::map_persona_proposal_row(row)?);
        }
        Ok(proposals)
    }

    pub fn append_persona_history(&self, append: PersonaHistoryAppend) -> Result<i64> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let diff_s = serde_json::to_string(&append.diff).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO persona_history (persona_id, proposal_id, diff, applied_by, applied_at) VALUES (?, ?, ?, ?, ?)",
            params![
                append.persona_id,
                append.proposal_id,
                diff_s,
                append.applied_by,
                now
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_persona_history(
        &self,
        persona_id: &str,
        limit: i64,
    ) -> Result<Vec<PersonaHistoryEntry>> {
        let conn = self.conn()?;
        let limit = limit.clamp(1, 500);
        let mut stmt = conn.prepare(
            "SELECT id, persona_id, proposal_id, diff, applied_by, applied_at \
             FROM persona_history WHERE persona_id=? ORDER BY applied_at DESC LIMIT ?",
        )?;
        let mut rows = stmt.query(params![persona_id, limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(Self::map_persona_history_row(row)?);
        }
        Ok(out)
    }

    pub async fn upsert_persona_entry_async(
        &self,
        upsert: PersonaEntryUpsert,
    ) -> Result<PersonaEntry> {
        self.run_blocking(move |kernel| kernel.upsert_persona_entry(upsert))
            .await
    }

    pub async fn get_persona_entry_async(&self, id: String) -> Result<Option<PersonaEntry>> {
        self.run_blocking(move |kernel| kernel.get_persona_entry(&id))
            .await
    }

    pub async fn list_persona_entries_async(
        &self,
        owner_kind: Option<String>,
        owner_ref: Option<String>,
        limit: i64,
    ) -> Result<Vec<PersonaEntry>> {
        self.run_blocking(move |kernel| {
            let owner_filter = match (owner_kind.as_deref(), owner_ref.as_deref()) {
                (Some(kind), Some(r)) => Some((kind, r)),
                _ => None,
            };
            kernel.list_persona_entries(owner_filter, limit)
        })
        .await
    }

    pub async fn insert_persona_proposal_async(
        &self,
        create: PersonaProposalCreate,
    ) -> Result<String> {
        self.run_blocking(move |kernel| kernel.insert_persona_proposal(create))
            .await
    }

    pub async fn get_persona_proposal_async(
        &self,
        proposal_id: String,
    ) -> Result<Option<PersonaProposal>> {
        self.run_blocking(move |kernel| kernel.get_persona_proposal(&proposal_id))
            .await
    }

    pub async fn update_persona_proposal_status_async(
        &self,
        proposal_id: String,
        status: PersonaProposalStatusUpdate,
    ) -> Result<bool> {
        self.run_blocking(move |kernel| kernel.update_persona_proposal_status(&proposal_id, status))
            .await
    }

    pub async fn list_persona_proposals_async(
        &self,
        persona_id: Option<String>,
        status: Option<String>,
        limit: i64,
    ) -> Result<Vec<PersonaProposal>> {
        self.run_blocking(move |kernel| {
            kernel.list_persona_proposals(persona_id.as_deref(), status.as_deref(), limit)
        })
        .await
    }

    pub async fn append_persona_history_async(&self, entry: PersonaHistoryAppend) -> Result<i64> {
        self.run_blocking(move |kernel| kernel.append_persona_history(entry))
            .await
    }

    pub async fn list_persona_history_async(
        &self,
        persona_id: String,
        limit: i64,
    ) -> Result<Vec<PersonaHistoryEntry>> {
        self.run_blocking(move |kernel| kernel.list_persona_history(&persona_id, limit))
            .await
    }

    pub async fn apply_persona_diff_async(
        &self,
        persona_id: String,
        diff: JsonValue,
    ) -> Result<PersonaEntry> {
        self.run_blocking(move |kernel| kernel.apply_persona_diff(&persona_id, &diff))
            .await
    }

    fn map_persona_entry_row(row: &rusqlite::Row<'_>) -> Result<PersonaEntry> {
        let traits_raw: Option<String> = row.get(5)?;
        let preferences_raw: Option<String> = row.get(6)?;
        let worldview_raw: Option<String> = row.get(7)?;
        let vibe_raw: Option<String> = row.get(8)?;
        let calibration_raw: Option<String> = row.get(9)?;
        Ok(PersonaEntry {
            id: row.get(0)?,
            owner_kind: row.get(1)?,
            owner_ref: row.get(2)?,
            name: row.get(3)?,
            archetype: row.get(4)?,
            traits: parse_json_or_default(traits_raw, json!({})),
            preferences: parse_json_or_default(preferences_raw, json!({})),
            worldview: parse_json_or_default(worldview_raw, json!({})),
            vibe_profile: parse_json_or_default(vibe_raw, json!({})),
            calibration: parse_json_or_default(calibration_raw, json!({})),
            updated: row.get(10)?,
            version: row.get(11)?,
        })
    }

    fn map_persona_proposal_row(row: &rusqlite::Row<'_>) -> Result<PersonaProposal> {
        let diff_raw: Option<String> = row.get(3)?;
        let telemetry_raw: Option<String> = row.get(5)?;
        let leases_raw: Option<String> = row.get(6)?;
        Ok(PersonaProposal {
            proposal_id: row.get(0)?,
            persona_id: row.get(1)?,
            submitted_by: row.get(2)?,
            diff: parse_json_or_default(diff_raw, json!([])),
            rationale: row.get(4)?,
            telemetry_scope: parse_json_or_default(telemetry_raw, json!({})),
            leases_required: parse_json_or_default(leases_raw, json!([])),
            status: row.get(7)?,
            created: row.get(8)?,
            updated: row.get(9)?,
        })
    }

    fn map_persona_history_row(row: &rusqlite::Row<'_>) -> Result<PersonaHistoryEntry> {
        let diff_raw: Option<String> = row.get(3)?;
        Ok(PersonaHistoryEntry {
            id: row.get(0)?,
            persona_id: row.get(1)?,
            proposal_id: row.get(2)?,
            diff: parse_json_or_default(diff_raw, json!([])),
            applied_by: row.get(4)?,
            applied_at: row.get(5)?,
        })
    }

    pub fn apply_persona_diff(&self, persona_id: &str, diff: &JsonValue) -> Result<PersonaEntry> {
        let entry = self
            .get_persona_entry(persona_id)?
            .ok_or_else(|| anyhow!("persona id not found"))?;
        let mut entry_value = serde_json::to_value(&entry)?;

        if diff.is_array() {
            let patch: json_patch::Patch = serde_json::from_value(diff.clone())?;
            json_patch::patch(&mut entry_value, &patch)?;
        } else if diff.is_object() {
            merge_json(&mut entry_value, diff);
        } else {
            return Err(anyhow!("persona diff must be a JSON object or array"));
        }

        let mut updated: PersonaEntry = serde_json::from_value(entry_value)?;

        // preserve immutable fields
        updated.id = entry.id.clone();
        updated.owner_kind = entry.owner_kind.clone();
        updated.owner_ref = entry.owner_ref.clone();

        // ensure required JSON fields remain objects
        if !updated.traits.is_object() && !updated.traits.is_array() {
            updated.traits = entry.traits.clone();
        }
        if !updated.preferences.is_object() && !updated.preferences.is_array() {
            updated.preferences = entry.preferences.clone();
        }
        if !updated.worldview.is_object() && !updated.worldview.is_array() {
            updated.worldview = entry.worldview.clone();
        }
        if !updated.vibe_profile.is_object() && !updated.vibe_profile.is_array() {
            updated.vibe_profile = entry.vibe_profile.clone();
        }
        if !updated.calibration.is_object() && !updated.calibration.is_array() {
            updated.calibration = entry.calibration.clone();
        }

        let upsert = PersonaEntryUpsert {
            id: updated.id.clone(),
            owner_kind: updated.owner_kind.clone(),
            owner_ref: updated.owner_ref.clone(),
            name: updated.name.clone(),
            archetype: updated.archetype.clone(),
            traits: updated.traits.clone(),
            preferences: updated.preferences.clone(),
            worldview: updated.worldview.clone(),
            vibe_profile: updated.vibe_profile.clone(),
            calibration: updated.calibration.clone(),
        };

        self.upsert_persona_entry(upsert)
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

    // ---------------- Async wrappers (blocking pool) ----------------
    // These helpers offload rusqlite work onto the dedicated blocking pool.

    pub async fn insert_memory_async(&self, owned: MemoryInsertOwned) -> Result<String> {
        self.run_blocking(move |k| {
            let args = owned.to_args();
            k.insert_memory(&args)
        })
        .await
    }

    pub async fn insert_memory_with_record_async(
        &self,
        owned: MemoryInsertOwned,
    ) -> Result<(String, serde_json::Value)> {
        self.run_blocking(move |k| {
            let args = owned.to_args();
            k.insert_memory_with_record(&args)
        })
        .await
    }

    pub async fn search_memory_async(
        &self,
        q: String,
        lane: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.search_memory(&q, lane.as_deref(), limit))
            .await
    }

    pub async fn fts_search_memory_async(
        &self,
        q: String,
        lane: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.fts_search_memory(&q, lane.as_deref(), limit))
            .await
    }

    pub async fn search_memory_by_embedding_async(
        &self,
        embed: Vec<f32>,
        lane: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.search_memory_by_embedding(&embed, lane.as_deref(), limit))
            .await
    }

    pub async fn select_memory_hybrid_async(
        &self,
        q: Option<String>,
        embed: Option<Vec<f32>>,
        lane: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| {
            k.select_memory_hybrid(q.as_deref(), embed.as_deref(), lane.as_deref(), limit)
        })
        .await
    }

    pub async fn list_recent_memory_async(
        &self,
        lane: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_recent_memory(lane.as_deref(), limit))
            .await
    }

    pub async fn find_memory_by_hash_async(
        &self,
        hash: String,
    ) -> Result<Option<serde_json::Value>> {
        self.run_blocking(move |k| k.find_memory_by_hash(&hash))
            .await
    }

    pub async fn expired_memory_candidates_async(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        self.run_blocking(move |k| k.expired_memory_candidates(now, limit))
            .await
    }

    pub async fn lane_overflow_candidates_async(
        &self,
        lane: String,
        cap: usize,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        self.run_blocking(move |k| k.lane_overflow_candidates(&lane, cap, limit))
            .await
    }

    pub async fn delete_memory_records_async(&self, ids: Vec<String>) -> Result<usize> {
        self.run_blocking(move |k| k.delete_memory_records(&ids))
            .await
    }

    pub async fn insert_memory_link_async(
        &self,
        src_id: String,
        dst_id: String,
        rel: Option<String>,
        weight: Option<f64>,
    ) -> Result<()> {
        self.run_blocking(move |k| k.insert_memory_link(&src_id, &dst_id, rel.as_deref(), weight))
            .await
    }

    pub async fn backfill_embed_blobs_async(&self, batch_limit: usize) -> Result<usize> {
        if batch_limit == 0 {
            return Ok(0);
        }
        self.run_blocking(move |k| k.backfill_embed_blobs(batch_limit))
            .await
    }

    pub async fn pending_embed_backfill_async(&self) -> Result<u64> {
        self.run_blocking(|k| k.pending_embed_backfill()).await
    }

    pub async fn list_memory_links_async(
        &self,
        src_id: String,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_memory_links(&src_id, limit))
            .await
    }

    pub async fn list_memory_links_many_async(
        &self,
        src_ids: Vec<String>,
        limit_per: i64,
    ) -> Result<HashMap<String, Vec<serde_json::Value>>> {
        self.run_blocking(move |k| k.list_memory_links_many(&src_ids, limit_per))
            .await
    }

    pub async fn get_memory_async(&self, id: String) -> Result<Option<serde_json::Value>> {
        self.run_blocking(move |k| k.get_memory(&id)).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_lease_async(
        &self,
        id: String,
        subject: String,
        capability: String,
        scope: Option<String>,
        ttl_until: String,
        budget: Option<f64>,
        policy_ctx: Option<serde_json::Value>,
    ) -> Result<()> {
        self.run_blocking(move |k| {
            k.insert_lease(
                &id,
                &subject,
                &capability,
                scope.as_deref(),
                &ttl_until,
                budget,
                policy_ctx.as_ref(),
            )
        })
        .await
    }

    pub async fn list_leases_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_leases(limit)).await
    }

    pub async fn insert_config_snapshot_async(&self, config: serde_json::Value) -> Result<String> {
        self.run_blocking(move |k| k.insert_config_snapshot(&config))
            .await
    }

    pub async fn get_config_snapshot_async(&self, id: String) -> Result<Option<serde_json::Value>> {
        self.run_blocking(move |k| k.get_config_snapshot(&id)).await
    }

    pub async fn list_config_snapshots_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_config_snapshots(limit))
            .await
    }

    pub async fn insert_logic_unit_async(
        &self,
        id: String,
        manifest: serde_json::Value,
        status: String,
    ) -> Result<()> {
        self.run_blocking(move |k| k.insert_logic_unit(&id, &manifest, &status))
            .await
    }

    pub async fn list_logic_units_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_logic_units(limit)).await
    }

    pub async fn insert_orchestrator_job_async(
        &self,
        goal: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<String> {
        let goal_owned = goal.to_string();
        let data_clone = data.cloned();
        self.run_blocking(move |k| k.insert_orchestrator_job(&goal_owned, data_clone.as_ref()))
            .await
    }

    pub async fn update_orchestrator_job_async(
        &self,
        id: String,
        status: Option<String>,
        progress: Option<f64>,
        data_patch: Option<serde_json::Value>,
    ) -> Result<bool> {
        self.run_blocking(move |k| {
            k.update_orchestrator_job(&id, status.as_deref(), progress, data_patch.as_ref())
        })
        .await
    }

    pub async fn list_orchestrator_jobs_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_orchestrator_jobs(limit))
            .await
    }

    pub async fn update_action_result_async(
        &self,
        id: String,
        output: Option<serde_json::Value>,
        error: Option<String>,
    ) -> Result<bool> {
        self.run_blocking(move |k| k.update_action_result(&id, output.as_ref(), error.as_deref()))
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn append_egress_async(
        &self,
        decision: String,
        reason: Option<String>,
        dest_host: Option<String>,
        dest_port: Option<i64>,
        protocol: Option<String>,
        bytes_in: Option<i64>,
        bytes_out: Option<i64>,
        corr_id: Option<String>,
        proj: Option<String>,
        posture: Option<String>,
        meta: Option<serde_json::Value>,
    ) -> Result<i64> {
        let meta = meta.map(std::sync::Arc::new);
        self.run_blocking(move |k| {
            k.append_egress(
                &decision,
                reason.as_deref(),
                dest_host.as_deref(),
                dest_port,
                protocol.as_deref(),
                bytes_in,
                bytes_out,
                corr_id.as_deref(),
                proj.as_deref(),
                posture.as_deref(),
                meta.as_deref(),
            )
        })
        .await
    }

    pub async fn dequeue_one_queued_async(
        &self,
    ) -> Result<Option<(String, String, serde_json::Value)>> {
        self.run_blocking(|k| k.dequeue_one_queued()).await
    }

    pub async fn append_event_async(&self, env: &arw_events::Envelope) -> Result<i64> {
        let env = env.clone();
        self.run_blocking(move |k| k.append_event(&env)).await
    }

    pub async fn recent_events_async(
        &self,
        limit: i64,
        after_id: Option<i64>,
    ) -> Result<Vec<EventRow>> {
        self.run_blocking(move |k| k.recent_events(limit, after_id))
            .await
    }

    pub async fn events_by_corr_id_async(
        &self,
        corr_id: &str,
        limit: Option<i64>,
    ) -> Result<Vec<EventRow>> {
        let cid = corr_id.to_string();
        self.run_blocking(move |k| k.events_by_corr_id(&cid, limit))
            .await
    }

    pub async fn events_by_corr_ids_async(
        &self,
        corr_ids: Vec<String>,
        limit: Option<i64>,
    ) -> Result<HashMap<String, Vec<EventRow>>> {
        self.run_blocking(move |k| k.events_by_corr_ids(&corr_ids, limit))
            .await
    }

    pub async fn tail_events_async(
        &self,
        limit: i64,
        prefixes: Vec<String>,
    ) -> Result<(Vec<EventRow>, i64)> {
        self.run_blocking(move |k| k.tail_events(limit, &prefixes))
            .await
    }

    pub async fn count_actions_by_state_async(&self, state: &str) -> Result<i64> {
        let s = state.to_string();
        self.run_blocking(move |k| k.count_actions_by_state(&s))
            .await
    }

    pub async fn find_action_by_idem_async(&self, idem: &str) -> Result<Option<String>> {
        let s = idem.to_string();
        self.run_blocking(move |k| k.find_action_by_idem(&s)).await
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
        let id = id.to_string();
        let kind = kind.to_string();
        let input = input.clone();
        let policy_ctx = policy_ctx.cloned();
        let idem_key = idem_key.map(|s| s.to_string());
        let state_s = state.to_string();
        self.run_blocking(move |k| {
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
    }

    pub async fn get_action_async(&self, id: &str) -> Result<Option<ActionRow>> {
        let s = id.to_string();
        self.run_blocking(move |k| k.get_action(&s)).await
    }

    pub async fn set_action_state_async(&self, id: &str, state: &str) -> Result<bool> {
        let id_s = id.to_string();
        let st = state.to_string();
        self.run_blocking(move |k| k.set_action_state(&id_s, &st))
            .await
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
        let subject = subject.to_string();
        let kind = kind.to_string();
        let unit = unit.to_string();
        let corr_id = corr_id.map(|s| s.to_string());
        let proj = proj.map(|s| s.to_string());
        let meta = meta.cloned();
        self.run_blocking(move |k| {
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
    }

    pub async fn upsert_research_watcher_item_async(
        &self,
        source: Option<String>,
        source_id: Option<String>,
        title: Option<String>,
        summary: Option<String>,
        url: Option<String>,
        payload: Option<serde_json::Value>,
    ) -> Result<String> {
        self.run_blocking(move |k| {
            k.upsert_research_watcher_item(
                source.as_deref(),
                source_id.as_deref(),
                title.as_deref(),
                summary.as_deref(),
                url.as_deref(),
                payload.as_ref(),
            )
        })
        .await
    }

    pub async fn list_research_watcher_items_async(
        &self,
        status: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_research_watcher_items(status.as_deref(), limit))
            .await
    }

    pub async fn update_research_watcher_status_async(
        &self,
        id: String,
        status: String,
        note: Option<String>,
    ) -> Result<bool> {
        self.run_blocking(move |k| k.update_research_watcher_status(&id, &status, note.as_deref()))
            .await
    }

    pub async fn get_research_watcher_item_async(
        &self,
        id: String,
    ) -> Result<Option<ResearchWatcherItem>> {
        self.run_blocking(move |k| k.get_research_watcher_item(&id))
            .await
    }

    pub async fn insert_staging_action_async(
        &self,
        action_kind: String,
        action_input: serde_json::Value,
        project: Option<String>,
        requested_by: Option<String>,
        evidence: Option<serde_json::Value>,
    ) -> Result<String> {
        self.run_blocking(move |k| {
            k.insert_staging_action(
                &action_kind,
                &action_input,
                project.as_deref(),
                requested_by.as_deref(),
                evidence.as_ref(),
            )
        })
        .await
    }

    pub async fn list_staging_actions_async(
        &self,
        status: Option<String>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_staging_actions(status.as_deref(), limit))
            .await
    }

    pub async fn get_staging_action_async(&self, id: String) -> Result<Option<StagingAction>> {
        self.run_blocking(move |k| k.get_staging_action(&id)).await
    }

    pub async fn update_staging_action_status_async(
        &self,
        id: String,
        status: String,
        decision: Option<String>,
        decided_by: Option<String>,
        decided_at: Option<String>,
        action_id: Option<String>,
    ) -> Result<bool> {
        self.run_blocking(move |k| {
            k.update_staging_action_status(
                &id,
                &status,
                decision.as_deref(),
                decided_by.as_deref(),
                decided_at.as_deref(),
                action_id.as_deref(),
            )
        })
        .await
    }

    pub async fn list_contributions_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_contributions(limit))
            .await
    }

    pub async fn list_actions_async(
        &self,
        opts: ActionListOptions,
    ) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_actions_filtered(&opts))
            .await
    }

    pub async fn list_egress_async(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.run_blocking(move |k| k.list_egress(limit)).await
    }
}

#[derive(Clone, Debug, Default)]
pub struct ActionListOptions {
    pub limit: i64,
    pub state: Option<String>,
    pub kind_prefix: Option<String>,
    pub updated_since: Option<String>,
}

impl ActionListOptions {
    pub fn new(limit: i64) -> Self {
        Self {
            limit,
            ..Default::default()
        }
    }

    pub fn clamped_limit(&self) -> i64 {
        self.limit.clamp(1, 2000)
    }
}

impl KernelSession {
    fn store(&self) -> MemoryStore<'_> {
        MemoryStore::new(&self.conn)
    }

    pub fn select_memory_hybrid(
        &self,
        query: Option<&str>,
        embed: Option<&[f32]>,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.store().select_memory_hybrid(query, embed, lane, limit)
    }

    pub fn list_memory_links_many(
        &self,
        src_ids: &[String],
        limit_per: i64,
    ) -> Result<HashMap<String, Vec<serde_json::Value>>> {
        self.store().list_memory_links_many(src_ids, limit_per)
    }

    pub fn get_memory_many(&self, ids: &[String]) -> Result<HashMap<String, serde_json::Value>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        self.store().get_memory_many(ids)
    }

    pub fn expired_memory_candidates(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        self.store().expired_candidates(now, limit)
    }

    pub fn lane_overflow_candidates(
        &self,
        lane: &str,
        cap: usize,
        limit: usize,
    ) -> Result<Vec<MemoryGcCandidate>> {
        self.store().lane_overflow_candidates(lane, cap, limit)
    }

    pub fn delete_memory_records(&self, ids: &[String]) -> Result<usize> {
        self.store().delete_records(ids)
    }

    pub fn list_recent_memory(
        &self,
        lane: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.store().list_recent_memory(lane, limit)
    }

    pub fn list_logic_units(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn: &Connection = &self.conn;
        let mut stmt = conn.prepare(
            "SELECT id,manifest,status,created,updated \
             FROM logic_units ORDER BY updated DESC LIMIT ?",
        )?;
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

    pub fn list_orchestrator_jobs(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn: &Connection = &self.conn;
        let mut stmt = conn.prepare(
            "SELECT id,status,goal,data,progress,created,updated \
             FROM orchestrator_jobs ORDER BY updated DESC LIMIT ?",
        )?;
        let mut rows = stmt.query([limit])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let status_raw: String = r.get::<_, String>(1)?;
            let (status_slug, status_label) = Kernel::normalize_orchestrator_status(&status_raw);
            let mut payload = serde_json::json!({
                "id": r.get::<_, String>(0)?,
                "status": status_raw,
                "status_slug": status_slug,
                "status_label": status_label,
                "goal": r.get::<_, Option<String>>(2)?,
                "progress": r.get::<_, Option<f64>>(4)?,
                "created": r.get::<_, String>(5)?,
                "updated": r.get::<_, String>(6)?,
            });
            let data_raw: Option<String> = r.get(3)?;
            if let Some(data_raw) = data_raw {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_raw) {
                    if !val.is_null() {
                        if let serde_json::Value::Object(ref mut map) = payload {
                            map.insert("data".into(), val);
                        }
                    }
                }
            }
            out.push(payload);
        }
        Ok(out)
    }

    pub fn list_leases(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        let conn: &Connection = &self.conn;
        let mut stmt = conn.prepare(
            "SELECT id,subject,capability,scope,ttl_until,budget,policy_ctx,created,updated \
             FROM leases ORDER BY updated DESC LIMIT ?",
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{SecondsFormat, Utc};
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn orchestrator_status_normalization() {
        let cases = vec![
            ("queued", ("queued", "Queued")),
            ("Pending", ("queued", "Queued")),
            ("running", ("running", "Running")),
            ("IN_PROGRESS", ("running", "Running")),
            ("completed", ("completed", "Completed")),
            ("DONE", ("completed", "Completed")),
            ("failed", ("failed", "Failed")),
            ("ERROR", ("failed", "Failed")),
            ("canceled", ("cancelled", "Cancelled")),
            ("", ("unknown", "Unknown")),
        ];
        for (input, expected) in cases {
            assert_eq!(Kernel::normalize_orchestrator_status(input), expected);
        }
    }

    #[tokio::test]
    async fn research_watcher_upsert_and_status() {
        let dir = TempDir::new().expect("temp dir");
        let kernel = Kernel::open(dir.path()).expect("kernel open");

        let id = kernel
            .upsert_research_watcher_item_async(
                Some("arxiv".to_string()),
                Some("arxiv:2309".to_string()),
                Some("Original title".to_string()),
                Some("Initial summary".to_string()),
                Some("https://example.test/paper".to_string()),
                Some(json!({"authors": ["Ada"]})),
            )
            .await
            .expect("insert research watcher item");

        let pending = kernel
            .list_research_watcher_items_async(Some("pending".to_string()), 10)
            .await
            .expect("list pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["id"], id);

        // Upsert with same source_id should update the existing record.
        let same_id = kernel
            .upsert_research_watcher_item_async(
                Some("arxiv".to_string()),
                Some("arxiv:2309".to_string()),
                Some("Updated title".to_string()),
                Some("Refined summary".to_string()),
                Some("https://example.test/paper".to_string()),
                None,
            )
            .await
            .expect("update research watcher item");
        assert_eq!(id, same_id);

        let note = Some("Looks promising".to_string());
        let changed = kernel
            .update_research_watcher_status_async(id.clone(), "approved".to_string(), note.clone())
            .await
            .expect("update status");
        assert!(changed);

        let item = kernel
            .get_research_watcher_item_async(id.clone())
            .await
            .expect("fetch item")
            .expect("item present");
        assert_eq!(item.status, "approved");
        assert_eq!(item.note, note);

        let still_pending = kernel
            .list_research_watcher_items_async(Some("pending".to_string()), 10)
            .await
            .expect("list pending after status change");
        assert!(still_pending.is_empty());

        // Unknown id returns false
        let changed = kernel
            .update_research_watcher_status_async(
                "missing".to_string(),
                "archived".to_string(),
                None,
            )
            .await
            .expect("update missing");
        assert!(!changed);
    }

    #[tokio::test]
    async fn orchestrator_jobs_surface_data_payload() {
        let dir = TempDir::new().expect("temp dir");
        let kernel = Kernel::open(dir.path()).expect("kernel open");

        let data = json!({
            "preset": "balanced",
            "diversity": 0.5,
            "recency": 0.35,
            "compression": 0.4,
        });
        let job_id = kernel
            .insert_orchestrator_job_async("test goal", Some(&data))
            .await
            .expect("insert orchestrator job");

        let jobs = kernel
            .list_orchestrator_jobs_async(5)
            .await
            .expect("list orchestrator jobs");
        assert!(!jobs.is_empty(), "expected at least one job");

        let job = jobs
            .into_iter()
            .find(|job| job["id"] == job_id)
            .expect("job present");
        assert_eq!(job["goal"], json!("test goal"));
        assert_eq!(job["status_slug"], json!("queued"));
        assert_eq!(job["status_label"], json!("Queued"));
        assert_eq!(job["progress"], json!(0.0));

        let data_field = job.get("data").cloned().expect("data field surfaced");
        assert_eq!(data_field["preset"], json!("balanced"));
        assert_eq!(data_field["diversity"], json!(0.5));
        assert_eq!(data_field["recency"], json!(0.35));
        assert_eq!(data_field["compression"], json!(0.4));
    }

    #[tokio::test]
    async fn staging_actions_lifecycle() {
        let dir = TempDir::new().expect("temp dir");
        let kernel = Kernel::open(dir.path()).expect("kernel open");
        let payload = json!({
            "project": "demo",
            "evidence": {"link": "https://example.test"}
        });

        let staging_id = kernel
            .insert_staging_action_async(
                "fs.patch".to_string(),
                payload.clone(),
                Some("demo".to_string()),
                Some("alice@example.test".to_string()),
                payload.get("evidence").cloned(),
            )
            .await
            .expect("insert staging action");

        let pending = kernel
            .list_staging_actions_async(Some("pending".to_string()), 10)
            .await
            .expect("list pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["id"], staging_id);

        let review_time = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let approved = kernel
            .update_staging_action_status_async(
                staging_id.clone(),
                "approved".to_string(),
                Some("approved".to_string()),
                Some("reviewer".to_string()),
                Some(review_time.clone()),
                Some("action-1".to_string()),
            )
            .await
            .expect("approve staging");
        assert!(approved);

        let record = kernel
            .get_staging_action_async(staging_id.clone())
            .await
            .expect("get staging action")
            .expect("staging exists");
        assert_eq!(record.status, "approved");
        assert_eq!(record.action_id.as_deref(), Some("action-1"));
        assert_eq!(record.decided_by.as_deref(), Some("reviewer"));

        let history = kernel
            .list_staging_actions_async(None, 10)
            .await
            .expect("list all");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["status"], json!("approved"));
    }

    #[tokio::test]
    async fn staging_actions_denied() {
        let dir = TempDir::new().expect("temp dir");
        let kernel = Kernel::open(dir.path()).expect("kernel open");
        let payload = json!({"project": "lab"});
        let id = kernel
            .insert_staging_action_async(
                "net.http.get".to_string(),
                payload.clone(),
                payload
                    .get("project")
                    .and_then(|p| p.as_str())
                    .map(|s| s.to_string()),
                None,
                None,
            )
            .await
            .expect("insert staging");

        let denied = kernel
            .update_staging_action_status_async(
                id.clone(),
                "denied".to_string(),
                Some("unsupported".to_string()),
                Some("reviewer".to_string()),
                Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
                None,
            )
            .await
            .expect("deny staging");
        assert!(denied);

        let record = kernel
            .get_staging_action_async(id.clone())
            .await
            .expect("get staging")
            .expect("staging exists");
        assert_eq!(record.status, "denied");
        assert_eq!(record.decision.as_deref(), Some("unsupported"));
        assert_eq!(record.action_id, None);
    }
}
