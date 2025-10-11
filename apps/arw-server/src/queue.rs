use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

/// Signals consumers when the action queue transitions between idle and busy states.
/// Wraps a monotonic sequence so waiters never miss a wake even if they subscribe late.
#[derive(Default)]
pub(crate) struct QueueSignals {
    notify: Notify,
    seq: AtomicU64,
}

impl QueueSignals {
    /// Return the last emitted sequence number.
    pub fn version(&self) -> u64 {
        self.seq.load(Ordering::Acquire)
    }

    /// Emit a wake notification for waiters and bump the sequence.
    pub fn wake(&self) {
        self.seq.fetch_add(1, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// Wait until the sequence advances beyond the provided snapshot or the timeout elapses.
    /// Returns the latest observed sequence after the wait (which may be unchanged on timeout).
    pub async fn wait_for_change(&self, last_seen: u64, max_wait: Duration) -> u64 {
        let wait_for = max_wait.max(Duration::from_millis(1));
        loop {
            let current = self.version();
            if current > last_seen {
                return current;
            }
            let notified = self.notify.notified();
            match tokio::time::timeout(wait_for, notified).await {
                Ok(_) => {
                    // Loop around to observe the updated sequence.
                }
                Err(_) => {
                    return self.version();
                }
            }
        }
    }
}
