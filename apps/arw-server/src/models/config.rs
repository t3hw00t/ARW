use std::path::PathBuf;
use std::time::Duration;

use fs2::available_space;
use once_cell::sync::OnceCell;

#[derive(Clone, Copy, Debug)]
pub struct DownloadTuning {
    pub idle_timeout: Option<Duration>,
    pub send_retries: u32,
    pub stream_retries: u32,
    pub retry_backoff_ms: u64,
}

impl DownloadTuning {
    pub fn from_env() -> Self {
        let idle_timeout = std::env::var("ARW_DL_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());
        let idle_timeout = match idle_timeout {
            Some(0) => None,
            Some(secs) => Some(Duration::from_secs(secs)),
            None => Some(Duration::from_secs(300)),
        };
        let send_retries = std::env::var("ARW_DL_SEND_RETRIES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(2);
        let stream_retries = std::env::var("ARW_DL_STREAM_RETRIES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(2);
        let retry_backoff_ms = std::env::var("ARW_DL_RETRY_BACKOFF_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(500)
            .clamp(50, 60_000);
        Self {
            idle_timeout,
            send_retries,
            stream_retries,
            retry_backoff_ms,
        }
    }

    pub fn idle_timeout_secs(&self) -> Option<u64> {
        self.idle_timeout.map(|d| d.as_secs())
    }

    pub fn backoff_delay(&self, attempt: u32) -> Duration {
        let step = attempt.max(1);
        let base = Duration::from_millis(self.retry_backoff_ms);
        base.checked_mul(step).unwrap_or(base)
    }
}

pub fn download_tuning() -> &'static DownloadTuning {
    static TUNING: OnceCell<DownloadTuning> = OnceCell::new();
    TUNING.get_or_init(DownloadTuning::from_env)
}

pub fn disk_reserve_bytes() -> u64 {
    static BYTES: OnceCell<u64> = OnceCell::new();
    *BYTES.get_or_init(|| {
        std::env::var("ARW_MODELS_DISK_RESERVE_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(256)
            .saturating_mul(1024 * 1024)
    })
}

pub fn max_download_bytes() -> Option<u64> {
    static BYTES: OnceCell<Option<u64>> = OnceCell::new();
    *BYTES.get_or_init(|| {
        std::env::var("ARW_MODELS_MAX_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1024 * 1024))
            .filter(|b| *b > 0)
    })
}

pub fn quota_bytes() -> Option<u64> {
    static BYTES: OnceCell<Option<u64>> = OnceCell::new();
    *BYTES.get_or_init(|| {
        std::env::var("ARW_MODELS_QUOTA_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mb| mb.saturating_mul(1024 * 1024))
            .filter(|b| *b > 0)
    })
}

pub fn available_space_bytes(path: PathBuf) -> Result<u64, String> {
    available_space(path).map_err(|e| e.to_string())
}
