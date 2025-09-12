use once_cell::sync::OnceCell;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

struct Entry {
    summary: &'static str,
    exec: fn(&Value) -> Result<Value, String>,
}

static REG: OnceCell<RwLock<HashMap<&'static str, Entry>>> = OnceCell::new();

fn reg() -> &'static RwLock<HashMap<&'static str, Entry>> {
    REG.get_or_init(|| {
        let mut map: HashMap<&'static str, Entry> = HashMap::new();
        // Built-in examples
        map.insert(
            "math.add",
            Entry {
                summary:
                    "Add two numbers: input {\"a\": number, \"b\": number} -> {\"sum\": number}",
                exec: |input| {
                    let a = input
                        .get("a")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'a'")?;
                    let b = input
                        .get("b")
                        .and_then(|v| v.as_f64())
                        .ok_or("missing or invalid 'b'")?;
                    Ok(json!({"sum": a + b}))
                },
            },
        );
        map.insert(
            "time.now",
            Entry {
                summary: "UTC time in ms: input {} -> {\"now_ms\": number}",
                exec: |_input| {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis() as i64;
                    Ok(json!({"now_ms": now}))
                },
            },
        );
        RwLock::new(map)
    })
}

pub fn run(id: &str, input: &Value) -> Result<Value, String> {
    let map = reg().read().unwrap();
    if let Some(ent) = map.get(id) {
        (ent.exec)(input)
    } else {
        Err(format!("unknown tool id: {}", id))
    }
}

pub fn list() -> Vec<(&'static str, &'static str)> {
    let map = reg().read().unwrap();
    map.iter().map(|(k, v)| (*k, v.summary)).collect()
}
