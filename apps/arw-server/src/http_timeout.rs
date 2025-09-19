use once_cell::sync::OnceCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const FALLBACK_HTTP_TIMEOUT_SECS: u64 = 20;

fn default_from_env() -> u64 {
    std::env::var("ARW_HTTP_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(FALLBACK_HTTP_TIMEOUT_SECS)
}

fn global_handle() -> &'static Arc<AtomicU64> {
    static HANDLE: OnceCell<Arc<AtomicU64>> = OnceCell::new();
    HANDLE.get_or_init(|| Arc::new(AtomicU64::new(default_from_env())))
}

/// Seed the global timeout from the environment; returns the applied seconds.
pub fn init_from_env() -> u64 {
    let secs = default_from_env().max(1);
    set_secs(secs);
    secs
}

/// Current timeout in seconds.
pub fn get_secs() -> u64 {
    global_handle().load(Ordering::Relaxed)
}

/// Current timeout as a Duration (at least 1 second).
pub fn get_duration() -> Duration {
    Duration::from_secs(get_secs().max(1))
}

/// Update the timeout in seconds at runtime.
pub fn set_secs(secs: u64) {
    let clamped = secs.max(1);
    global_handle().store(clamped, Ordering::Relaxed);
    std::env::set_var("ARW_HTTP_TIMEOUT_SECS", clamped.to_string());
}

#[cfg(test)]
mod tests {
    #[test]
    fn init_and_update() {
        super::init_from_env();
        let before = super::get_secs();
        super::set_secs(99);
        assert_eq!(super::get_secs(), 99);
        super::set_secs(0);
        assert_eq!(super::get_secs(), 1);
        // Restore original
        super::set_secs(before);
        assert_eq!(super::get_secs(), before);
    }
}
