use once_cell::sync::OnceCell;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::{
    fmt,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter,
};
static ACCESS_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();

/// Initialize tracing. If compiled with feature `otlp` and env ARW_OTEL=1,
/// you can extend this to wire OTLP exporters (left as future step).
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    #[cfg(feature = "otlp")]
    {
        if std::env::var("ARW_OTEL").as_deref() == Ok("1") {
            // Placeholder: add OTLP pipeline here later.
            // For now, we fall back to plain formatting to avoid surprises.
            let _ = fmt().with_env_filter(filter).try_init();
            tracing::warn!(
                "ARW_OTEL=1 set but OTLP pipeline not yet implemented; using plain tracing."
            );
            return;
        }
    }

    // Base console layer
    let fmt_layer = fmt::layer();
    // Compose registry with layers
    let base = tracing_subscriber::registry().with(fmt_layer.with_filter(filter));
    // Optional rolling access log layer filtered to http.access
    if std::env::var("ARW_ACCESS_LOG_ROLL").ok().as_deref() == Some("1") {
        let dir = std::env::var("ARW_ACCESS_LOG_DIR")
            .ok()
            .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
            .unwrap_or_else(|| "logs".to_string());
        let prefix =
            std::env::var("ARW_ACCESS_LOG_PREFIX").unwrap_or_else(|_| "http-access".into());
        let rotation = std::env::var("ARW_ACCESS_LOG_ROTATION").unwrap_or_else(|_| "daily".into());
        let _ = std::fs::create_dir_all(&dir);
        let writer = match rotation.to_lowercase().as_str() {
            "hourly" => tracing_appender::rolling::hourly(&dir, &prefix),
            "minutely" => tracing_appender::rolling::minutely(&dir, &prefix),
            _ => tracing_appender::rolling::daily(&dir, &prefix),
        };
        let (nb, guard) = tracing_appender::non_blocking(writer);
        let _ = ACCESS_GUARD.set(guard);
        let targets = Targets::new().with_target("http.access", tracing::Level::INFO);
        let access_layer = fmt::layer()
            .with_ansi(false)
            .with_target(false)
            .with_writer(nb);
        let subscriber = base.with(access_layer.with_filter(targets));
        let _ = subscriber.try_init();
    } else {
        let _ = base.try_init();
    }
}

#[allow(dead_code)]
pub fn shutdown() {
    #[cfg(feature = "otlp")]
    {
        opentelemetry::global::shutdown_tracer_provider();
    }
}
