use arw_macros::{arw_admin, arw_gate};
use axum::{extract::State, response::IntoResponse, Json};

use crate::resources::hierarchy_service::HierarchyService;
use crate::AppState;
use arw_core::hierarchy as hier;
use arw_protocol::{CoreAccept, CoreHello, CoreOffer};

#[arw_admin(
    method = "POST",
    path = "/admin/hierarchy/hello",
    summary = "Hierarchy hello"
)]
#[arw_gate("hierarchy:hello")]
pub async fn hello(State(state): State<AppState>, Json(req): Json<CoreHello>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<HierarchyService>() {
        svc.hello(&state, req).await;
        return super::ok(serde_json::json!({})).into_response();
    }
    hier::configure_self(req.id.clone(), req.scope_tags.clone());
    hier::set_role(match req.role {
        arw_protocol::CoreRole::Root => hier::Role::Root,
        arw_protocol::CoreRole::Regional => hier::Role::Regional,
        arw_protocol::CoreRole::Edge => hier::Role::Edge,
        arw_protocol::CoreRole::Connector => hier::Role::Connector,
        arw_protocol::CoreRole::Observer => hier::Role::Observer,
    });
    state.bus.publish("Hierarchy.Hello", &req);
    super::ok(serde_json::json!({})).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/hierarchy/offer",
    summary = "Hierarchy offer"
)]
#[arw_gate("hierarchy:offer")]
pub async fn offer(State(state): State<AppState>, Json(req): Json<CoreOffer>) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<HierarchyService>() {
        svc.offer(&state, req).await;
        return super::ok(serde_json::json!({})).into_response();
    }
    if let Some(parent) = &req.parent_hint {
        if parent == &hier::get_state().self_node.id {
            hier::add_child(req.from_id.clone());
        }
    }
    state.bus.publish("Hierarchy.Offer", &req);
    super::ok(serde_json::json!({})).into_response()
}

#[arw_admin(
    method = "POST",
    path = "/admin/hierarchy/accept",
    summary = "Hierarchy accept"
)]
#[arw_gate("hierarchy:accept")]
pub async fn accept(
    State(state): State<AppState>,
    Json(req): Json<CoreAccept>,
) -> impl IntoResponse {
    if let Some(svc) = state.resources.get::<HierarchyService>() {
        svc.accept(&state, req).await;
        return super::ok(serde_json::json!({})).into_response();
    }
    if req.parent_id == hier::get_state().self_node.id {
        hier::add_child(req.child_id.clone());
    }
    state.bus.publish("Hierarchy.Accepted", &req);
    super::ok(serde_json::json!({})).into_response()
}
