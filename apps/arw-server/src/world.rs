use arw_events::Envelope;
use arw_topics as topics;
use chrono::SecondsFormat;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use crate::{tasks::TaskHandle, util, AppState};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Entity,
    Claim,
    Task,
    Policy,
    Budget,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Supports,
    Contradicts,
    DependsOn,
    DerivedFrom,
    ObservedAt,
    VerifiedBy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub corr_id: Option<String>,
    #[serde(default)]
    pub sources: Vec<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub props: Map<String, Value>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub last_observed: Option<String>,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub src: String,
    pub dst: String,
    pub kind: EdgeKind,
    #[serde(default)]
    pub props: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct BeliefGraph {
    #[serde(default)]
    pub nodes: HashMap<String, Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub version: u64,
}

#[derive(Clone, Default)]
struct WorldStore {
    proj_graphs: HashMap<String, BeliefGraph>,
    default_graph: BeliefGraph,
}

static STORE: OnceCell<RwLock<WorldStore>> = OnceCell::new();
static VERSION: OnceCell<AtomicU64> = OnceCell::new();

fn store() -> &'static RwLock<WorldStore> {
    STORE.get_or_init(|| RwLock::new(WorldStore::default()))
}

fn ver() -> &'static AtomicU64 {
    VERSION.get_or_init(|| AtomicU64::new(0))
}

fn world_dir() -> PathBuf {
    util::state_dir().join("world")
}

fn world_path() -> PathBuf {
    world_dir().join("world.json")
}

fn world_versions_dir() -> PathBuf {
    world_dir().join("versions")
}

fn proj_from_env(env: &Envelope) -> Option<String> {
    env.payload
        .get("proj")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ensure_graph<'a>(ws: &'a mut WorldStore, proj: Option<&str>) -> &'a mut BeliefGraph {
    if let Some(p) = proj {
        ws.proj_graphs.entry(p.to_string()).or_default()
    } else {
        &mut ws.default_graph
    }
}

fn upsert_node<'a>(g: &'a mut BeliefGraph, id: &'a str, kind: NodeKind) -> &'a mut Node {
    if !g.nodes.contains_key(id) {
        g.nodes.insert(
            id.to_string(),
            Node {
                id: id.to_string(),
                kind,
                props: Map::new(),
                confidence: None,
                last_observed: None,
                provenance: Vec::new(),
            },
        );
    }
    g.nodes.get_mut(id).unwrap()
}

pub(crate) async fn load_persisted() {
    let path = world_path();
    let maybe = tokio::fs::read(&path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if let Some(v) = maybe {
        if let Some(default) = v.get("default") {
            if let Ok(graph) = serde_json::from_value::<BeliefGraph>(default.clone()) {
                let mut ws = store().write().unwrap();
                ws.default_graph = graph;
            }
        }
        if let Some(projects) = v.get("projects").and_then(|p| p.as_object()) {
            let mut ws = store().write().unwrap();
            for (k, val) in projects {
                if let Ok(g) = serde_json::from_value::<BeliefGraph>(val.clone()) {
                    ws.proj_graphs.insert(k.clone(), g);
                }
            }
        }
        if let Some(version) = v.get("version").and_then(|x| x.as_u64()) {
            ver().store(version, Ordering::Relaxed);
        }
    }
}

pub(crate) fn start(state: AppState) -> Vec<TaskHandle> {
    let bus = state.bus();
    vec![TaskHandle::new(
        "world.bus_listener",
        tokio::spawn(async move {
            let mut rx = bus.subscribe();
            while let Ok(env) = rx.recv().await {
                process_event(&bus, &env).await;
            }
        }),
    )]
}

async fn process_event(bus: &arw_events::Bus, env: &Envelope) {
    let proj = proj_from_env(env);
    let now = now_iso();
    let touched = {
        let mut ws = store().write().unwrap();
        let g = ensure_graph(&mut ws, proj.as_deref());
        apply_event(g, env, &now)
    };
    if !touched {
        return;
    }
    let snapshot = {
        let ws = store().read().unwrap();
        ws.clone()
    };
    persist_world(&snapshot).await;
    publish_world_update(bus, proj.as_deref()).await;
}

fn apply_event(g: &mut BeliefGraph, env: &Envelope, now: &str) -> bool {
    let mut touched = false;
    match env.kind.as_str() {
        topics::TOPIC_WORLD_UPDATED => {}
        topics::TOPIC_PROJECTS_CREATED => {
            if let Some(name) = env.payload.get("name").and_then(|v| v.as_str()) {
                let ent_id = format!("proj:{}", name);
                let node = upsert_node(g, &ent_id, NodeKind::Entity);
                node.props
                    .insert("created".into(), Value::String(env.time.clone()));
                node.last_observed = Some(now.to_string());
                touched = true;
            }
        }
        topics::TOPIC_MODELS_CHANGED => {
            if let Some(id) = env.payload.get("id").and_then(|v| v.as_str()) {
                let ent_id = format!("model:{}", id);
                let node = upsert_node(g, &ent_id, NodeKind::Entity);
                if let Some(op) = env.payload.get("op").and_then(|v| v.as_str()) {
                    node.props
                        .insert("status".into(), Value::String(op.to_string()));
                }
                if let Some(path) = env.payload.get("path").and_then(|v| v.as_str()) {
                    node.props
                        .insert("path".into(), Value::String(path.to_string()));
                }
                node.last_observed = Some(now.to_string());
                touched = true;
            }
        }
        topics::TOPIC_MEMORY_APPLIED => {
            if let Some(kind) = env.payload.get("kind").and_then(|v| v.as_str()) {
                let lane_id = format!("memory:{}", kind);
                let node = upsert_node(g, &lane_id, NodeKind::Entity);
                node.props.insert("last".into(), env.payload.clone());
                node.last_observed = Some(now.to_string());
                touched = true;
            }
        }
        topics::TOPIC_POLICY_DECISION => {
            let node = upsert_node(g, "policy:hints", NodeKind::Policy);
            node.props.insert("decision".into(), env.payload.clone());
            node.last_observed = Some(now.to_string());
            touched = true;
        }
        other => {
            if other.starts_with("actions.") {
                if let Some(id) = env.payload.get("id").and_then(|v| v.as_str()) {
                    let action_id = format!("action:{}", id);
                    let node = upsert_node(g, &action_id, NodeKind::Task);
                    node.props.insert("event".into(), env.payload.clone());
                    node.last_observed = Some(now.to_string());
                    touched = true;
                }
            }
        }
    }
    if touched {
        g.last_updated = Some(now.to_string());
        g.version = g.version.saturating_add(1);
        ver().fetch_add(1, Ordering::Relaxed);
    }
    touched
}

async fn persist_world(ws: &WorldStore) {
    let payload = json!({
        "version": ver().load(Ordering::Relaxed),
        "default": &ws.default_graph,
        "projects": &ws.proj_graphs,
    });
    let main_path = world_path();
    if let Some(parent) = main_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&main_path, serde_json::to_vec_pretty(&payload).unwrap()).await;
    let versions = world_versions_dir();
    let _ = tokio::fs::create_dir_all(&versions).await;
    let version_path = versions.join(format!("world.v{}.json", ver().load(Ordering::Relaxed)));
    let _ = tokio::fs::write(&version_path, serde_json::to_vec_pretty(&payload).unwrap()).await;
    prune_versions(&versions).await;
}

async fn prune_versions(dir: &PathBuf) {
    if let Ok(mut rd) = tokio::fs::read_dir(dir).await {
        let mut entries: Vec<(u64, PathBuf)> = Vec::new();
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                if let Some(num) = name
                    .strip_prefix("world.v")
                    .and_then(|s| s.strip_suffix(".json"))
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    entries.push((num, ent.path()));
                }
            }
        }
        entries.sort_by_key(|(n, _)| *n);
        if entries.len() > 5 {
            for (_, path) in entries.iter().take(entries.len() - 5) {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }
}

async fn publish_world_update(bus: &arw_events::Bus, proj: Option<&str>) {
    let ws = store().read().unwrap();
    let graph = if let Some(p) = proj {
        ws.proj_graphs.get(p)
    } else {
        Some(&ws.default_graph)
    };
    if let Some(g) = graph {
        let claims = g
            .nodes
            .values()
            .filter(|n| matches!(n.kind, NodeKind::Claim))
            .count();
        let contradictions = g
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Contradicts))
            .count();
        let payload = json!({
            "proj": proj,
            "version": ver().load(Ordering::Relaxed),
            "graph_version": g.version,
            "nodes": g.nodes.len(),
            "edges": g.edges.len(),
            "claims": claims,
            "contradictions": contradictions,
        });
        bus.publish(topics::TOPIC_WORLD_UPDATED, &payload);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectMap {
    pub version: u64,
    pub project: Option<String>,
    pub entities: Vec<Value>,
    pub claims: Vec<Value>,
    pub contradictions: Vec<Value>,
    pub stale: Vec<Value>,
    pub coverage: Value,
}

pub(crate) fn snapshot_project_map(project: Option<&str>) -> ProjectMap {
    let ws = store().read().unwrap();
    let global_version = ver().load(Ordering::Relaxed);
    let (graph, graph_version) = if let Some(p) = project {
        let g = ws.proj_graphs.get(p);
        let version = g
            .map(|graph| graph.version)
            .unwrap_or(ws.default_graph.version);
        (g, version)
    } else {
        (Some(&ws.default_graph), ws.default_graph.version)
    };
    if let Some(g) = graph {
        let mut entities = Vec::new();
        let mut claims = Vec::new();
        let mut kinds = HashSet::new();
        let mut ents_seen = 0usize;
        let mut cl_seen = 0usize;
        for n in g.nodes.values() {
            kinds.insert(format!("{:?}", n.kind));
            match n.kind {
                NodeKind::Entity => {
                    ents_seen += 1;
                    entities.push(json!({
                        "id": n.id,
                        "props": n.props.clone(),
                        "confidence": n.confidence,
                        "last": n.last_observed,
                    }));
                    if entities.len() >= 50 {
                        break;
                    }
                }
                NodeKind::Claim => {
                    cl_seen += 1;
                    claims.push(json!({
                        "id": n.id,
                        "props": n.props.clone(),
                        "confidence": n.confidence,
                        "last": n.last_observed,
                    }));
                    if claims.len() >= 50 {
                        break;
                    }
                }
                _ => {}
            }
        }
        let mut contradictions = Vec::new();
        for e in g
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Contradicts))
        {
            contradictions.push(json!({"src": e.src, "dst": e.dst, "props": e.props.clone()}));
            if contradictions.len() >= 20 {
                break;
            }
        }
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
        let mut stale = Vec::new();
        for n in g.nodes.values() {
            if let Some(ts) = n.last_observed.as_deref() {
                if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                    if parsed.with_timezone(&chrono::Utc) < cutoff {
                        stale.push(json!({"id": n.id, "last": ts}));
                        if stale.len() >= 20 {
                            break;
                        }
                    }
                }
            }
        }
        ProjectMap {
            version: global_version.max(graph_version),
            project: project.map(|s| s.to_string()),
            entities,
            claims,
            contradictions,
            stale,
            coverage: json!({
                "entities": ents_seen,
                "claims": cl_seen,
                "kinds": kinds,
            }),
        }
    } else {
        ProjectMap {
            version: global_version,
            project: project.map(|s| s.to_string()),
            entities: Vec::new(),
            claims: Vec::new(),
            contradictions: Vec::new(),
            stale: Vec::new(),
            coverage: json!({}),
        }
    }
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn flatten_value(v: &Value, cap: usize) -> String {
    match v {
        Value::String(s) => s.chars().take(cap).collect(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Value::Array(arr) => {
            let mut out = String::new();
            for (idx, item) in arr.iter().enumerate() {
                if out.len() >= cap {
                    break;
                }
                if idx > 0 {
                    out.push(' ');
                }
                out.push_str(&flatten_value(item, cap - out.len()));
            }
            out
        }
        Value::Object(map) => {
            let mut out = String::new();
            for (key, val) in map.iter() {
                if out.len() >= cap {
                    break;
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(key);
                out.push(' ');
                out.push_str(&flatten_value(val, cap - out.len()));
            }
            out
        }
        Value::Null => String::new(),
    }
}

fn score_claim(node: &Node, query: &str) -> (f64, Value) {
    let query = query.trim();
    if query.is_empty() {
        return (
            node.confidence.unwrap_or(0.0),
            json!({"conf": node.confidence}),
        );
    }
    let tokens = tokens(query);
    let id_text = node.id.to_ascii_lowercase();
    let props_text = flatten_value(&Value::Object(node.props.clone()), 2048).to_ascii_lowercase();
    let mut hits_id = 0u64;
    let mut hits_props = 0u64;
    for token in tokens.iter() {
        if id_text.contains(token) {
            hits_id += 1;
        }
        if props_text.contains(token) {
            hits_props += 1;
        }
    }
    let conf = node.confidence.unwrap_or(0.5);
    let recency = node
        .last_observed
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|t| {
            let age = (chrono::Utc::now() - t.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0) as f64;
            let half = 6.0 * 3600.0;
            (0.5f64).powf(age / half)
        })
        .unwrap_or(1.0);
    let score = (2.0 * hits_id as f64 + hits_props as f64) * 0.5 + conf + 0.5 * recency;
    (
        score,
        json!({"hits_id": hits_id, "hits_props": hits_props, "conf": conf, "recency": recency}),
    )
}

pub(crate) fn select_top_claims(proj: Option<&str>, query: &str, k: usize) -> Vec<Value> {
    let ws = store().read().unwrap();
    let graph = if let Some(p) = proj {
        ws.proj_graphs.get(p)
    } else {
        Some(&ws.default_graph)
    };
    let mut scored = Vec::new();
    if let Some(g) = graph {
        for node in g.nodes.values() {
            if !matches!(node.kind, NodeKind::Claim) {
                continue;
            }
            let (score, trace) = score_claim(node, query);
            let mut item = json!({
                "id": node.id,
                "kind": "claim",
                "confidence": node.confidence,
                "props": node.props.clone(),
                "last": node.last_observed,
                "trace": trace,
            });
            if !node.provenance.is_empty() {
                item["provenance"] = json!(node.provenance.iter().take(3).collect::<Vec<_>>());
            }
            scored.push((score, item));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top = k.clamp(1, 50);
    scored.into_iter().take(top).map(|(_, v)| v).collect()
}

pub(crate) fn select_top_claims_diverse(
    proj: Option<&str>,
    query: &str,
    k: usize,
    lambda: f64,
) -> Vec<Value> {
    let lambda = lambda.clamp(0.0, 1.0);
    let candidates = select_top_claims(proj, query, k.max(20));
    let mut selected = Vec::new();
    let mut selected_tokens: Vec<HashSet<String>> = Vec::new();
    for item in candidates {
        if selected.len() >= k {
            break;
        }
        let text = flatten_value(&item, 2048);
        let text_lower = text.to_ascii_lowercase();
        let toks_vec = tokens(&text_lower);
        let toks_set: HashSet<String> = toks_vec.into_iter().collect();
        let relevance = 1.0f64;
        let mut penalty: f64 = 0.0;
        for tokset in selected_tokens.iter() {
            let inter = toks_set.intersection(tokset).count() as f64;
            let union = (tokset.len() + toks_set.len()) as f64 - inter;
            let union = union.max(1.0);
            penalty = penalty.max(inter / union);
        }
        let mmr = lambda * relevance - (1.0 - lambda) * penalty;
        if mmr >= 0.0 {
            selected_tokens.push(toks_set);
            selected.push(item);
        }
    }
    selected
}
