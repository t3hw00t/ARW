use super::ModelsMetricsCounters;

/// Tracks download metrics before they are exposed via read-model snapshots.
#[derive(Clone, Default)]
pub(super) struct MetricsState {
    started: u64,
    queued: u64,
    admitted: u64,
    resumed: u64,
    canceled: u64,
    completed: u64,
    completed_cached: u64,
    errors: u64,
    bytes_total: u64,
    ewma_mbps: Option<f64>,
    preflight_ok: u64,
    preflight_denied: u64,
    preflight_skipped: u64,
    coalesced: u64,
}

impl MetricsState {
    pub(super) fn snapshot(&self) -> ModelsMetricsCounters {
        ModelsMetricsCounters {
            started: self.started,
            queued: self.queued,
            admitted: self.admitted,
            resumed: self.resumed,
            canceled: self.canceled,
            completed: self.completed,
            completed_cached: self.completed_cached,
            errors: self.errors,
            bytes_total: self.bytes_total,
            ewma_mbps: self.ewma_mbps,
            preflight_ok: self.preflight_ok,
            preflight_denied: self.preflight_denied,
            preflight_skipped: self.preflight_skipped,
            coalesced: self.coalesced,
        }
    }

    pub(super) fn record_started(&mut self) {
        self.started = self.started.saturating_add(1);
        self.queued = self.queued.saturating_add(1);
    }

    pub(super) fn record_admitted(&mut self) {
        if self.queued > 0 {
            self.queued -= 1;
        }
        self.admitted = self.admitted.saturating_add(1);
    }

    pub(super) fn record_resumed(&mut self) {
        self.resumed = self.resumed.saturating_add(1);
    }

    pub(super) fn record_completed(&mut self, bytes: u64, mbps: Option<f64>, cached: bool) {
        if cached {
            self.completed_cached = self.completed_cached.saturating_add(1);
        } else {
            self.completed = self.completed.saturating_add(1);
        }
        self.bytes_total = self.bytes_total.saturating_add(bytes);
        if let Some(speed) = mbps {
            self.ewma_mbps = Some(match self.ewma_mbps {
                Some(prev) => (prev * 0.6) + (speed * 0.4),
                None => speed,
            });
        }
    }

    pub(super) fn record_error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    pub(super) fn record_canceled(&mut self) {
        self.canceled = self.canceled.saturating_add(1);
    }

    pub(super) fn record_preflight_ok(&mut self) {
        self.preflight_ok = self.preflight_ok.saturating_add(1);
    }

    pub(super) fn record_preflight_denied(&mut self) {
        self.preflight_denied = self.preflight_denied.saturating_add(1);
    }

    pub(super) fn record_preflight_skipped(&mut self) {
        self.preflight_skipped = self.preflight_skipped.saturating_add(1);
    }

    pub(super) fn record_coalesced(&mut self) {
        self.coalesced = self.coalesced.saturating_add(1);
    }

    pub(super) fn decrement_queue(&mut self) {
        if self.queued > 0 {
            self.queued -= 1;
        }
    }

    pub(super) fn set_ewma_mbps(&mut self, ewma: f64) {
        self.ewma_mbps = Some(ewma);
    }
}
