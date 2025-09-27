use crate::{
    admin_ok,
    metrics::{cache_stats_snapshot, MetricsSummary},
    tool_cache::ToolCacheStats,
    AppState,
};
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

fn render_prometheus(
    summary: &MetricsSummary,
    bus: &arw_events::BusStats,
    cache: &ToolCacheStats,
) -> String {
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
    out.push_str("# HELP arw_memory_gc_expired_total Memory records reclaimed because TTL expired\n# TYPE arw_memory_gc_expired_total counter\n");
    write_metric_line(
        &mut out,
        "arw_memory_gc_expired_total",
        &[],
        summary.memory_gc.expired_total,
    );
    out.push_str("# HELP arw_memory_gc_evicted_total Memory records reclaimed because lanes exceeded caps\n# TYPE arw_memory_gc_evicted_total counter\n");
    write_metric_line(
        &mut out,
        "arw_memory_gc_evicted_total",
        &[],
        summary.memory_gc.evicted_total,
    );
    out.push_str("# HELP arw_legacy_capsule_headers_total Legacy capsule headers rejected\n# TYPE arw_legacy_capsule_headers_total counter\n");
    write_metric_line(
        &mut out,
        "arw_legacy_capsule_headers_total",
        &[],
        summary.compatibility.legacy_capsule_headers,
    );

    out.push_str("# HELP arw_tool_cache_hits_total Tool cache hits\n# TYPE arw_tool_cache_hits_total counter\n");
    write_metric_line(&mut out, "arw_tool_cache_hits_total", &[], cache.hit);
    out.push_str("# HELP arw_tool_cache_miss_total Tool cache misses\n# TYPE arw_tool_cache_miss_total counter\n");
    write_metric_line(&mut out, "arw_tool_cache_miss_total", &[], cache.miss);
    out.push_str("# HELP arw_tool_cache_coalesced_total Tool cache coalesced waiters\n# TYPE arw_tool_cache_coalesced_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_coalesced_total",
        &[],
        cache.coalesced,
    );
    out.push_str("# HELP arw_tool_cache_errors_total Tool cache errors\n# TYPE arw_tool_cache_errors_total counter\n");
    write_metric_line(&mut out, "arw_tool_cache_errors_total", &[], cache.errors);
    out.push_str("# HELP arw_tool_cache_bypass_total Tool cache bypasses\n# TYPE arw_tool_cache_bypass_total counter\n");
    write_metric_line(&mut out, "arw_tool_cache_bypass_total", &[], cache.bypass);
    out.push_str("# HELP arw_tool_cache_entries Tool cache entry count\n# TYPE arw_tool_cache_entries gauge\n");
    write_metric_line(&mut out, "arw_tool_cache_entries", &[], cache.entries);
    out.push_str("# HELP arw_tool_cache_latency_saved_ms_total Latency saved via cache (ms)\n# TYPE arw_tool_cache_latency_saved_ms_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_latency_saved_ms_total",
        &[],
        cache.latency_saved_ms_total,
    );
    out.push_str("# HELP arw_tool_cache_latency_saved_samples_total Latency saved samples\n# TYPE arw_tool_cache_latency_saved_samples_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_latency_saved_samples_total",
        &[],
        cache.latency_saved_samples,
    );
    out.push_str("# HELP arw_tool_cache_payload_bytes_saved_total Payload bytes saved via cache\n# TYPE arw_tool_cache_payload_bytes_saved_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_payload_bytes_saved_total",
        &[],
        cache.payload_bytes_saved_total,
    );
    out.push_str("# HELP arw_tool_cache_payload_saved_samples_total Payload saved samples\n# TYPE arw_tool_cache_payload_saved_samples_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_payload_saved_samples_total",
        &[],
        cache.payload_saved_samples,
    );
    out.push_str("# HELP arw_tool_cache_avg_latency_saved_ms Average latency saved per sample\n# TYPE arw_tool_cache_avg_latency_saved_ms gauge\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_avg_latency_saved_ms",
        &[],
        cache.avg_latency_saved_ms,
    );
    out.push_str("# HELP arw_tool_cache_avg_payload_bytes_saved Average payload bytes saved per sample\n# TYPE arw_tool_cache_avg_payload_bytes_saved gauge\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_avg_payload_bytes_saved",
        &[],
        cache.avg_payload_bytes_saved,
    );
    out.push_str("# HELP arw_tool_cache_avg_hit_age_secs Average age of cache hits\n# TYPE arw_tool_cache_avg_hit_age_secs gauge\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_avg_hit_age_secs",
        &[],
        cache.avg_hit_age_secs,
    );
    out.push_str("# HELP arw_tool_cache_hit_age_samples_total Cache hit age samples\n# TYPE arw_tool_cache_hit_age_samples_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_hit_age_samples_total",
        &[],
        cache.hit_age_samples,
    );
    out.push_str("# HELP arw_tool_cache_last_hit_age_secs Last observed cache hit age\n# TYPE arw_tool_cache_last_hit_age_secs gauge\n");
    if let Some(last_age) = cache.last_hit_age_secs {
        write_metric_line(&mut out, "arw_tool_cache_last_hit_age_secs", &[], last_age);
    }
    out.push_str("# HELP arw_tool_cache_max_hit_age_secs Max observed cache hit age\n# TYPE arw_tool_cache_max_hit_age_secs gauge\n");
    if let Some(max_age) = cache.max_hit_age_secs {
        write_metric_line(&mut out, "arw_tool_cache_max_hit_age_secs", &[], max_age);
    }
    out.push_str("# HELP arw_tool_cache_stampede_suppression_rate Stampede suppression rate\n# TYPE arw_tool_cache_stampede_suppression_rate gauge\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_stampede_suppression_rate",
        &[],
        cache.stampede_suppression_rate,
    );
    out.push_str("# HELP arw_tool_cache_last_latency_saved_ms Last observed latency saved\n# TYPE arw_tool_cache_last_latency_saved_ms gauge\n");
    if let Some(last_saved) = cache.last_latency_saved_ms {
        write_metric_line(
            &mut out,
            "arw_tool_cache_last_latency_saved_ms",
            &[],
            last_saved,
        );
    }
    out.push_str("# HELP arw_tool_cache_last_payload_bytes Last observed payload bytes\n# TYPE arw_tool_cache_last_payload_bytes gauge\n");
    if let Some(last_payload) = cache.last_payload_bytes {
        write_metric_line(
            &mut out,
            "arw_tool_cache_last_payload_bytes",
            &[],
            last_payload,
        );
    }
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
    let cache_stats = state.tool_cache().stats();
    let body = render_prometheus(&summary, &bus_stats, &cache_stats);
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
    let cache_stats = state.tool_cache().stats();
    Json(json!({
        "events": summary.events,
        "routes": summary.routes,
        "tasks": summary.tasks,
        "compatibility": summary.compatibility,
        "cache": cache_stats_snapshot(&cache_stats),
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
