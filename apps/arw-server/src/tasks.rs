use std::{borrow::Cow, sync::Arc, time::Duration};

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
