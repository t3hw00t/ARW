use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

use super::queue::Queue;
use super::types::{LeaseToken, Task, DEFAULT_LEASE_TTL_MS, MIN_LEASE_TTL_MS};
use super::util::now_millis;

/// In-memory queue for single-process testing and defaults.
#[derive(Clone)]
pub struct LocalQueue {
    inner: Arc<LocalInner>,
}

struct LocalInner {
    // simple FIFO per priority; lowest key is highest priority (i.e. -10 runs before 0)
    queues: Mutex<BTreeMap<i32, VecDeque<Task>>>,
    pending: Mutex<HashMap<String, (Task, u64)>>, // lease_id -> (task, expires_at_ms)
    notify: Notify,
    lease_ttl_ms: u64,
    sweeper_shutdown: Arc<Notify>,
    stop_flag: AtomicBool,
}

impl LocalQueue {
    pub fn new() -> Self {
        Self::with_lease_ttl(DEFAULT_LEASE_TTL_MS)
    }

    pub fn with_lease_ttl(ttl_ms: u64) -> Self {
        let inner = Arc::new(LocalInner {
            queues: Mutex::new(BTreeMap::new()),
            pending: Mutex::new(HashMap::new()),
            notify: Notify::new(),
            lease_ttl_ms: ttl_ms.max(MIN_LEASE_TTL_MS),
            sweeper_shutdown: Arc::new(Notify::new()),
            stop_flag: AtomicBool::new(false),
        });
        let this = Self {
            inner: inner.clone(),
        };
        // Start lease sweeper to re-enqueue expired leases
        let inner_clone = inner.clone();
        tokio::spawn(async move {
            let shutdown = inner_clone.sweeper_shutdown.clone();
            loop {
                if inner_clone.stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {}
                    _ = shutdown.notified() => {
                        if inner_clone.stop_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        continue;
                    }
                }
                if inner_clone.stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                let now = now_millis();
                // collect expired leases
                let mut expired: Vec<Task> = Vec::new();
                {
                    let mut pend = inner_clone.pending.lock().await;
                    let lids: Vec<String> = pend
                        .iter()
                        .filter_map(
                            |(lid, (_t, exp))| if *exp <= now { Some(lid.clone()) } else { None },
                        )
                        .collect();
                    for lid in lids {
                        if let Some((t, _)) = pend.remove(&lid) {
                            expired.push(t);
                        }
                    }
                }
                if !expired.is_empty() {
                    {
                        let mut map = inner_clone.queues.lock().await;
                        for mut t in expired {
                            t.attempt = t.attempt.saturating_add(1);
                            let q = map.entry(t.priority).or_insert_with(VecDeque::new);
                            q.push_back(t);
                        }
                    }
                    inner_clone.notify.notify_waiters();
                }
            }
        });
        this
    }
}

impl Default for LocalQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Queue for LocalQueue {
    async fn enqueue(&self, mut t: Task) -> anyhow::Result<String> {
        if t.id.is_empty() {
            t.id = Uuid::new_v4().to_string();
        }
        let id = t.id.clone();
        let prio = t.priority;
        let mut map = self.inner.queues.lock().await;
        let q = map.entry(prio).or_insert_with(VecDeque::new);
        q.push_back(t);
        drop(map);
        self.inner.notify.notify_one();
        Ok(id)
    }

    async fn dequeue(&self, _group: &str) -> anyhow::Result<(Task, LeaseToken)> {
        loop {
            // pop highest-priority non-empty queue
            let sel = {
                let mut map = self.inner.queues.lock().await;
                let mut selected: Option<Task> = None;
                let mut empty_key: Option<i32> = None;
                for (priority, queue) in map.iter_mut() {
                    if let Some(task) = queue.pop_front() {
                        if queue.is_empty() {
                            empty_key = Some(*priority);
                        }
                        selected = Some(task);
                        break;
                    }
                }
                if let Some(key) = empty_key {
                    map.remove(&key);
                }
                selected
            };
            if let Some(task) = sel {
                let lease_id = Uuid::new_v4().to_string();
                let now_ms = now_millis();
                let ttl_ms = self.inner.lease_ttl_ms; // configurable lease ttl
                let exp = now_ms + ttl_ms;
                {
                    let mut pend = self.inner.pending.lock().await;
                    pend.insert(lease_id.clone(), (task.clone(), exp));
                }
                let task_id = task.id.clone();
                return Ok((
                    task,
                    LeaseToken {
                        task_id,
                        lease_id,
                        expires_at_ms: exp,
                    },
                ));
            }
            // nothing ready; wait for a new task
            self.inner.notify.notified().await;
        }
    }

    async fn ack(&self, lease: LeaseToken) -> anyhow::Result<()> {
        let mut pend = self.inner.pending.lock().await;
        pend.remove(&lease.lease_id);
        Ok(())
    }

    async fn nack(&self, lease: LeaseToken, retry_after_ms: Option<u64>) -> anyhow::Result<()> {
        let mut pend = self.inner.pending.lock().await;
        if let Some((mut task, _exp)) = pend.remove(&lease.lease_id) {
            task.attempt = task.attempt.saturating_add(1);
            drop(pend);
            if let Some(delay) = retry_after_ms {
                let q = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    let _ = q.enqueue(task).await;
                });
            } else {
                self.enqueue(task).await?;
            }
        }
        Ok(())
    }
}

impl Drop for LocalQueue {
    fn drop(&mut self) {
        // only signal shutdown when this is the final handle
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.stop_flag.store(true, Ordering::SeqCst);
            self.inner.sweeper_shutdown.notify_waiters();
            self.inner.notify.notify_waiters();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::Task;
    use super::LocalQueue;
    use crate::orchestrator::Queue;
    use serde_json::json;
    use tokio::time::{sleep, timeout, Duration};

    #[tokio::test]
    async fn local_queue_requeues_expired_tasks() {
        let queue = LocalQueue::with_lease_ttl(100);
        let mut task = Task::new("op.test", json!({"k": "v"}));
        task.priority = 1;
        let id = queue.enqueue(task).await.unwrap();

        let (first_task, _lease) = queue.dequeue("g1").await.unwrap();
        assert_eq!(first_task.id, id);
        assert_eq!(first_task.attempt, 0);

        sleep(Duration::from_millis(200)).await;

        let (second_task, second_lease) = timeout(Duration::from_secs(2), queue.dequeue("g1"))
            .await
            .expect("dequeue timed out")
            .unwrap();

        assert_eq!(second_task.id, id);
        assert_eq!(second_task.priority, 1);
        assert_eq!(second_task.attempt, 1);

        queue.ack(second_lease).await.unwrap();
    }
}
