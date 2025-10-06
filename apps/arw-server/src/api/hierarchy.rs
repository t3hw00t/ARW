use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::{extract::State, Json};
use serde::Deserialize;
use serde_json::json;
use utoipa::ToSchema;

use crate::{admin_ok, responses, AppState};
use arw_core::hierarchy as hier;
use arw_protocol::{CoreAccept, CoreHello, CoreOffer, CoreRole};
use arw_topics as topics;

fn unauthorized() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        Json(json!({"type":"about:blank","title":"Unauthorized","status":401})),
    )
        .into_response()
}

fn map_role(role: CoreRole) -> hier::Role {
    match role {
        CoreRole::Root => hier::Role::Root,
        CoreRole::Regional => hier::Role::Regional,
        CoreRole::Edge => hier::Role::Edge,
        CoreRole::Connector => hier::Role::Connector,
        CoreRole::Observer => hier::Role::Observer,
    }
}

fn apply_role_defaults(role: hier::Role) {
    let gate_role = match role {
        hier::Role::Root => arw_core::gating::Role::Root,
        hier::Role::Regional => arw_core::gating::Role::Regional,
        hier::Role::Edge => arw_core::gating::Role::Edge,
        hier::Role::Connector => arw_core::gating::Role::Connector,
        hier::Role::Observer => arw_core::gating::Role::Observer,
    };
    arw_core::gating::apply_role_defaults(gate_role);
}

#[utoipa::path(
    get,
    path = "/admin/hierarchy/state",
    tag = "Admin/Hierarchy",
    responses(
        (status = 200, description = "Hierarchy state", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn hierarchy_state(
    headers: HeaderMap,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let st = hier::get_state();
    let mut payload = json!({"epoch": st.epoch});
    responses::attach_corr(&mut payload);
    state.bus().publish(topics::TOPIC_HIERARCHY_STATE, &payload);
    Json(st).into_response()
}

#[derive(Deserialize, ToSchema)]
pub struct RoleRequest {
    pub role: String,
}

#[utoipa::path(
    post,
    path = "/admin/hierarchy/role",
    tag = "Admin/Hierarchy",
    request_body = RoleRequest,
    responses(
        (status = 200, description = "Role updated", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn hierarchy_role_set(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<RoleRequest>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let role = match req.role.as_str() {
        "root" => hier::Role::Root,
        "regional" => hier::Role::Regional,
        "edge" => hier::Role::Edge,
        "connector" => hier::Role::Connector,
        _ => hier::Role::Observer,
    };
    hier::set_role(role);
    apply_role_defaults(role);
    let mut payload = json!({"role": req.role});
    responses::attach_corr(&mut payload);
    state
        .bus()
        .publish(topics::TOPIC_HIERARCHY_ROLE_CHANGED, &payload);
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/hierarchy/hello",
    tag = "Admin/Hierarchy",
    request_body = CoreHello,
    responses(
        (status = 200, description = "Hello processed", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn hierarchy_hello(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<CoreHello>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    hier::configure_self(req.id.clone(), req.scope_tags.clone());
    let role = map_role(req.role.clone());
    hier::set_role(role);
    apply_role_defaults(role);
    state.bus().publish(topics::TOPIC_HIERARCHY_HELLO, &req);
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/hierarchy/offer",
    tag = "Admin/Hierarchy",
    request_body = CoreOffer,
    responses(
        (status = 200, description = "Offer processed", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn hierarchy_offer(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<CoreOffer>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let self_id = hier::get_state().self_node.id;
    if let Some(parent) = req.parent_hint.as_deref() {
        if parent == self_id.as_str() {
            hier::add_child(req.from_id.clone());
        }
    }
    state.bus().publish(topics::TOPIC_HIERARCHY_OFFER, &req);
    Json(json!({"ok": true})).into_response()
}

#[utoipa::path(
    post,
    path = "/admin/hierarchy/accept",
    tag = "Admin/Hierarchy",
    request_body = CoreAccept,
    responses(
        (status = 200, description = "Accept processed", body = serde_json::Value),
        (status = 401, description = "Unauthorized")
    )
)]
pub async fn hierarchy_accept(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(req): Json<CoreAccept>,
) -> impl IntoResponse {
    if !admin_ok(&headers).await {
        return unauthorized();
    }
    let self_id = hier::get_state().self_node.id;
    if req.parent_id.as_str() == self_id.as_str() {
        hier::add_child(req.child_id.clone());
    }
    state.bus().publish(topics::TOPIC_HIERARCHY_ACCEPTED, &req);
    Json(json!({"ok": true})).into_response()
}
