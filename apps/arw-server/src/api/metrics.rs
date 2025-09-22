use crate::{admin_ok, metrics::MetricsSummary, AppState};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::{extract::State, Json};
use serde_json::json;
use std::fmt::Write;

fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\n' => ' ',
            '\r' => ' ',
            '"' => '\'',
            _ => c,
        })
        .collect()
}

fn write_metric_line(
    out: &mut String,
    name: &str,
    labels: &[(&str, String)],
    value: impl std::fmt::Display,
) {
    if labels.is_empty() {
        let _ = writeln!(out, "{} {}", name, value);
    } else {
        let rendered_labels: Vec<String> = labels
            .iter()
            .map(|(k, v)| format!("{}=\"{}\"", k, sanitize_label(v)))
            .collect();
        let _ = writeln!(out, "{}{{{}}} {}", name, rendered_labels.join(","), value);
    }
}

fn render_prometheus(summary: &MetricsSummary, bus: &arw_events::BusStats) -> String {
    let mut out = String::new();
    out.push_str(
        "# HELP arw_bus_published_total Events published\n# TYPE arw_bus_published_total counter\n",
    );
    write_metric_line(&mut out, "arw_bus_published_total", &[], bus.published);
    out.push_str(
        "# HELP arw_bus_delivered_total Events delivered\n# TYPE arw_bus_delivered_total counter\n",
    );
    write_metric_line(&mut out, "arw_bus_delivered_total", &[], bus.delivered);
    out.push_str(
        "# HELP arw_bus_receivers Gauge of current receivers\n# TYPE arw_bus_receivers gauge\n",
    );
    write_metric_line(&mut out, "arw_bus_receivers", &[], bus.receivers);
    out.push_str(
        "# HELP arw_bus_lagged_total Lag signals observed\n# TYPE arw_bus_lagged_total counter\n",
    );
    write_metric_line(&mut out, "arw_bus_lagged_total", &[], bus.lagged);
    out.push_str("# HELP arw_bus_no_receivers_total Publishes with no receivers\n# TYPE arw_bus_no_receivers_total counter\n");
    write_metric_line(
        &mut out,
        "arw_bus_no_receivers_total",
        &[],
        bus.no_receivers,
    );

    out.push_str(
        "# HELP arw_events_total Events processed by kind\n# TYPE arw_events_total counter\n",
    );
    for (kind, count) in summary.events.kinds.iter() {
        write_metric_line(
            &mut out,
            "arw_events_total",
            &[("kind", kind.clone())],
            count,
        );
    }

    out.push_str(
        "# HELP arw_route_hits_total HTTP hits per route\n# TYPE arw_route_hits_total counter\n",
    );
    out.push_str("# HELP arw_route_errors_total HTTP errors per route\n# TYPE arw_route_errors_total counter\n");
    out.push_str("# HELP arw_route_ewma_ms Exponentially weighted latency (ms)\n# TYPE arw_route_ewma_ms gauge\n");
    out.push_str(
        "# HELP arw_route_p95_ms Rolling p95 latency (ms)\n# TYPE arw_route_p95_ms gauge\n",
    );
    out.push_str("# HELP arw_route_max_ms Max latency (ms)\n# TYPE arw_route_max_ms gauge\n");
    for (path, stat) in summary.routes.by_path.iter() {
        let labels = [("path", path.clone())];
        write_metric_line(&mut out, "arw_route_hits_total", &labels, stat.hits);
        write_metric_line(&mut out, "arw_route_errors_total", &labels, stat.errors);
        write_metric_line(&mut out, "arw_route_ewma_ms", &labels, stat.ewma_ms);
        write_metric_line(&mut out, "arw_route_p95_ms", &labels, stat.p95_ms);
        write_metric_line(&mut out, "arw_route_max_ms", &labels, stat.max_ms);
    }

    out.push_str("# HELP arw_task_inflight Current background task inflight count\n# TYPE arw_task_inflight gauge\n");
    out.push_str("# HELP arw_task_started_total Background task start events\n# TYPE arw_task_started_total counter\n");
    out.push_str("# HELP arw_task_completed_total Background task completions\n# TYPE arw_task_completed_total counter\n");
    out.push_str("# HELP arw_task_aborted_total Background task aborts\n# TYPE arw_task_aborted_total counter\n");
    for (task, status) in summary.tasks.iter() {
        let labels = [("task", task.clone())];
        write_metric_line(&mut out, "arw_task_inflight", &labels, status.inflight);
        write_metric_line(&mut out, "arw_task_started_total", &labels, status.started);
        write_metric_line(
            &mut out,
            "arw_task_completed_total",
            &labels,
            status.completed,
        );
        write_metric_line(&mut out, "arw_task_aborted_total", &labels, status.aborted);
    }
    out.push_str("# HELP arw_legacy_capsule_headers_total Legacy capsule headers rejected\n# TYPE arw_legacy_capsule_headers_total counter\n");
    write_metric_line(
        &mut out,
        "arw_legacy_capsule_headers_total",
        &[],
        summary.compatibility.legacy_capsule_headers,
    );
    out
}

#[utoipa::path(
    get,
    path = "/metrics",
    tag = "Public",
    responses((status = 200, description = "Prometheus metrics", content_type = "text/plain", body = String))
)]
pub async fn metrics_prometheus(State(state): State<AppState>) -> Response {
    let summary = state.metrics().snapshot();
    let bus_stats = state.bus().stats();
    let body = render_prometheus(&summary, &bus_stats);
    let mut response = Response::new(body.into());
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    response
}

#[utoipa::path(
    get,
    path = "/admin/introspect/stats",
    tag = "Admin/Introspect",
    responses(
        (status = 200, description = "Metrics snapshot", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn metrics_overview(headers: HeaderMap, State(state): State<AppState>) -> Response {
    if !admin_ok(&headers) {
        return unauthorized();
    }
    let summary = state.metrics().snapshot();
    let bus = state.bus().stats();
    Json(json!({
        "events": summary.events,
        "routes": summary.routes,
        "tasks": summary.tasks,
        "compatibility": summary.compatibility,
        "bus": {
            "published": bus.published,
            "delivered": bus.delivered,
            "receivers": bus.receivers,
            "lagged": bus.lagged,
            "no_receivers": bus.no_receivers,
        }
    }))
    .into_response()
}
