use arw_events::Envelope;
use arw_macros::arw_admin;
use axum::{extract::Query, response::IntoResponse};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

// Lightweight, per-project world model: typed belief graph + provenance

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
    pub sources: Vec<Value>, // artifacts, urls, paths, etc.
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

#[derive(Default)]
struct WorldStore {
    proj_graphs: HashMap<String, BeliefGraph>, // project -> graph
    default_graph: BeliefGraph,                // fallback when no project is implied
}

static STORE: OnceCell<RwLock<WorldStore>> = OnceCell::new();
static VERSION: OnceCell<AtomicU64> = OnceCell::new();
fn store() -> &'static RwLock<WorldStore> {
    STORE.get_or_init(|| RwLock::new(WorldStore::default()))
}
fn ver() -> &'static AtomicU64 {
    VERSION.get_or_init(|| AtomicU64::new(0))
}

// Resolve project from an event payload when present
fn proj_from_env(env: &Envelope) -> Option<String> {
    env.payload
        .get("proj")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn ensure_graph<'a>(ws: &'a mut WorldStore, proj: Option<&str>) -> &'a mut BeliefGraph {
    if let Some(p) = proj {
        ws.proj_graphs.entry(p.to_string()).or_default()
    } else {
        &mut ws.default_graph
    }
}

// Public helper for read-only lookup of a belief node by id with optional project scoping.
pub fn get_belief_node(proj: Option<&str>, id: &str) -> Option<Node> {
    let ws = store().read().unwrap();
    let g_opt = if let Some(p) = proj { ws.proj_graphs.get(p) } else { Some(&ws.default_graph) };
    g_opt.and_then(|g| g.nodes.get(id)).cloned()
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

fn add_edge(g: &mut BeliefGraph, src: &str, dst: &str, kind: EdgeKind, props: Map<String, Value>) {
    g.edges.push(Edge {
        src: src.to_string(),
        dst: dst.to_string(),
        kind,
        props,
    });
}

async fn persist_world(ws: &WorldStore) {
    let dir = crate::ext::paths::world_dir();
    let p = crate::ext::paths::world_path();
    let versions = crate::ext::paths::world_versions_dir();
    let _ = tokio::fs::create_dir_all(&dir).await;
    let _ = tokio::fs::create_dir_all(&versions).await;
    let version = ver().load(Ordering::Relaxed);
    let snap = json!({
        "version": version,
        "default": &ws.default_graph,
        "projects": &ws.proj_graphs,
    });
    let _ = crate::ext::io::save_json_file_async(&p, &snap).await;
    // also write a versioned copy and prune older (keep 5)
    let vp = versions.join(format!("world.v{}.json", version));
    let _ = crate::ext::io::save_json_file_async(&vp, &snap).await;
    // prune versions (best-effort)
    if let Ok(mut rd) = tokio::fs::read_dir(&versions).await {
        let mut files: Vec<(u64, std::path::PathBuf)> = Vec::new();
        while let Ok(Some(e)) = rd.next_entry().await {
            if let Some(name) = e.file_name().to_str() {
                if let Some(num) = name
                    .strip_prefix("world.v")
                    .and_then(|s| s.strip_suffix(".json"))
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    files.push((num, e.path()));
                }
            }
        }
        files.sort_by_key(|(n, _)| *n);
        if files.len() > 5 {
            let drop = files.len() - 5;
            for (_n, pth) in files.iter().take(drop) {
                let _ = tokio::fs::remove_file(pth).await;
            }
        }
    }
}

pub async fn load_persisted() {
    let p = crate::ext::paths::world_path();
    if let Some(v) = crate::ext::io::load_json_file_async(&p).await {
        let mut ws = store().write().unwrap();
        let version = v.get("version").and_then(|x| x.as_u64()).unwrap_or(0);
        if let Some(def) = v.get("default") {
            if let Ok(g) = serde_json::from_value::<BeliefGraph>(def.clone()) {
                ws.default_graph = g;
            }
        }
        if let Some(proj) = v.get("projects").and_then(|x| x.as_object()) {
            let mut map = HashMap::new();
            for (k, vv) in proj.iter() {
                if let Ok(g) = serde_json::from_value::<BeliefGraph>(vv.clone()) {
                    map.insert(k.clone(), g);
                }
            }
            ws.proj_graphs = map;
        }
        ver().store(version, Ordering::Relaxed);
    }
}

// Map a subset of existing events into the belief graph
pub async fn on_event(bus: &arw_events::Bus, env: &Envelope) {
    let proj = proj_from_env(env);
    let mut touched = false;
    {
        let mut ws = store().write().unwrap();
        let g = ensure_graph(&mut ws, proj.as_deref());
        let now = now_iso();

        match env.kind.as_str() {
        // Feedback suggestions -> claims with confidence + provenance
        "Feedback.Suggested" => {
            let list: Vec<Value> = env
                .payload
                .get("suggestions")
                .and_then(|a| a.as_array())
                .cloned()
                .unwrap_or_else(|| vec![env.payload.clone()]);
            let cid = env.payload.get("corr_id").and_then(|v| v.as_str()).map(|s| s.to_string());
            for it in list {
                let id = it
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("claim")
                    .to_string();
                let nid = format!("claim:{}", id);
                let n = upsert_node(g, &nid, NodeKind::Claim);
                n.props = it.as_object().cloned().unwrap_or_default();
                n.confidence = it.get("confidence").and_then(|v| v.as_f64());
                n.last_observed = Some(now.clone());
                n.provenance.push(Provenance {
                    kind: env.kind.clone(),
                    observed_at: env.time.clone(),
                    corr_id: cid.clone(),
                    sources: vec![],
                });
            }
            touched = true;
        }
        _ if env.kind.starts_with("Beliefs.") => {
            let cid = env
                .payload
                .get("corr_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let nid = format!("claim:{}", env.kind.replace('.', ":"));
            let n = upsert_node(g, &nid, NodeKind::Claim);
            n.props = env.payload.as_object().cloned().unwrap_or_default();
            n.last_observed = Some(now.clone());
            n.provenance.push(Provenance {
                kind: env.kind.clone(),
                observed_at: env.time.clone(),
                corr_id: cid,
                sources: vec![],
            });
            touched = true;
        }
        // Project file writes -> entities and observed_at edges
        "Projects.FileWritten" => {
            let proj = env
                .payload
                .get("proj")
                .and_then(|v| v.as_str())
                .unwrap_or("proj");
            let path = env
                .payload
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !path.is_empty() {
                let ent_id = format!("file:{}/{}", proj, path);
                let n = upsert_node(g, &ent_id, NodeKind::Entity);
                n.props.insert("proj".into(), Value::String(proj.to_string()));
                n.props.insert("path".into(), Value::String(path.to_string()));
                n.last_observed = Some(now.clone());
                n.provenance.push(Provenance {
                    kind: env.kind.clone(),
                    observed_at: env.time.clone(),
                    corr_id: env
                        .payload
                        .get("corr_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    sources: vec![json!({"type":"file","proj": proj, "path": path})],
                });
                // observed_at edge from entity to a synthetic observation node
                let obs_id = format!("obs:{}", env.time);
                let _o = upsert_node(g, &obs_id, NodeKind::Entity);
                let mut props = Map::new();
                props.insert("time".into(), Value::String(env.time.clone()));
                add_edge(g, &ent_id, &obs_id, EdgeKind::ObservedAt, props);
            }
            touched = true;
        }
        // Model download progress -> entities and budget nodes
        k if k == "Models.DownloadProgress" => {
            if let Some(id) = env.payload.get("id").and_then(|v| v.as_str()) {
                let status = env
                    .payload
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let ent_id = format!("model:{}", id);
                let n = upsert_node(g, &ent_id, NodeKind::Entity);
                n.props.insert("status".into(), Value::String(status.to_string()));
                if let Some(b) = env.payload.get("budget") {
                    n.props.insert("budget".into(), b.clone());
                }
                n.last_observed = Some(now.clone());
                touched = true;
            }
        }
        // Model lifecycle changes
        "Models.Changed" => {
            if let Some(id) = env.payload.get("id").and_then(|v| v.as_str()) {
                let op = env
                    .payload
                    .get("op")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let ent_id = format!("model:{}", id);
                let n = upsert_node(g, &ent_id, NodeKind::Entity);
                let status = match op {
                    "downloaded" => "available",
                    "delete" => "deleted",
                    other => other,
                };
                n.props.insert("status".into(), Value::String(status.to_string()));
                if let Some(p) = env.payload.get("path").and_then(|v| v.as_str()) {
                    n.props.insert("path".into(), Value::String(p.to_string()));
                }
                n.last_observed = Some(now.clone());
                touched = true;
            }
        }
        // Project lifecycle
        "Projects.Created" => {
            if let Some(name) = env.payload.get("name").and_then(|v| v.as_str()) {
                let ent_id = format!("proj:{}", name);
                let n = upsert_node(g, &ent_id, NodeKind::Entity);
                n.props.insert("created".into(), Value::String(env.time.clone()));
                n.last_observed = Some(now.clone());
                touched = true;
            }
        }
        // Runtime health -> entity runtime with metrics
        "Service.Health" | "Probe.Metrics" | "Runtime.Health" => {
            let ent_id = "runtime";
            let n = upsert_node(g, ent_id, NodeKind::Entity);
            n.props.insert("last".into(), env.payload.clone());
            n.last_observed = Some(now.clone());
            touched = true;
        }
        // Policy hints applied -> policy node
        "Actions.HintApplied" => {
            let pol_id = "policy:hints";
            let n = upsert_node(g, pol_id, NodeKind::Policy);
            n.props.insert("last".into(), env.payload.clone());
            n.last_observed = Some(now.clone());
            touched = true;
        }
        _ => {}
        }

        if touched {
            g.last_updated = Some(now);
            g.version = g.version.saturating_add(1);
            let _v = ver().fetch_add(1, Ordering::Relaxed) + 1;
        }
    }

    if touched {
        // Persist asynchronously without holding any lock across await
        let (def, projs) = {
            let ws_r = store().read().unwrap();
            (ws_r.default_graph.clone(), ws_r.proj_graphs.clone())
        };
        let snap_ws = WorldStore {
            proj_graphs: projs,
            default_graph: def,
        };
        persist_world(&snap_ws).await;
        // Publish a compact world updated event for subscribers/UI
        let ws_r = store().read().unwrap();
        let g_now = if let Some(p) = proj.as_deref() {
            ws_r.proj_graphs.get(p)
        } else {
            Some(&ws_r.default_graph)
        };
        if let Some(gx) = g_now {
            let claim_count = gx
                .nodes
                .values()
                .filter(|n| matches!(n.kind, NodeKind::Claim))
                .count();
            let contradict_count = gx
                .edges
                .iter()
                .filter(|e| matches!(e.kind, EdgeKind::Contradicts))
                .count();
            let stale_count = {
                let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
                gx.nodes
                    .values()
                    .filter(|n| n
                        .last_observed
                        .as_deref()
                        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                        .map(|t| t.with_timezone(&chrono::Utc) < cutoff)
                        .unwrap_or(false))
                    .count()
            };
            let mut payload = json!({
                "proj": proj,
                "version": ver().load(Ordering::Relaxed),
                "graph_version": gx.version,
                "nodes": gx.nodes.len(),
                "edges": gx.edges.len(),
                "claims": claim_count,
                "contradictions": contradict_count,
                "stale": stale_count,
            });
            crate::ext::corr::ensure_corr(&mut payload);
            bus.publish("World.Updated", &payload);
        }
    }
}

// Compact Project Map view (entities, key claims, contradictions, stale)
#[derive(Serialize)]
struct ProjectMap {
    version: u64,
    project: Option<String>,
    entities: Vec<Value>,
    claims: Vec<Value>,
    contradictions: Vec<Value>,
    stale: Vec<Value>,
    coverage: Value,
}

fn snapshot_project_map(ws: &WorldStore, project: Option<&str>) -> ProjectMap {
    let g = if let Some(p) = project {
        ws.proj_graphs.get(p)
    } else {
        Some(&ws.default_graph)
    };
    let mut entities = Vec::new();
    let mut claims = Vec::new();
    let mut contradictions = Vec::new();
    let mut stale = Vec::new();
    let mut ents_seen = 0u64;
    let mut cl_seen = 0u64;
    let mut kinds: HashMap<String, u64> = HashMap::new();
    let mut last_ver = 0u64;
    if let Some(g) = g {
        last_ver = g.version;
        for n in g.nodes.values() {
            let kind_s = match n.kind { NodeKind::Entity => "entity", NodeKind::Claim => "claim", NodeKind::Task => "task", NodeKind::Policy => "policy", NodeKind::Budget => "budget" };
            *kinds.entry(kind_s.to_string()).or_insert(0) += 1;
            match n.kind {
                NodeKind::Entity => {
                    ents_seen += 1;
                    if entities.len() < 50 {
                        entities.push(json!({"id": n.id, "props": n.props, "last": n.last_observed}));
                    }
                }
                NodeKind::Claim => {
                    cl_seen += 1;
                    let conf = n.confidence.unwrap_or(0.0);
                    claims.push(json!({"id": n.id, "confidence": conf, "props": n.props}));
                }
                _ => {}
            }
        }
        // Sort claims by confidence desc and cap
        claims.sort_by(|a, b| {
            let ca = a.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let cb = b.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);
            cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
        });
        if claims.len() > 20 { claims.truncate(20); }
        // Derive contradictions from edges
        for e in g.edges.iter().filter(|e| matches!(e.kind, EdgeKind::Contradicts)) {
            contradictions.push(json!({"src": e.src, "dst": e.dst, "props": e.props}));
            if contradictions.len() >= 20 { break; }
        }
        // Stale: entities not observed in > 1 hour
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(1);
        for n in g.nodes.values() {
            if let Some(ts) = n.last_observed.as_deref() {
                if let Ok(t) = chrono::DateTime::parse_from_rfc3339(ts) {
                    if t.with_timezone(&chrono::Utc) < cutoff {
                        stale.push(json!({"id": n.id, "last": ts}));
                        if stale.len() >= 20 { break; }
                    }
                }
            }
        }
    }
    ProjectMap {
        version: ver().load(Ordering::Relaxed).max(last_ver),
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
}

#[derive(Deserialize)]
pub struct WorldQs {
    pub proj: Option<String>,
}
#[arw_admin(
    method = "GET",
    path = "/admin/state/world",
    summary = "Scoped world model: Project Map (belief graph view)"
)]
pub async fn world_get(Query(q): Query<WorldQs>) -> impl IntoResponse {
    let ws = store().read().unwrap();
    let map = snapshot_project_map(&ws, q.proj.as_deref());
    super::ok(serde_json::to_value(map).unwrap_or_else(|_| json!({})))
}

// ---- Top-K beliefs (claims) selection with simple scoring and trace ----

#[derive(Deserialize)]
pub struct WorldSelectQs {
    pub proj: Option<String>,
    pub q: Option<String>,
    pub k: Option<usize>,
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
            if *b { "true".into() } else { "false".into() }
        }
        Value::Array(a) => {
            let mut out = String::new();
            for (i, it) in a.iter().enumerate() {
                if out.len() >= cap { break; }
                if i > 0 { out.push(' '); }
                out.push_str(&flatten_value(it, cap - out.len()));
            }
            out
        }
        Value::Object(m) => {
            let mut out = String::new();
            for (k, v) in m.iter() {
                if out.len() >= cap { break; }
                if !out.is_empty() { out.push(' '); }
                out.push_str(k);
                out.push(' ');
                out.push_str(&flatten_value(v, cap - out.len()));
            }
            out
        }
        Value::Null => String::new(),
    }
}

fn score_claim(n: &Node, query: &str) -> (f64, serde_json::Value) {
    let q = query.trim();
    if q.is_empty() { return (n.confidence.unwrap_or(0.0), json!({"conf": n.confidence})); }
    let qtok = tokens(q);
    let id_text = n.id.to_ascii_lowercase();
    let props_text = flatten_value(&Value::Object(n.props.clone()), 2048).to_ascii_lowercase();
    let mut hits_id = 0u64;
    let mut hits_props = 0u64;
    for t in qtok.iter() {
        if id_text.contains(t) { hits_id += 1; }
        if props_text.contains(t) { hits_props += 1; }
    }
    let conf = n.confidence.unwrap_or(0.5);
    // Recency: half-life 6h (configurable later)
    let rec = n
        .last_observed
        .as_deref()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|t| {
            let age_s = (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds().max(0) as f64;
            let half = 6.0 * 3600.0; // 6 hours
            (0.5f64).powf(age_s / half)
        })
        .unwrap_or(1.0);
    let score = (2.0 * hits_id as f64 + 1.0 * hits_props as f64) * 0.5 + conf * 1.0 + rec * 0.5;
    (score, json!({"hits_id": hits_id, "hits_props": hits_props, "conf": conf, "recency": rec}))
}

#[arw_admin(
    method = "GET",
    path = "/admin/state/world/select",
    summary = "Select top‑K beliefs (claims) for a query"
)]
pub async fn world_select_get(Query(q): Query<WorldSelectQs>) -> impl IntoResponse {
    let ws = store().read().unwrap();
    let g = if let Some(p) = q.proj.as_deref() {
        ws.proj_graphs.get(p)
    } else {
        Some(&ws.default_graph)
    };
    let mut scored: Vec<(f64, Value)> = Vec::new();
    if let Some(g) = g {
        for n in g.nodes.values() {
            if !matches!(n.kind, NodeKind::Claim) { continue; }
            let (s, tr) = score_claim(n, q.q.as_deref().unwrap_or(""));
            let mut item = json!({
                "id": n.id,
                "kind": "claim",
                "confidence": n.confidence,
                "props": n.props,
                "last": n.last_observed,
                "trace": tr,
            });
            // provenance size bound
            if !n.provenance.is_empty() {
                item["provenance"] = json!(n.provenance.iter().take(3).collect::<Vec<_>>());
            }
            scored.push((s, item));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let k = q.k.unwrap_or(8).clamp(1, 50);
    let items: Vec<Value> = scored.into_iter().take(k).map(|(_, v)| v).collect();
    super::ok(json!({"items": items}))
}

// Public helper for context assembly
pub async fn select_top_claims(proj: Option<&str>, q: &str, k: usize) -> Vec<Value> {
    let ws = store().read().unwrap();
    let g = if let Some(p) = proj { ws.proj_graphs.get(p) } else { Some(&ws.default_graph) };
    let mut scored: Vec<(f64, Value)> = Vec::new();
    if let Some(g) = g {
        for n in g.nodes.values() {
            if !matches!(n.kind, NodeKind::Claim) { continue; }
            let (s, tr) = score_claim(n, q);
            let mut item = json!({
                "id": n.id,
                "kind": "claim",
                "confidence": n.confidence,
                "props": n.props,
                "last": n.last_observed,
                "trace": tr,
            });
            if !n.provenance.is_empty() {
                item["provenance"] = json!(n.provenance.iter().take(3).collect::<Vec<_>>());
            }
            scored.push((s, item));
        }
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let kk = k.clamp(1, 50);
    scored.into_iter().take(kk).map(|(_, v)| v).collect()
}

// Diversity-aware selection (MMR-style): greedily selects items maximizing
// lambda * relevance - (1-lambda) * similarity_to_selected.
// Similarity is a cheap Jaccard over token sets from id + string props.
pub async fn select_top_claims_diverse(
    proj: Option<&str>,
    q: &str,
    k: usize,
    lambda: f64,
) -> Vec<Value> {
    // Clamp params
    let k = k.clamp(1, 50);
    let lambda = if !(0.0..=1.0).contains(&lambda) { 0.5 } else { lambda };
    // Build scored list using existing scorer
    let ws = store().read().unwrap();
    let g = if let Some(p) = proj { ws.proj_graphs.get(p) } else { Some(&ws.default_graph) };
    let mut scored: Vec<(f64, Value)> = Vec::new();
    if let Some(g) = g {
        for n in g.nodes.values() {
            if !matches!(n.kind, NodeKind::Claim) { continue; }
            let (s, tr) = score_claim(n, q);
            let mut item = json!({
                "id": n.id,
                "kind": "claim",
                "confidence": n.confidence,
                "props": n.props,
                "last": n.last_observed,
                "trace": tr,
            });
            if !n.provenance.is_empty() {
                item["provenance"] = json!(n.provenance.iter().take(3).collect::<Vec<_>>());
            }
            scored.push((s, item));
        }
    }
    // Tokenize helper
    fn tokenize(v: &Value) -> std::collections::HashSet<String> {
        let mut s = String::new();
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) { s.push_str(id); s.push(' '); }
        if let Some(props) = v.get("props").and_then(|x| x.as_object()) {
            for (_k, vv) in props.iter() {
                if let Some(t) = vv.as_str() { s.push_str(t); s.push(' '); }
            }
        }
        s.split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_ascii_lowercase())
            .collect()
    }
    fn jaccard(a: &std::collections::HashSet<String>, b: &std::collections::HashSet<String>) -> f64 {
        if a.is_empty() || b.is_empty() { return 0.0; }
        let inter = a.intersection(b).count() as f64;
        let uni = a.union(b).count() as f64;
        if uni == 0.0 { 0.0 } else { inter / uni }
    }
    // Precompute tokens for scored list
    let mut items: Vec<(f64, Value, std::collections::HashSet<String>)> = scored
        .into_iter()
        .map(|(s, v)| {
            let toks = tokenize(&v);
            (s, v, toks)
        })
        .collect();
    // Greedy MMR selection
    let mut selected: Vec<(f64, Value, std::collections::HashSet<String>)> = Vec::new();
    while selected.len() < k && !items.is_empty() {
        let mut best_idx: usize = 0;
        let mut best_score: f64 = f64::NEG_INFINITY;
        for (i, (rel, v, toks)) in items.iter().enumerate() {
            // max similarity to already selected
            let mut max_sim = 0.0;
            for (_r2, _v2, t2) in selected.iter() {
                let sim = jaccard(toks, t2);
                if sim > max_sim { max_sim = sim; }
            }
            let mmr = lambda * *rel - (1.0 - lambda) * max_sim;
            if mmr > best_score { best_score = mmr; best_idx = i; }
        }
        let picked = items.remove(best_idx);
        selected.push(picked);
    }
    selected.into_iter().map(|(_s, v, _t)| v).collect()
}

// Recent file entities (by last_observed), optionally scoped to project
pub async fn select_recent_files(proj: Option<&str>, k: usize) -> Vec<Value> {
    let ws = store().read().unwrap();
    let g = if let Some(p) = proj { ws.proj_graphs.get(p) } else { Some(&ws.default_graph) };
    let mut items: Vec<(i64, Value)> = Vec::new();
    if let Some(g) = g {
        for n in g.nodes.values() {
            if !matches!(n.kind, NodeKind::Entity) { continue; }
            // File entities carry a 'path' prop; keep only those
            if !n.props.contains_key("path") { continue; }
            if let Some(p) = proj {
                if n.props.get("proj").and_then(|v| v.as_str()) != Some(p) { continue; }
            }
            let ts = n.last_observed.clone().unwrap_or_default();
            let ts_ms = chrono::DateTime::parse_from_rfc3339(&ts)
                .ok()
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            let item = json!({
                "id": n.id,
                "proj": n.props.get("proj").cloned().unwrap_or(Value::Null),
                "path": n.props.get("path").cloned().unwrap_or(Value::Null),
                "last": n.last_observed,
            });
            items.push((ts_ms, item));
        }
    }
    items.sort_by(|a, b| b.0.cmp(&a.0));
    let kk = k.clamp(1, 50);
    items.into_iter().take(kk).map(|(_, v)| v).collect()
}
