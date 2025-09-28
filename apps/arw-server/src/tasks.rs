use futures_util::FutureExt;
use once_cell::sync::OnceCell;
use std::{
    borrow::Cow,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use tokio::task::JoinHandle;
use tracing::{debug, trace};

use crate::metrics;

#[derive(Debug)]
pub struct TaskHandle {
    name: Cow<'static, str>,
    handle: JoinHandle<()>,
    started_recorded: bool,
}

impl TaskHandle {
    pub fn new(name: impl Into<Cow<'static, str>>, handle: JoinHandle<()>) -> Self {
        Self {
            name: name.into(),
            handle,
            started_recorded: false,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn mark_started(&mut self) {
        self.started_recorded = true;
    }

    pub(crate) fn started_recorded(&self) -> bool {
        self.started_recorded
    }

    pub fn into_inner(self) -> (Cow<'static, str>, bool, JoinHandle<()>) {
        (self.name, self.started_recorded, self.handle)
    }
}

#[derive(Default)]
pub struct TaskManager {
    tasks: Vec<TaskHandle>,
    metrics: Option<Arc<metrics::Metrics>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            metrics: None,
        }
    }

    pub fn with_metrics(metrics: Arc<metrics::Metrics>) -> Self {
        // Register metrics for global supervisor visibility
        register_global_metrics(metrics.clone());
        Self {
            tasks: Vec::new(),
            metrics: Some(metrics),
        }
    }

    pub fn push(&mut self, mut task: TaskHandle) {
        trace!(task = task.name(), "task registered");
        if let Some(metrics) = &self.metrics {
            metrics.task_started(task.name());
            task.mark_started();
        }
        self.tasks.push(task);
    }

    #[allow(dead_code)]
    pub fn push_handle(&mut self, name: impl Into<Cow<'static, str>>, handle: JoinHandle<()>) {
        self.push(TaskHandle::new(name, handle));
    }

    pub fn extend<I>(&mut self, tasks: I)
    where
        I: IntoIterator<Item = TaskHandle>,
    {
        for task in tasks {
            self.push(task);
        }
    }

    pub fn merge(&mut self, mut other: TaskManager) {
        if self.metrics.is_none() {
            self.metrics = other.metrics.clone();
        }
        for mut task in other.tasks.drain(..) {
            if !task.started_recorded() {
                if let Some(metrics) = &self.metrics {
                    metrics.task_started(task.name());
                    task.mark_started();
                }
            }
            self.tasks.push(task);
        }
    }

    #[allow(dead_code)]
    pub async fn shutdown(self) {
        self.shutdown_with_grace(Duration::from_secs(0)).await;
    }

    pub async fn shutdown_with_grace(self, grace: Duration) {
        let metrics = self.metrics.clone();
        for task in self.tasks {
            let (name_cow, started_recorded, mut handle) = task.into_inner();
            let name = name_cow.into_owned();
            let record_outcome = |outcome: TaskOutcome| {
                if started_recorded {
                    if let Some(metrics) = &metrics {
                        match outcome {
                            TaskOutcome::Completed => metrics.task_completed(&name),
                            TaskOutcome::Aborted => metrics.task_aborted(&name),
                        }
                    }
                }
            };

            if grace.is_zero() {
                handle.abort();
                let result = handle.await;
                let outcome = if result.is_ok() {
                    TaskOutcome::Completed
                } else {
                    debug!(task = %name, ?result, "task join after abort failed");
                    TaskOutcome::Aborted
                };
                record_outcome(outcome);
                continue;
            }

            let sleeper = tokio::time::sleep(grace);
            tokio::pin!(sleeper);
            let outcome = tokio::select! {
                res = &mut handle => {
                    if let Err(err) = res {
                        debug!(task = %name, ?err, "task exited with error");
                        TaskOutcome::Aborted
                    } else {
                        TaskOutcome::Completed
                    }
                }
                _ = &mut sleeper => {
                    handle.abort();
                    match handle.await {
                        Ok(_) => TaskOutcome::Completed,
                        Err(err) => {
                            debug!(task = %name, ?err, "task join after abort failed");
                            TaskOutcome::Aborted
                        }
                    }
                }
            };
            record_outcome(outcome);
        }
    }
}

impl From<Vec<TaskHandle>> for TaskManager {
    fn from(tasks: Vec<TaskHandle>) -> Self {
        let mut manager = TaskManager::new();
        manager.extend(tasks);
        manager
    }
}

impl IntoIterator for TaskManager {
    type Item = TaskHandle;
    type IntoIter = std::vec::IntoIter<TaskHandle>;

    fn into_iter(self) -> Self::IntoIter {
        self.tasks.into_iter()
    }
}

enum TaskOutcome {
    Completed,
    Aborted,
}

/// Spawn a supervised background task that restarts on panic with exponential backoff.
/// Use for long-running loops that should survive transient failures.
pub fn spawn_supervised<F, Fut>(name: impl Into<Cow<'static, str>>, factory: F) -> TaskHandle
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    spawn_supervised_with(name, factory, Option::<fn(u32)>::None)
}

/// Supervised spawn with a restart callback. The callback receives the restart count within
/// the current window whenever a panic occurs.
pub fn spawn_supervised_with<F, Fut, R>(
    name: impl Into<Cow<'static, str>>,
    mut factory: F,
    mut on_restart: Option<R>,
) -> TaskHandle
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
    R: FnMut(u32) + Send + 'static,
{
    let name_cow = name.into();
    let name_for_task = name_cow.clone();
    let handle = tokio::spawn(async move {
        // Safe-mode initial delay (applies once per task startup, not per restart cycle)
        crate::crashguard::await_initial_delay().await;
        let mut backoff_ms: u64 = 200;
        // Thrash detection window
        let window = Duration::from_secs(30);
        let mut window_start = Instant::now();
        let mut restarts_in_window: u32 = 0;
        loop {
            // Catch panics from the future body to keep the supervisor alive.
            let result = std::panic::AssertUnwindSafe(factory()).catch_unwind().await;
            match result {
                Ok(()) => {
                    tracing::debug!(task = %name_for_task, "supervised task completed normally");
                    break;
                }
                Err(e) => {
                    let now = Instant::now();
                    if now.duration_since(window_start) > window {
                        window_start = now;
                        restarts_in_window = 0;
                    }
                    restarts_in_window = restarts_in_window.saturating_add(1);
                    // Record restarts in metrics if available
                    if let Some(w) = GLOBAL_METRICS.get() {
                        if let Some(m) = w.upgrade() {
                            m.task_restarts_window_set(&name_for_task, restarts_in_window as u64);
                        }
                    }
                    tracing::error!(task = %name_for_task, backoff_ms, restarts_in_window, "supervised task panicked; restarting");
                    if let Some(cb) = on_restart.as_mut() {
                        cb(restarts_in_window);
                    }
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(10_000);
                    let _ = e; // drop payload
                }
            }
        }
    });
    TaskHandle::new(name_cow, handle)
}

static GLOBAL_METRICS: OnceCell<Weak<crate::metrics::Metrics>> = OnceCell::new();

fn register_global_metrics(metrics: Arc<crate::metrics::Metrics>) {
    let _ = GLOBAL_METRICS.set(Arc::downgrade(&metrics));
}
