use once_cell::sync::OnceCell;
use std::time::Duration;

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

fn connect_timeout() -> Duration {
    Duration::from_secs(env_u64("ARW_HTTP_CONNECT_TIMEOUT_SECS", 3).max(1))
}

fn keepalive() -> Duration {
    Duration::from_secs(env_u64("ARW_HTTP_TCP_KEEPALIVE_SECS", 60).max(1))
}

fn pool_idle() -> Duration {
    Duration::from_secs(env_u64("ARW_HTTP_POOL_IDLE_SECS", 90).max(1))
}

fn user_agent() -> String {
    format!("arw-server/{}", env!("CARGO_PKG_VERSION"))
}

/// Base client builder with harmonized defaults. Apply per-call `.timeout(...)` as needed.
pub fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .connect_timeout(connect_timeout())
        .tcp_keepalive(keepalive())
        .pool_idle_timeout(pool_idle())
}

/// Shared default client honoring the global http request timeout.
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceCell<reqwest::Client> = OnceCell::new();
    CLIENT.get_or_init(|| {
        builder()
            .timeout(crate::http_timeout::get_duration())
            .build()
            .expect("http client")
    })
}

/// Build a client with a specific request timeout.
pub fn client_with_timeout(timeout: Duration) -> reqwest::Client {
    builder().timeout(timeout).build().expect("http client")
}
