use serde_json::{json, Value};

use crate::app_state::AppState;

#[derive(Default)]
pub struct MemoryService;

impl MemoryService {
    pub fn new() -> Self { Self }

    pub async fn snapshot(&self) -> Value {
        crate::ext::memory::memory().read().await.clone()
    }

    pub async fn save(&self) -> Result<(), String> {
        let snap = crate::ext::memory::memory().read().await.clone();
        crate::ext::io::save_json_file_async(&crate::ext::paths::memory_path(), &snap)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn load(&self) -> Result<Value, String> {
        match crate::ext::io::load_json_file_async(&crate::ext::paths::memory_path()).await {
            Some(v) => {
                let mut m = crate::ext::memory::memory().write().await;
                *m = v.clone();
                Ok(v)
            }
            None => Err("no memory.json".into()),
        }
    }

    pub async fn get_limit(&self) -> usize {
        *crate::ext::memory::mem_limit().read().await
    }

    pub async fn set_limit(&self, limit: usize) {
        let mut n = crate::ext::memory::mem_limit().write().await;
        *n = limit.max(1);
    }

    pub async fn apply(
        &self,
        state: &AppState,
        kind: String,
        value: Value,
        ttl_ms: Option<u64>,
    ) -> Result<(), String> {
        let mut mem = crate::ext::memory::memory().write().await;
        let lane = match kind.as_str() {
            "ephemeral" => mem.get_mut("ephemeral").and_then(Value::as_array_mut),
            "episodic" => mem.get_mut("episodic").and_then(Value::as_array_mut),
            "semantic" => mem.get_mut("semantic").and_then(Value::as_array_mut),
            "procedural" => mem.get_mut("procedural").and_then(Value::as_array_mut),
            _ => None,
        };
        if let Some(arr) = lane {
            arr.push(value.clone());
            let cap = { *crate::ext::memory::mem_limit().read().await };
            while arr.len() > cap { arr.remove(0); }
            let snap = mem.clone();
            drop(mem);
            let _ = crate::ext::io::save_json_file_async(&crate::ext::paths::memory_path(), &snap).await;
            let mut payload = json!({"kind": kind, "value": value, "ttl_ms": ttl_ms});
            crate::ext::corr::ensure_corr(&mut payload);
            state.bus.publish("Memory.Applied", &payload);
            Ok(())
        } else {
            Err("invalid kind".into())
        }
    }
}
