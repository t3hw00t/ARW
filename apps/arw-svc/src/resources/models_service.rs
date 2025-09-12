use serde_json::{json, Value};

use crate::app_state::AppState;

#[derive(Default)]
pub struct ModelsService;

impl ModelsService {
    pub fn new() -> Self { Self }

    pub async fn list(&self) -> Vec<Value> {
        crate::ext::models().read().await.clone()
    }

    pub async fn refresh(&self, state: &AppState) -> Vec<Value> {
        let new = super::super::ext::default_models();
        {
            let mut m = crate::ext::models().write().await;
            *m = new.clone();
        }
        let _ = super::super::ext::io::save_json_file_async(&super::super::ext::paths::models_path(), &Value::Array(new.clone())).await;
        state.bus.publish("Models.Refreshed", &json!({"count": new.len()}));
        new
    }

    pub async fn save(&self) -> Result<(), String> {
        let v = crate::ext::models().read().await.clone();
        super::super::ext::io::save_json_file_async(&super::super::ext::paths::models_path(), &Value::Array(v))
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn load(&self) -> Result<Vec<Value>, String> {
        match super::super::ext::io::load_json_file_async(&super::super::ext::paths::models_path()).await.and_then(|v| v.as_array().cloned()) {
            Some(arr) => {
                {
                    let mut m = crate::ext::models().write().await;
                    *m = arr.clone();
                }
                Ok(arr)
            }
            None => Err("no models.json".into()),
        }
    }

    pub async fn add(&self, state: &AppState, id: String, provider: Option<String>) {
        let mut v = crate::ext::models().write().await;
        if !v.iter().any(|m| m.get("id").and_then(|s| s.as_str()) == Some(&id)) {
            v.push(json!({"id": id, "provider": provider.unwrap_or_else(|| "local".to_string()), "status":"available"}));
            state.bus.publish("Models.Changed", &json!({"op":"add","id": v.last().and_then(|m| m.get("id")).cloned()}));
        }
    }

    pub async fn delete(&self, state: &AppState, id: String) {
        let mut v = crate::ext::models().write().await;
        let before = v.len();
        v.retain(|m| m.get("id").and_then(|s| s.as_str()) != Some(&id));
        if v.len() != before {
            state.bus.publish("Models.Changed", &json!({"op":"delete","id": id}));
        }
    }

    pub async fn default_get(&self) -> String {
        crate::ext::default_model().read().await.clone()
    }

    pub async fn default_set(&self, state: &AppState, id: String) -> Result<(), String> {
        {
            let mut d = crate::ext::default_model().write().await;
            *d = id.clone();
        }
        state.bus.publish("Models.Changed", &json!({"op":"default","id": id}));
        super::super::ext::io::save_json_file_async(&super::super::ext::paths::models_path(), &Value::Array(crate::ext::models().read().await.clone()))
            .await
            .map_err(|e| e.to_string())
    }
}
