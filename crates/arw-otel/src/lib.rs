use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::trace::TracerProvider as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::{
    fmt,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter,
};

static ACCESS_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();
#[cfg(feature = "otlp")]
static OTEL_PROVIDER: OnceCell<opentelemetry_sdk::trace::SdkTracerProvider> = OnceCell::new();

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    #[cfg(feature = "otlp")]
    {
        if std::env::var("ARW_OTEL").as_deref() == Ok("1") {
            match init_with_otlp(filter.clone()) {
                Ok(()) => return,
                Err(err) => {
                    tracing::warn!(
                        %err,
                        "failed to initialise OTLP exporter; falling back to console tracing"
                    );
                }
            }
        }
    }

    install_console(filter);
}

fn install_console(filter: EnvFilter) {
    let fmt_layer = fmt::layer();
    let registry = tracing_subscriber::registry().with(fmt_layer.with_filter(filter));
    if std::env::var("ARW_ACCESS_LOG_ROLL").ok().as_deref() == Some("1") {
        let dir = std::env::var("ARW_ACCESS_LOG_DIR")
            .ok()
            .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
            .unwrap_or_else(|| "logs".to_string());
        let prefix =
            std::env::var("ARW_ACCESS_LOG_PREFIX").unwrap_or_else(|_| "http-access".into());
        let rotation = std::env::var("ARW_ACCESS_LOG_ROTATION").unwrap_or_else(|_| "daily".into());
        if std::fs::create_dir_all(&dir).is_err() {
            tracing::warn!(directory = %dir, "failed to create access log directory");
        }
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
            .with_writer(nb)
            .with_filter(targets);
        let subscriber = registry.with(access_layer);
        let _ = subscriber.try_init();
    } else {
        let _ = registry.try_init();
    }
}

#[cfg(feature = "otlp")]
fn init_with_otlp(filter: EnvFilter) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use opentelemetry::{global, KeyValue};
    use opentelemetry_otlp::{WithExportConfig, OTEL_EXPORTER_OTLP_HEADERS};
    use opentelemetry_sdk::{
        propagation::TraceContextPropagator, trace::SdkTracerProvider, Resource,
    };
    use std::time::Duration;

    let endpoint =
        std::env::var("ARW_OTEL_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:4317".to_string());

    const OTEL_TRACES_HEADERS_ENV: &str = "OTEL_EXPORTER_OTLP_TRACES_HEADERS";

    if let Ok(raw_headers) = std::env::var("ARW_OTEL_HEADERS") {
        if std::env::var(OTEL_EXPORTER_OTLP_HEADERS).is_err() {
            std::env::set_var(OTEL_EXPORTER_OTLP_HEADERS, &raw_headers);
        }
        if std::env::var(OTEL_TRACES_HEADERS_ENV).is_err() {
            std::env::set_var(OTEL_TRACES_HEADERS_ENV, raw_headers);
        }
    }

    let mut exporter_builder = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.clone());

    if let Some(timeout_ms) = std::env::var("ARW_OTEL_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        exporter_builder = exporter_builder.with_timeout(Duration::from_millis(timeout_ms));
    }

    let exporter = exporter_builder.build()?;

    let service_name =
        std::env::var("ARW_OTEL_SERVICE_NAME").unwrap_or_else(|_| "arw-server".to_string());
    let resource = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", service_name),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new(
                "service.instance.id",
                std::env::var("ARW_NODE_ID")
                    .unwrap_or_else(|_| format!("pid-{}", std::process::id())),
            ),
        ])
        .build();

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let _ = OTEL_PROVIDER.set(tracer_provider.clone());
    global::set_tracer_provider(tracer_provider.clone());
    let tracer = tracer_provider.tracer("arw-server");

    global::set_text_map_propagator(TraceContextPropagator::new());

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let fmt_layer = fmt::layer();
    let registry = tracing_subscriber::registry()
        .with(fmt_layer.with_filter(filter.clone()))
        .with(otel_layer);

    if std::env::var("ARW_ACCESS_LOG_ROLL").ok().as_deref() == Some("1") {
        let dir = std::env::var("ARW_ACCESS_LOG_DIR")
            .ok()
            .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
            .unwrap_or_else(|| "logs".to_string());
        let prefix =
            std::env::var("ARW_ACCESS_LOG_PREFIX").unwrap_or_else(|_| "http-access".into());
        let rotation = std::env::var("ARW_ACCESS_LOG_ROTATION").unwrap_or_else(|_| "daily".into());
        if std::fs::create_dir_all(&dir).is_err() {
            tracing::warn!(directory = %dir, "failed to create access log directory");
        }
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
            .with_writer(nb)
            .with_filter(targets);
        let subscriber = registry.with(access_layer);
        subscriber.try_init()?;
    } else {
        registry.try_init()?;
    }

    tracing::info!(endpoint, "OTLP tracing exporter initialised");
    Ok(())
}

#[allow(dead_code)]
pub fn shutdown() {
    #[cfg(feature = "otlp")]
    {
        if let Some(provider) = OTEL_PROVIDER.get() {
            if let Err(err) = provider.shutdown() {
                tracing::warn!(%err, "failed to shut down OTLP tracer provider cleanly");
            }
        }
    }
}
