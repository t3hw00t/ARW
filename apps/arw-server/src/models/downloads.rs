use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{ModelsJobDestination, ModelsJobSnapshot};

#[derive(Clone)]
pub(super) struct DestInfo {
    pub(super) host: String,
    pub(super) port: u16,
    pub(super) protocol: String,
}

pub(super) struct DownloadHandle {
    pub(super) cancel: CancellationToken,
    pub(super) task: Option<JoinHandle<()>>,
    pub(super) job_id: String,
    pub(super) url_display: String,
    pub(super) corr_id: String,
    pub(super) dest: DestInfo,
    pub(super) started_at: Instant,
}

pub(super) struct DownloadsState {
    jobs: Mutex<HashMap<String, DownloadHandle>>,
    notify: Notify,
}

impl DownloadsState {
    pub(super) fn new() -> Self {
        Self {
            jobs: Mutex::new(HashMap::new()),
            notify: Notify::new(),
        }
    }

    pub(super) async fn contains(&self, id: &str) -> bool {
        self.jobs.lock().await.contains_key(id)
    }

    pub(super) async fn active_count(&self) -> usize {
        self.jobs.lock().await.len()
    }

    pub(super) async fn wait_for_slot(
        &self,
        max: u64,
        cancel: &CancellationToken,
    ) -> Result<(), ()> {
        let max = max.max(1);
        loop {
            let current = self.active_count().await as u64;
            if current < max {
                return Ok(());
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(()),
                _ = self.notify.notified() => {}
            }
        }
    }

    pub(super) async fn wait_until_at_most(&self, limit: u64) {
        let limit = limit.max(1);
        loop {
            let current = self.active_count().await as u64;
            if current <= limit {
                return;
            }
            self.notify.notified().await;
        }
    }

    pub(super) async fn insert_job(
        &self,
        model_id: &str,
        handle: DownloadHandle,
    ) -> Result<(), ()> {
        let mut jobs = self.jobs.lock().await;
        if jobs.contains_key(model_id) {
            return Err(());
        }
        jobs.insert(model_id.to_string(), handle);
        Ok(())
    }

    pub(super) async fn attach_task(&self, model_id: &str, task: JoinHandle<()>) {
        let mut jobs = self.jobs.lock().await;
        if let Some(entry) = jobs.get_mut(model_id) {
            entry.task = Some(task);
        }
    }

    pub(super) async fn remove_job(&self, model_id: &str) -> Option<DownloadHandle> {
        let mut jobs = self.jobs.lock().await;
        let removed = jobs.remove(model_id);
        if removed.is_some() {
            self.notify.notify_waiters();
        }
        removed
    }

    pub(super) async fn cancel_job(&self, model_id: &str) -> Option<(String, DestInfo)> {
        let handle = {
            let mut jobs = self.jobs.lock().await;
            jobs.remove(model_id)
        };
        if let Some(mut handle) = handle {
            let corr_id = handle.corr_id.clone();
            let dest = handle.dest.clone();
            handle.cancel.cancel();
            self.notify.notify_waiters();
            if let Some(task) = handle.task.take() {
                tokio::spawn(async move {
                    if let Err(err) = task.await {
                        warn!("cancelled download join err: {err}");
                    }
                });
            }
            Some((corr_id, dest))
        } else {
            None
        }
    }

    pub(super) async fn job_snapshot(&self) -> Vec<ModelsJobSnapshot> {
        let jobs = self.jobs.lock().await;
        jobs.iter()
            .map(|(model_id, handle)| {
                let dest = &handle.dest;
                ModelsJobSnapshot {
                    model_id: model_id.clone(),
                    job_id: handle.job_id.clone(),
                    url: handle.url_display.clone(),
                    corr_id: handle.corr_id.clone(),
                    dest: ModelsJobDestination {
                        host: dest.host.clone(),
                        port: dest.port,
                        protocol: dest.protocol.clone(),
                    },
                    started_at: handle.started_at.elapsed().as_secs(),
                }
            })
            .collect()
    }

    pub(super) fn notify_all(&self) {
        self.notify.notify_waiters();
    }
}
