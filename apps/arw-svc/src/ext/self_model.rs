use serde_json::{json, Value};

// Minimal helpers for storing and proposing agent self-models
// Placement: <state>/self/{agent}.json and proposals under <state>/self/_proposals/{id}.json

pub async fn load(agent: &str) -> Option<Value> {
    let Some(p) = crate::ext::paths::self_model_path(agent) else { return None; };
    crate::ext::io::load_json_file_async(&p).await
}

pub async fn save(agent: &str, model: &Value) -> Result<(), String> {
    use tokio::fs as afs;
    let dir = crate::ext::paths::self_dir();
    if let Err(e) = afs::create_dir_all(&dir).await { return Err(e.to_string()); }
    let Some(p) = crate::ext::paths::self_model_path(agent) else { return Err("invalid agent id".into()); };
    crate::ext::io::save_json_file_async(&p, model).await.map_err(|e| e.to_string())
}

pub async fn list() -> Vec<(String, Value)> {
    use tokio::fs as afs;
    let mut out: Vec<(String, Value)> = Vec::new();
    let dir = crate::ext::paths::self_dir();
    if let Ok(mut rd) = afs::read_dir(&dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            if let Some(name) = ent.file_name().to_str() {
                if name.ends_with(".json") && name != "world.json" {
                    if let Ok(body) = afs::read(ent.path()).await {
                        if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                            let id = name.trim_end_matches(".json").to_string();
                            out.push((id, v));
                        }
                    }
                }
            }
        }
    }
    out
}

pub async fn propose_update(
    agent: &str,
    patch: Value,
    rationale: Option<String>,
) -> Result<Value, String> {
    use tokio::fs as afs;
    // Load current model (object)
    let current = load(agent).await.unwrap_or_else(|| json!({}));
    // Merge JSON objects; simple deep merge
    fn merge(a: &mut Value, b: &Value) {
        match (a, b) {
            (Value::Object(ao), Value::Object(bo)) => {
                for (k, bv) in bo {
                    match ao.get_mut(k) {
                        Some(av) => merge(av, bv),
                        None => {
                            ao.insert(k.clone(), bv.clone());
                        }
                    }
                }
            }
            (a, b) => *a = b.clone(),
        }
    }
    let mut proposed = current.clone();
    merge(&mut proposed, &patch);

    // Persist proposal
    let prop_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let envelope = json!({
        "id": prop_id,
        "time": now,
        "agent": agent,
        "rationale": rationale,
        "patch": patch,
        "current": current,
        "proposed": proposed,
    });
    let dir = crate::ext::paths::self_proposals_dir();
    if let Err(e) = afs::create_dir_all(&dir).await { return Err(e.to_string()); }
    let path = dir.join(format!("{}.json", envelope["id"].as_str().unwrap_or("proposal")));
    let _ = crate::ext::io::save_json_file_async(&path, &envelope).await;
    Ok(envelope)
}

pub async fn apply_proposal(prop_id: &str) -> Result<Value, String> {
    // Load the proposal, write proposed -> model path, emit compact response
    let path = crate::ext::paths::self_proposals_dir().join(format!("{}.json", prop_id));
    let Some(v) = crate::ext::io::load_json_file_async(&path).await else { return Err("proposal_not_found".into()); };
    let agent = v.get("agent").and_then(|s| s.as_str()).ok_or_else(|| "bad_proposal".to_string())?;
    let proposed = v.get("proposed").ok_or_else(|| "bad_proposal".to_string())?.clone();
    save(agent, &proposed).await?;
    Ok(json!({"id": prop_id, "agent": agent, "applied": true}))
}

