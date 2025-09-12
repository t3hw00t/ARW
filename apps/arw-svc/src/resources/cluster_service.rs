use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::app_state::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct NodeInfo {
    pub id: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<HashMap<String, Value>>, // models/tools/hardware summaries
}

#[derive(Default)]
pub struct ClusterService {
    nodes: RwLock<Vec<NodeInfo>>, // Home + invited Workers (runtime view)
}

impl ClusterService {
    pub fn new() -> Self {
        Self::default()
    }

    // Minimal state event (for a future `/state/runtime_matrix` extension)
    #[allow(dead_code)]
    pub async fn state_event(&self, state: &AppState) -> Value {
        let nodes = { self.nodes.read().await.clone() };
        let mut payload = json!({
            "nodes": nodes,
        });
        crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Cluster.Node.Changed", &payload);
        payload
    }

    // Update/insert a node entry.
    #[allow(dead_code)]
    pub async fn upsert_node(&self, info: NodeInfo) {
        let mut nodes = self.nodes.write().await;
        if let Some(n) = nodes.iter_mut().find(|n| n.id == info.id) {
            *n = info;
            return;
        }
        nodes.push(info);
    }

    // Publish a local advertise (MVP placeholder; safe to call at startup later)
    #[allow(dead_code)]
    pub async fn advertise_local(&self, state: &AppState) {
        let id = std::env::var("ARW_NODE_ID")
            .unwrap_or_else(|_| sysinfo::System::host_name().unwrap_or_else(|| "local".into()));
        let role = format!("{:?}", arw_core::hierarchy::get_state().self_node.role);
        let mut caps = HashMap::new();
        // Minimal capability hints; extend with models/tools later
        caps.insert("os".into(), Value::String(std::env::consts::OS.into()));
        caps.insert("arch".into(), Value::String(std::env::consts::ARCH.into()));
        // Summarize locally available model hashes for quick ads
        let models_view = {
            let list = crate::ext::models().read().await.clone();
            use std::collections::HashSet;
            let mut hs: HashSet<String> = HashSet::new();
            for m in list.into_iter() {
                if let Some(s) = m.get("sha256").and_then(|v| v.as_str()) {
                    if s.len() == 64 {
                        hs.insert(s.to_string());
                    }
                }
            }
            let mut hashes: Vec<String> = hs.into_iter().collect();
            hashes.sort();
            // limit to first few to keep ads small
            let n = hashes.len();
            let preview: Vec<String> = hashes.into_iter().take(8).collect();
            json!({"count": n, "preview": preview})
        };

        let node = NodeInfo {
            id: id.clone(),
            role,
            name: None,
            health: Some("ok".into()),
            capabilities: Some(caps),
        };
        self.upsert_node(node.clone()).await;
        let mut payload = json!({
            "id": id,
            "role": node.role,
            "capabilities": node.capabilities,
            "models": models_view,
        });
        crate::ext::corr::ensure_corr(&mut payload);
        state.bus.publish("Cluster.Node.Advertise", &payload);
    }
}
