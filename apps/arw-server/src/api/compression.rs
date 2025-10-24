use axum::http::StatusCode;
use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{error, warn};
use utoipa::ToSchema;

use arw_compress::{Budget, CompressionMode};

use crate::{metrics::CompressionPromptSample, responses, AppState};
use std::time::Instant;

#[derive(Debug, Deserialize, ToSchema)]
pub struct PromptCompressRequest {
    pub text: String,
    #[serde(default)]
    pub target_tokens: Option<usize>,
    #[serde(default)]
    pub ratio: Option<f32>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub extras: Option<Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PromptCompressResponse {
    pub compressor: String,
    pub compressed_text: String,
    pub ratio: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept_spans: Option<Value>,
    pub meta: Value,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MemoryBuildTreeRequest {
    pub corpus_id: String,
    #[serde(default)]
    pub fanout: Option<usize>,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub summarizer_model: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MemoryCodebookRequest {
    pub corpus_id: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub bits: Option<u32>,
}

#[utoipa::path(
    post,
    path = "/v1/compress/prompt",
    tag = "Compression",
    responses(
        (status = 200, description = "Compressed prompt", body = PromptCompressResponse),
        (status = 400, description = "Invalid request", body = arw_protocol::ProblemDetails),
        (status = 502, description = "Compression backend failure", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn compress_prompt(
    State(state): State<AppState>,
    Json(request): Json<PromptCompressRequest>,
) -> axum::response::Response {
    if request.text.trim().is_empty() {
        return responses::problem_response(
            StatusCode::BAD_REQUEST,
            "Invalid Request",
            Some("text must not be empty"),
        );
    }

    let mut budget = Budget::default();
    if let Some(target) = request.target_tokens {
        if target == 0 {
            return responses::problem_response(
                StatusCode::BAD_REQUEST,
                "Invalid Request",
                Some("target_tokens must be positive"),
            );
        }
        budget = budget.with_target_tokens(target);
    }
    if let Some(ratio) = request.ratio {
        if !(0.0..=1.0).contains(&ratio) {
            return responses::problem_response(
                StatusCode::BAD_REQUEST,
                "Invalid Request",
                Some("ratio must be within [0,1]"),
            );
        }
        budget = budget.with_ratio(ratio);
    }
    if budget.ratio.is_none() && budget.target_tokens.is_none() {
        budget = budget.with_ratio(0.5);
    }
    budget.mode = request.mode.as_deref().and_then(parse_mode);
    if let Some(extras) = request.extras.clone() {
        budget.extras = Some(extras);
    }

    let prompt_service = state.compression().prompt();
    let input = request.text;
    let pre_chars = input.chars().count() as u64;
    let pre_bytes = input.len() as u64;
    let started_at = Instant::now();
    let budget_clone = budget.clone();
    match prompt_service.compress(input, budget_clone).await {
        Ok(blob) => {
            let latency_ms = started_at.elapsed().as_secs_f64() * 1000.0;
            let compressed_text = match blob.to_text() {
                Ok(text) => text,
                Err(err) => {
                    error!(
                        target = "arw::compression",
                        error = %err,
                        "failed to decode compressed prompt"
                    );
                    return responses::problem_response(
                        StatusCode::BAD_GATEWAY,
                        "Compression Failed",
                        Some("Compressed payload was not UTF-8"),
                    );
                }
            };
            let meta = blob.meta.clone();
            let post_chars = compressed_text.chars().count() as u64;
            let post_bytes = compressed_text.len() as u64;
            let compressor_id = meta
                .get("compressor")
                .and_then(|v| v.as_str())
                .unwrap_or("noop.prompt")
                .to_string();
            let applied_ratio = meta
                .get("applied_ratio")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or_else(|| budget.fallback_ratio(1.0));
            let kept_spans = meta.get("kept_spans").cloned().filter(|v| !v.is_null());
            let fallback = meta
                .get("fallback")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            metrics::counter!("arw.compression.prompt.requests").increment(1);
            metrics::histogram!("arw.compression.prompt.latency_ms").record(latency_ms);
            metrics::histogram!("arw.compression.prompt.ratio").record(applied_ratio as f64);
            metrics::histogram!("arw.compression.prompt.pre_chars").record(pre_chars as f64);
            metrics::histogram!("arw.compression.prompt.post_chars").record(post_chars as f64);
            metrics::histogram!("arw.compression.prompt.pre_bytes").record(pre_bytes as f64);
            metrics::histogram!("arw.compression.prompt.post_bytes").record(post_bytes as f64);
            if fallback {
                metrics::counter!("arw.compression.prompt.fallbacks").increment(1);
            } else {
                metrics::counter!("arw.compression.prompt.primary").increment(1);
            }

            state
                .metrics()
                .record_prompt_compression_success(CompressionPromptSample {
                    latency_ms,
                    ratio: applied_ratio as f64,
                    pre_chars,
                    post_chars,
                    pre_bytes,
                    post_bytes,
                    fallback,
                });

            responses::json_ok(PromptCompressResponse {
                compressor: compressor_id,
                compressed_text,
                ratio: applied_ratio,
                kept_spans,
                meta,
            })
            .into_response()
        }
        Err(err) => {
            error!(
                target = "arw::compression",
                error = %err,
                "prompt compression failed"
            );
            metrics::counter!("arw.compression.prompt.errors").increment(1);
            state.metrics().record_prompt_compression_error();
            responses::problem_response(
                StatusCode::BAD_GATEWAY,
                "Compression Failed",
                Some("Prompt compression backend unavailable"),
            )
        }
    }
}

fn parse_mode(mode: &str) -> Option<CompressionMode> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "extractive" | "extract" | "extractive_summary" => Some(CompressionMode::Extractive),
        "abstractive" | "abstract" | "rewrite" => Some(CompressionMode::Abstractive),
        _ => None,
    }
}

#[utoipa::path(
    post,
    path = "/v1/compress/memory/build_tree",
    tag = "Compression",
    responses(
        (status = 202, description = "Memory compression scheduled", body = serde_json::Value),
        (status = 501, description = "Memory compression unavailable", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn compress_memory_build_tree(
    State(_state): State<AppState>,
    Json(request): Json<MemoryBuildTreeRequest>,
) -> impl IntoResponse {
    warn!(
        target = "arw::compression",
        corpus = %request.corpus_id,
        fanout = ?request.fanout,
        depth = ?request.depth,
        summarizer = ?request.summarizer_model,
        "RAPTOR tree builder not yet implemented"
    );
    responses::problem_response(
        StatusCode::NOT_IMPLEMENTED,
        "Memory Compression Unavailable",
        Some("RAPTOR memory builder not wired yet"),
    )
}

#[utoipa::path(
    post,
    path = "/v1/compress/memory/codebook",
    tag = "Compression",
    responses(
        (status = 202, description = "Memory codebook scheduled", body = serde_json::Value),
        (status = 501, description = "Memory compression unavailable", body = arw_protocol::ProblemDetails)
    )
)]
pub async fn compress_memory_codebook(
    State(_state): State<AppState>,
    Json(request): Json<MemoryCodebookRequest>,
) -> impl IntoResponse {
    warn!(
        target = "arw::compression",
        corpus = %request.corpus_id,
        method = ?request.method,
        bits = ?request.bits,
        "memory codebook builder not yet implemented"
    );
    responses::problem_response(
        StatusCode::NOT_IMPLEMENTED,
        "Memory Compression Unavailable",
        Some("Codebook compaction is not yet available"),
    )
}
