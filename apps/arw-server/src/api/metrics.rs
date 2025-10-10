use crate::{metrics::MetricsSummary, tool_cache::ToolCacheStats, AppState};
use axum::extract::State;
use axum::http::HeaderValue;
use axum::response::Response;
use std::fmt::Write;

//

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
        "# HELP arw_workers_configured Configured worker pool size\n# TYPE arw_workers_configured gauge\n",
    );
    write_metric_line(
        &mut out,
        "arw_workers_configured",
        &[],
        summary.worker.configured,
    );
    out.push_str(
        "# HELP arw_workers_busy Current number of workers processing actions\n# TYPE arw_workers_busy gauge\n",
    );
    write_metric_line(&mut out, "arw_workers_busy", &[], summary.worker.busy);
    out.push_str(
        "# HELP arw_worker_jobs_started_total Actions started by workers\n# TYPE arw_worker_jobs_started_total counter\n",
    );
    write_metric_line(
        &mut out,
        "arw_worker_jobs_started_total",
        &[],
        summary.worker.started,
    );
    out.push_str(
        "# HELP arw_worker_jobs_completed_total Actions completed by workers\n# TYPE arw_worker_jobs_completed_total counter\n",
    );
    write_metric_line(
        &mut out,
        "arw_worker_jobs_completed_total",
        &[],
        summary.worker.completed,
    );
    out.push_str(
        "# HELP arw_actions_queue_depth Estimated queued actions awaiting workers\n# TYPE arw_actions_queue_depth gauge\n",
    );
    write_metric_line(
        &mut out,
        "arw_actions_queue_depth",
        &[],
        summary.worker.queue_depth,
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
        "# HELP arw_autonomy_interrupts_total Autonomy interrupts by reason\n# TYPE arw_autonomy_interrupts_total counter\n",
    );
    for (reason, count) in summary.autonomy.interrupts.iter() {
        write_metric_line(
            &mut out,
            "arw_autonomy_interrupts_total",
            &[("reason", reason.clone())],
            count,
        );
    }

    out.push_str(
        "# HELP arw_modular_agent_total Modular agent responses accepted\n# TYPE arw_modular_agent_total counter\n",
    );
    for (agent, count) in summary.modular.agent_totals.iter() {
        write_metric_line(
            &mut out,
            "arw_modular_agent_total",
            &[("agent", agent.clone())],
            count,
        );
    }
    out.push_str(
        "# HELP arw_modular_tool_total Modular tool invocations accepted\n# TYPE arw_modular_tool_total counter\n",
    );
    for (tool, count) in summary.modular.tool_totals.iter() {
        write_metric_line(
            &mut out,
            "arw_modular_tool_total",
            &[("tool", tool.clone())],
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
        let base_labels = vec![("path", path.clone())];
        write_metric_line(&mut out, "arw_route_hits_total", &base_labels, stat.hits);
        write_metric_line(
            &mut out,
            "arw_route_errors_total",
            &base_labels,
            stat.errors,
        );
        write_metric_line(&mut out, "arw_route_ewma_ms", &base_labels, stat.ewma_ms);
        write_metric_line(&mut out, "arw_route_p95_ms", &base_labels, stat.p95_ms);
        write_metric_line(&mut out, "arw_route_max_ms", &base_labels, stat.max_ms);
    }
    out.push_str(
        "# HELP arw_route_latency_seconds HTTP route latency histogram (seconds)\\n# TYPE arw_route_latency_seconds histogram\\n",
    );
    for (path, stat) in summary.routes.by_path.iter() {
        if let Some(hist) = &stat.latency_histogram {
            let base_labels = vec![("path", path.clone())];
            for bucket in &hist.buckets {
                let le_value = match bucket.le_ms {
                    Some(ms) => format!("{:.3}", ms / 1000.0),
                    None => "+Inf".to_string(),
                };
                let bucket_labels = vec![("path", path.clone()), ("le", le_value)];
                write_metric_line(
                    &mut out,
                    "arw_route_latency_seconds_bucket",
                    &bucket_labels,
                    bucket.count,
                );
            }
            write_metric_line(
                &mut out,
                "arw_route_latency_seconds_sum",
                &base_labels,
                hist.sum_ms / 1000.0,
            );
            write_metric_line(
                &mut out,
                "arw_route_latency_seconds_count",
                &base_labels,
                hist.count,
            );
        }
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
        write_metric_line(
            &mut out,
            "arw_task_restarts_window",
            &labels,
            status.restarts_window,
        );
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
    out.push_str("# HELP arw_tool_cache_payload_too_large_total Tool cache skips due to payload size\n# TYPE arw_tool_cache_payload_too_large_total counter\n");
    write_metric_line(
        &mut out,
        "arw_tool_cache_payload_too_large_total",
        &[],
        cache.payload_too_large,
    );
    out.push_str("# HELP arw_tool_cache_entries Tool cache entry count\n# TYPE arw_tool_cache_entries gauge\n");
    write_metric_line(&mut out, "arw_tool_cache_entries", &[], cache.entries);
    out.push_str("# HELP arw_tool_cache_max_payload_bytes Configured per-entry payload limit (0 when disabled)\n# TYPE arw_tool_cache_max_payload_bytes gauge\n");
    let limit_bytes = cache.max_payload_bytes.unwrap_or(0);
    write_metric_line(
        &mut out,
        "arw_tool_cache_max_payload_bytes",
        &[],
        limit_bytes,
    );
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

    // Safe-mode and crash visibility (best-effort)
    out.push_str("# HELP arw_safe_mode_active Safe-mode engaged due to recent crash markers\n# TYPE arw_safe_mode_active gauge\n");
    let until_ms = crate::crashguard::safe_mode_until_ms();
    write_metric_line(
        &mut out,
        "arw_safe_mode_active",
        &[],
        if until_ms > 0 { 1 } else { 0 },
    );
    out.push_str("# HELP arw_safe_mode_until_ms Epoch milliseconds until safe-mode ends (0 if inactive)\n# TYPE arw_safe_mode_until_ms gauge\n");
    write_metric_line(&mut out, "arw_safe_mode_until_ms", &[], until_ms);

    out.push_str("# HELP arw_last_crash_ms Timestamp of last known crash marker (ms since epoch, 0 if none)\n# TYPE arw_last_crash_ms gauge\n");
    let last_crash_ms = crate::read_models::cached_read_model("crashlog")
        .and_then(|v| {
            v.get("items")
                .and_then(|i| i.as_array())
                .and_then(|arr| arr.first().cloned())
        })
        .and_then(|item| item.get("ts_ms").and_then(|t| t.as_u64()))
        .unwrap_or(0);
    write_metric_line(&mut out, "arw_last_crash_ms", &[], last_crash_ms);
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

// legacy metrics_overview removed; use /state/route_stats instead

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cache_stats() -> ToolCacheStats {
        ToolCacheStats {
            hit: 0,
            miss: 0,
            coalesced: 0,
            errors: 0,
            bypass: 0,
            payload_too_large: 0,
            capacity: 0,
            ttl_secs: 0,
            entries: 0,
            max_payload_bytes: None,
            latency_saved_ms_total: 0,
            latency_saved_samples: 0,
            avg_latency_saved_ms: 0.0,
            payload_bytes_saved_total: 0,
            payload_saved_samples: 0,
            avg_payload_bytes_saved: 0.0,
            avg_hit_age_secs: 0.0,
            hit_age_samples: 0,
            last_hit_age_secs: None,
            max_hit_age_secs: None,
            stampede_suppression_rate: 0.0,
            last_latency_saved_ms: None,
            last_payload_bytes: None,
        }
    }

    #[test]
    fn prometheus_export_includes_histogram() {
        let metrics = crate::metrics::Metrics::new();
        metrics.record_route("/demo", 200, 8);
        metrics.record_route("/demo", 200, 42);
        metrics.record_route("/demo", 200, 1200);

        let summary = metrics.snapshot();
        let bus = arw_events::BusStats {
            published: 0,
            delivered: 0,
            lagged: 0,
            no_receivers: 0,
            receivers: 0,
        };
        let cache = empty_cache_stats();
        let rendered = render_prometheus(&summary, &bus, &cache);

        assert!(
            rendered.contains("arw_route_latency_seconds_bucket{path=\"/demo\",le=\"0.010\"} 1")
        );
        assert!(
            rendered.contains("arw_route_latency_seconds_bucket{path=\"/demo\",le=\"0.050\"} 2")
        );
        assert!(rendered.contains("arw_route_latency_seconds_bucket{path=\"/demo\",le=\"+Inf\"} 3"));
        assert!(rendered.contains("arw_route_latency_seconds_sum{path=\"/demo\"} 1.25"));
        assert!(rendered.contains("arw_route_latency_seconds_count{path=\"/demo\"} 3"));
    }
}
