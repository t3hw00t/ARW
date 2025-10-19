use axum::http::StatusCode;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use utoipa::IntoParams;

use crate::{
    api::persona::{ensure_persona_telemetry, resolve_vibe_telemetry_scope},
    persona::PersonaVibeMetricsSnapshot,
    responses, AppState,
};

#[derive(Debug, Deserialize, Default, IntoParams)]
#[serde(default)]
pub struct PersonaListQuery {
    pub owner_kind: Option<String>,
    pub owner_ref: Option<String>,
    pub limit: Option<i64>,
}

pub(crate) fn persona_disabled_response() -> axum::response::Response {
    responses::problem_response(
        StatusCode::NOT_IMPLEMENTED,
        "Persona Disabled",
        Some("Operation requires ARW_PERSONA_ENABLE=1"),
    )
}

#[utoipa::path(
    get,
    path = "/state/persona",
    tag = "State",
    params(PersonaListQuery),
    responses(
        (status = 200, description = "Persona summaries", body = serde_json::Value),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn state_persona_list(
    State(state): State<AppState>,
    Query(query): Query<PersonaListQuery>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    let limit = query.limit.unwrap_or(50);
    match service
        .list_entries(query.owner_kind.clone(), query.owner_ref.clone(), limit)
        .await
    {
        Ok(entries) => Json(json!({ "items": entries })).into_response(),
        Err(err) => responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load personas",
            Some(&err.to_string()),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/state/persona/{id}",
    tag = "State",
    params(("id" = String, Path, description = "Persona identifier")),
    responses(
        (status = 200, description = "Persona details", body = serde_json::Value),
        (status = 404, description = "Persona not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn state_persona_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    match service.get_entry(id.clone()).await {
        Ok(Some(entry)) => Json(entry).into_response(),
        Ok(None) => responses::problem_response(
            StatusCode::NOT_FOUND,
            "Persona Not Found",
            Some("Persona id not found"),
        ),
        Err(err) => responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load persona",
            Some(&err.to_string()),
        ),
    }
}

#[derive(Debug, Deserialize, Default, IntoParams)]
#[serde(default)]
pub struct PersonaHistoryQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Default, Deserialize, IntoParams)]
#[serde(default)]
pub struct PersonaVibeHistoryQuery {
    pub limit: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/state/persona/{id}/history",
    tag = "State",
    params(
        ("id" = String, Path, description = "Persona identifier"),
        PersonaHistoryQuery
    ),
    responses(
        (status = 200, description = "Persona history", body = serde_json::Value),
        (status = 404, description = "Persona not found"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn state_persona_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PersonaHistoryQuery>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    match service.get_entry(id.clone()).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return responses::problem_response(
                StatusCode::NOT_FOUND,
                "Persona Not Found",
                Some("Persona id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona",
                Some(&err.to_string()),
            )
        }
    }

    let limit = query.limit.unwrap_or(50);
    match service.list_history(id.clone(), limit).await {
        Ok(history) => Json(json!({ "items": history })).into_response(),
        Err(err) => responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load persona history",
            Some(&err.to_string()),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/state/persona/{id}/vibe_history",
    tag = "State",
    params(
        ("id" = String, Path, description = "Persona identifier"),
        PersonaVibeHistoryQuery
    ),
    responses(
        (status = 200, description = "Persona vibe history", body = serde_json::Value),
        (status = 404, description = "Persona not found"),
        (status = 412, description = "Telemetry disabled for persona"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn state_persona_vibe_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PersonaVibeHistoryQuery>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    let entry = match service.get_entry(id.clone()).await {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            return responses::problem_response(
                StatusCode::NOT_FOUND,
                "Persona Not Found",
                Some("Persona id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona",
                Some(&err.to_string()),
            )
        }
    };

    let telemetry_config = match resolve_vibe_telemetry_scope(&entry) {
        Some(config) => config,
        None => {
            return responses::problem_response(
                StatusCode::PRECONDITION_REQUIRED,
                "Telemetry Disabled",
                Some("Persona vibe telemetry opt-in is disabled"),
            )
        }
    };

    if let Err(resp) = ensure_persona_telemetry(&state, &telemetry_config.scope).await {
        return resp;
    }

    let retain_max = crate::persona::vibe_sample_retain();
    let limit = query.limit.unwrap_or(retain_max).clamp(1, retain_max);
    match service.list_vibe_history(id.clone(), limit).await {
        Ok(history) => Json(json!({ "items": history, "retain_max": retain_max, "limit": limit }))
            .into_response(),
        Err(err) => responses::problem_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load persona vibe history",
            Some(&err.to_string()),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/state/persona/{id}/vibe_metrics",
    tag = "State",
    params(("id" = String, Path, description = "Persona identifier")),
    responses(
        (status = 200, description = "Persona vibe metrics", body = serde_json::Value),
        (status = 404, description = "Persona not found"),
        (status = 412, description = "Telemetry disabled for persona"),
        (status = 501, description = "Persona subsystem disabled")
    )
)]
pub async fn state_persona_vibe_metrics(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !state.persona_enabled() {
        return persona_disabled_response();
    }

    let service = match state.persona() {
        Some(service) => service,
        None => return persona_disabled_response(),
    };

    let entry = match service.get_entry(id.clone()).await {
        Ok(Some(entry)) => entry,
        Ok(None) => {
            return responses::problem_response(
                StatusCode::NOT_FOUND,
                "Persona Not Found",
                Some("Persona id not found"),
            )
        }
        Err(err) => {
            return responses::problem_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load persona",
                Some(&err.to_string()),
            )
        }
    };

    let telemetry_config = match resolve_vibe_telemetry_scope(&entry) {
        Some(config) => config,
        None => {
            return responses::problem_response(
                StatusCode::PRECONDITION_REQUIRED,
                "Telemetry Disabled",
                Some("Persona vibe telemetry opt-in is disabled"),
            )
        }
    };

    if let Err(resp) = ensure_persona_telemetry(&state, &telemetry_config.scope).await {
        return resp;
    }

    let snapshot: PersonaVibeMetricsSnapshot = service.vibe_metrics_snapshot(id.clone()).await;

    Json(snapshot).into_response()
}
