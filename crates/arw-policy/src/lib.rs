use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaseRule {
    pub kind_prefix: String,
    pub capability: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub allow_all: bool,
    #[serde(default)]
    pub lease_rules: Vec<LeaseRule>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self { allow_all: true, lease_rules: vec![] }
    }
}

#[derive(Clone, Debug)]
pub struct PolicyEngine {
    cfg: PolicyConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Decision {
    pub allow: bool,
    #[serde(default)]
    pub require_capability: Option<String>,
    #[serde(default)]
    pub explain: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<serde_json::Value>,
}

impl PolicyEngine {
    pub fn load_from_env() -> Self {
        // Highest precedence: explicit JSON file
        if let Ok(path) = std::env::var("ARW_POLICY_FILE") {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(cfg) = serde_json::from_slice::<PolicyConfig>(&bytes) {
                    return Self { cfg };
                }
            }
        }
        // Next: security posture presets
        if let Ok(posture) = std::env::var("ARW_SECURITY_POSTURE") {
            return Self { cfg: posture_to_config(&posture) };
        }
        // Default posture when nothing set
        Self { cfg: posture_to_config("standard") }
    }

    pub fn evaluate_action(&self, kind: &str) -> Decision {
        if self.cfg.allow_all {
            return Decision { allow: true, require_capability: None, explain: json!({"mode":"allow_all"}), model: None };
        }
        if let Some(rule) = self.cfg.lease_rules.iter().find(|r| kind.starts_with(&r.kind_prefix)) {
            return Decision {
                allow: false,
                require_capability: Some(rule.capability.clone()),
                explain: json!({"reason":"lease_required","kind_prefix": rule.kind_prefix, "capability": rule.capability}),
                model: None,
            };
        }
        Decision { allow: true, require_capability: None, explain: json!({"mode":"default_allow"}), model: None }
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(&self.cfg).unwrap_or(json!({}))
    }
}

// ----- Cedar-like ABAC façade -----
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Entity {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub attrs: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AbacRequest {
    /// Action kind (e.g., net.http.get)
    pub action: String,
    #[serde(default)]
    pub subject: Option<Entity>,
    #[serde(default)]
    pub resource: Option<Entity>,
}

impl PolicyEngine {
    pub fn evaluate_abac(&self, req: &AbacRequest) -> Decision {
        let mut d = self.evaluate_action(&req.action);
        // Attach a simple Cedar-shaped model for explainability/migration
        let model = json!({
            "principal": req.subject.as_ref().map(|s| json!({"kind": s.kind, "id": s.id, "attrs": s.attrs})).unwrap_or(json!({"kind":"node","id":"local"})),
            "action": { "id": req.action },
            "resource": req.resource.as_ref().map(|r| json!({"kind": r.kind, "id": r.id, "attrs": r.attrs})).unwrap_or(json!({"kind":"action","id": req.action}))
        });
        d.model = Some(model);
        d
    }
}

fn posture_to_config(posture: &str) -> PolicyConfig {
    let p = posture.trim().to_ascii_lowercase();
    match p.as_str() {
        // Dev-friendly: wide open
        "relaxed" => PolicyConfig { allow_all: true, lease_rules: vec![] },
        // Default: gate sensitive areas with leases
        "standard" => PolicyConfig {
            allow_all: false,
            lease_rules: vec![
                LeaseRule { kind_prefix: "net.http.".into(), capability: "net:http".into() },
                LeaseRule { kind_prefix: "net.tcp.".into(), capability: "net:tcp".into() },
                LeaseRule { kind_prefix: "fs.".into(), capability: "fs".into() },
                LeaseRule { kind_prefix: "context.rehydrate".into(), capability: "context:rehydrate:file".into() },
                LeaseRule { kind_prefix: "models.download".into(), capability: "models:download".into() },
                LeaseRule { kind_prefix: "tools.browser.".into(), capability: "browser".into() },
                LeaseRule { kind_prefix: "app.".into(), capability: "app".into() },
                LeaseRule { kind_prefix: "shell.".into(), capability: "shell".into() },
            ],
        },
        // Hardened: require leases for most effects (network, fs, process, app)
        "strict" => PolicyConfig {
            allow_all: false,
            lease_rules: vec![
                LeaseRule { kind_prefix: "net.".into(), capability: "net".into() },
                LeaseRule { kind_prefix: "fs.".into(), capability: "fs".into() },
                LeaseRule { kind_prefix: "context.".into(), capability: "context".into() },
                LeaseRule { kind_prefix: "models.".into(), capability: "models".into() },
                LeaseRule { kind_prefix: "tools.".into(), capability: "tools".into() },
                LeaseRule { kind_prefix: "app.".into(), capability: "app".into() },
                LeaseRule { kind_prefix: "shell.".into(), capability: "shell".into() },
                LeaseRule { kind_prefix: "system.".into(), capability: "system".into() },
            ],
        },
        _ => posture_to_config("standard"),
    }
}
