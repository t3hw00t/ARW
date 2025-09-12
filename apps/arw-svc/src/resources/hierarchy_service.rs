use arw_core::hierarchy as hier;
use arw_protocol::{CoreAccept, CoreHello, CoreOffer, CoreRole};
// use serde_json::json; // not used directly; using serde_json::json! fully qualified

use crate::app_state::AppState;

#[derive(Default)]
pub struct HierarchyService;

impl HierarchyService {
    pub fn new() -> Self {
        Self
    }

    pub async fn hello(&self, state: &AppState, req: CoreHello) {
        hier::configure_self(req.id.clone(), req.scope_tags.clone());
        hier::set_role(match req.role {
            CoreRole::Root => hier::Role::Root,
            CoreRole::Regional => hier::Role::Regional,
            CoreRole::Edge => hier::Role::Edge,
            CoreRole::Connector => hier::Role::Connector,
            CoreRole::Observer => hier::Role::Observer,
        });
        state.bus.publish("Hierarchy.Hello", &req);
    }

    pub async fn offer(&self, state: &AppState, req: CoreOffer) {
        if let Some(parent) = &req.parent_hint {
            if parent == &hier::get_state().self_node.id {
                hier::add_child(req.from_id.clone());
            }
        }
        state.bus.publish("Hierarchy.Offer", &req);
    }

    pub async fn accept(&self, state: &AppState, req: CoreAccept) {
        if req.parent_id == hier::get_state().self_node.id {
            hier::add_child(req.child_id.clone());
        }
        state.bus.publish("Hierarchy.Accepted", &req);
    }

    pub async fn state_event(&self, state: &AppState) -> arw_core::hierarchy::HierarchyState {
        let st = hier::get_state();
        let mut p = serde_json::json!({"epoch": st.epoch});
        crate::ext::corr::ensure_corr(&mut p);
        state.bus.publish("Hierarchy.State", &p);
        st
    }

    pub async fn role_set(&self, state: &AppState, role: &str) {
        let role_enum = match role {
            "root" => hier::Role::Root,
            "regional" => hier::Role::Regional,
            "edge" => hier::Role::Edge,
            "connector" => hier::Role::Connector,
            _ => hier::Role::Observer,
        };
        hier::set_role(role_enum);
        let gate_role = match role_enum {
            hier::Role::Root => arw_core::gating::Role::Root,
            hier::Role::Regional => arw_core::gating::Role::Regional,
            hier::Role::Edge => arw_core::gating::Role::Edge,
            hier::Role::Connector => arw_core::gating::Role::Connector,
            hier::Role::Observer => arw_core::gating::Role::Observer,
        };
        arw_core::gating::apply_role_defaults(gate_role);
        let mut p = serde_json::json!({"role": role});
        crate::ext::corr::ensure_corr(&mut p);
        state.bus.publish("Hierarchy.RoleChanged", &p);
    }
}
