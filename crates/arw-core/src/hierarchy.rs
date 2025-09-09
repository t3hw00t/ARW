use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::sync::RwLock;

/// Logical role of a core within a hierarchy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Root,
    Regional,
    Edge,
    Connector,
    Observer,
}

impl Default for Role {
    fn default() -> Self {
        Role::Edge
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeInfo {
    pub id: String,
    pub role: Role,
    pub tags: BTreeSet<String>,
    pub parent: Option<String>,
    pub children: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HierarchyState {
    pub epoch: u64,
    pub self_node: NodeInfo,
    pub peers: HashMap<String, NodeInfo>,
}

static HIER_STATE: OnceCell<RwLock<HierarchyState>> = OnceCell::new();

fn cell() -> &'static RwLock<HierarchyState> {
    HIER_STATE.get_or_init(|| RwLock::new(HierarchyState::default()))
}

pub fn get_state() -> HierarchyState {
    cell().read().unwrap().clone()
}

pub fn set_role(role: Role) {
    let mut st = cell().write().unwrap();
    if st.self_node.role != role {
        st.self_node.role = role;
        st.epoch = st.epoch.saturating_add(1);
    }
}

pub fn configure_self(id: String, tags: impl IntoIterator<Item = String>) {
    let mut st = cell().write().unwrap();
    st.self_node.id = id;
    st.self_node.tags = tags.into_iter().collect();
    st.epoch = st.epoch.saturating_add(1);
}

pub fn link_parent(parent_id: Option<String>) {
    let mut st = cell().write().unwrap();
    st.self_node.parent = parent_id;
    st.epoch = st.epoch.saturating_add(1);
}

pub fn add_child(child_id: String) {
    let mut st = cell().write().unwrap();
    st.self_node.children.insert(child_id);
    st.epoch = st.epoch.saturating_add(1);
}
