use tracing_subscriber::{fmt, EnvFilter};

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

    let _ = fmt().with_env_filter(filter).try_init();
}

#[allow(dead_code)]
pub fn shutdown() {
    #[cfg(feature = "otlp")]
    {
        opentelemetry::global::shutdown_tracer_provider();
    }
}
