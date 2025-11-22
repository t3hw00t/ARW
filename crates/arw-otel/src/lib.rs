use arw_core::util::env_bool;
use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::trace::TracerProvider as _;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::{
    fmt,
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter,
};

static LOG_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();
static ACCESS_GUARD: OnceCell<tracing_appender::non_blocking::WorkerGuard> = OnceCell::new();
#[cfg(feature = "otlp")]
static OTEL_PROVIDER: OnceCell<opentelemetry_sdk::trace::SdkTracerProvider> = OnceCell::new();
#[cfg(feature = "otlp")]
static OTEL_METRICS_PROVIDER: OnceCell<opentelemetry_sdk::metrics::SdkMeterProvider> =
    OnceCell::new();

fn resolve_logs_dir() -> String {
    std::env::var("ARW_LOGS_DIR")
        .ok()
        .unwrap_or_else(|| arw_core::effective_paths().logs_dir)
}

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    #[cfg(feature = "otlp")]
    {
        if env_bool("ARW_OTEL").unwrap_or(false) {
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
    let logs_dir = resolve_logs_dir();
    if let Err(err) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("warn: failed to create log directory {logs_dir}: {err}");
        let fmt_layer = fmt::layer().with_filter(filter.clone());
        if env_bool("ARW_ACCESS_LOG_ROLL").unwrap_or(false) {
            let registry = tracing_subscriber::registry().with(fmt_layer);
            let dir = std::env::var("ARW_ACCESS_LOG_DIR")
                .ok()
                .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
                .unwrap_or_else(resolve_logs_dir);
            let prefix =
                std::env::var("ARW_ACCESS_LOG_PREFIX").unwrap_or_else(|_| "http-access".into());
            let rotation =
                std::env::var("ARW_ACCESS_LOG_ROTATION").unwrap_or_else(|_| "daily".into());
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
            let registry = tracing_subscriber::registry().with(fmt_layer);
            let _ = registry.try_init();
        }
        return;
    }

    let file_appender = tracing_appender::rolling::daily(&logs_dir, "arw.log");
    let (file_nb, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);
    // Fan out to stdout and file so even aborted runs leave a log artifact.
    let writer = std::io::stdout.and(file_nb);
    let fmt_layer = fmt::layer().with_writer(writer).with_filter(filter.clone());
    if env_bool("ARW_ACCESS_LOG_ROLL").unwrap_or(false) {
        let registry = tracing_subscriber::registry().with(fmt_layer);
        let dir = std::env::var("ARW_ACCESS_LOG_DIR")
            .ok()
            .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
            .unwrap_or_else(resolve_logs_dir);
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
        let registry = tracing_subscriber::registry().with(fmt_layer);
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
    const OTEL_METRICS_HEADERS_ENV: &str = "OTEL_EXPORTER_OTLP_METRICS_HEADERS";

    if let Ok(raw_headers) = std::env::var("ARW_OTEL_HEADERS") {
        if std::env::var(OTEL_EXPORTER_OTLP_HEADERS).is_err() {
            std::env::set_var(OTEL_EXPORTER_OTLP_HEADERS, &raw_headers);
        }
        if std::env::var(OTEL_TRACES_HEADERS_ENV).is_err() {
            std::env::set_var(OTEL_TRACES_HEADERS_ENV, raw_headers.clone());
        }
        if std::env::var(OTEL_METRICS_HEADERS_ENV).is_err() {
            std::env::set_var(OTEL_METRICS_HEADERS_ENV, raw_headers);
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

    #[cfg(feature = "otlp")]
    if env_bool("ARW_OTEL_METRICS").unwrap_or(false) {
        use metrics_exporter_opentelemetry::Recorder as MetricsRecorder;
        use opentelemetry_otlp::MetricExporterBuilder;
        use opentelemetry_sdk::metrics::Temporality;

        let mut metrics_exporter_builder = MetricExporterBuilder::new()
            .with_tonic()
            .with_endpoint(endpoint.clone());
        if let Some(timeout_ms) = std::env::var("ARW_OTEL_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            metrics_exporter_builder =
                metrics_exporter_builder.with_timeout(Duration::from_millis(timeout_ms));
        }

        match metrics_exporter_builder
            .with_temporality(Temporality::Cumulative)
            .build()
        {
            Ok(exporter) => match MetricsRecorder::builder("arw-server")
                .with_instrumentation_scope(|scope| scope.with_version(env!("CARGO_PKG_VERSION")))
                .with_meter_provider(|builder| {
                    builder
                        .with_resource(resource.clone())
                        .with_periodic_exporter(exporter)
                })
                .install()
            {
                Ok((provider, _)) => {
                    opentelemetry::global::set_meter_provider(provider.clone());
                    let _ = OTEL_METRICS_PROVIDER.set(provider);
                    tracing::info!(endpoint, "OTLP metrics exporter initialised");
                }
                Err(err) => {
                    tracing::warn!(
                        %err,
                        "failed to initialise OTLP metrics exporter; continuing without metrics pipeline"
                    );
                }
            },
            Err(err) => {
                tracing::warn!(
                    %err,
                    "failed to build OTLP metrics exporter; continuing without metrics pipeline"
                );
            }
        }
    }

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource.clone())
        .build();

    let _ = OTEL_PROVIDER.set(tracer_provider.clone());
    global::set_tracer_provider(tracer_provider.clone());
    let tracer = tracer_provider.tracer("arw-server");

    global::set_text_map_propagator(TraceContextPropagator::new());

    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let logs_dir = resolve_logs_dir();
    if let Err(err) = std::fs::create_dir_all(&logs_dir) {
        eprintln!("warn: failed to create log directory {logs_dir}: {err}");
        let fmt_layer = fmt::layer().with_filter(filter.clone());
        let registry = tracing_subscriber::registry()
            .with(fmt_layer)
            .with(otel_layer);
        registry.try_init()?;
        tracing::info!(endpoint, "OTLP tracing exporter initialised");
        return Ok(());
    }

    let file_appender = tracing_appender::rolling::daily(&logs_dir, "arw.log");
    let (file_nb, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);
    let writer = std::io::stdout.and(file_nb);
    let fmt_layer = fmt::layer().with_writer(writer).with_filter(filter.clone());
    let registry = tracing_subscriber::registry()
        .with(fmt_layer)
        .with(otel_layer);

    if env_bool("ARW_ACCESS_LOG_ROLL").unwrap_or(false) {
        let dir = std::env::var("ARW_ACCESS_LOG_DIR")
            .ok()
            .or_else(|| std::env::var("ARW_LOGS_DIR").ok())
            .unwrap_or_else(resolve_logs_dir);
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
        if let Some(provider) = OTEL_METRICS_PROVIDER.get() {
            if let Err(err) = provider.shutdown() {
                tracing::warn!(%err, "failed to shut down OTLP metrics provider cleanly");
            }
        }
    }
}
