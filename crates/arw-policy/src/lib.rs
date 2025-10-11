use anyhow::{anyhow, Result};
use cedar_policy::{
    Authorizer, Context, Entities, Entity as CedarEntity, EntityId, EntityTypeName, EntityUid,
    PolicySet, RestrictedExpression,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::str::FromStr;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LeaseRule {
    pub kind_prefix: String,
    pub capability: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CedarConfig {
    #[serde(default)]
    pub policy: Option<String>,
    #[serde(default)]
    pub policy_path: Option<String>,
    #[serde(default)]
    pub entities: Option<Value>,
    #[serde(default)]
    pub entities_path: Option<String>,
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub schema_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub allow_all: bool,
    #[serde(default)]
    pub lease_rules: Vec<LeaseRule>,
    #[serde(default)]
    pub cedar: Option<CedarConfig>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            allow_all: true,
            lease_rules: vec![],
            cedar: None,
        }
    }
}

pub struct PolicyEngine {
    cfg: PolicyConfig,
    cedar: Option<CedarEngine>,
}

impl Clone for PolicyEngine {
    fn clone(&self) -> Self {
        Self::with_config(self.cfg.clone())
    }
}

impl fmt::Debug for PolicyEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolicyEngine")
            .field("cfg", &self.cfg)
            .finish()
    }
}

struct CedarEngine {
    authorizer: Authorizer,
    policies: PolicySet,
    base_entities: Option<Value>,
    schema: Option<cedar_policy::Schema>,
}

struct CedarEval {
    decision: cedar_policy::Decision,
    diagnostics: Value,
    model: Value,
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
            if let Ok(bytes) = fs::read(path) {
                if let Ok(cfg) = serde_json::from_slice::<PolicyConfig>(&bytes) {
                    return Self::with_config(cfg);
                }
            }
        }
        // Next: security posture presets
        if let Ok(posture) = std::env::var("ARW_SECURITY_POSTURE") {
            return Self::with_config(posture_to_config(&posture));
        }
        // Default posture when nothing set
        Self::with_config(posture_to_config("standard"))
    }

    fn with_config(cfg: PolicyConfig) -> Self {
        let cedar = CedarEngine::from_config(&cfg);
        Self { cfg, cedar }
    }

    pub fn evaluate_action(&self, kind: &str) -> Decision {
        let req = AbacRequest {
            action: kind.to_string(),
            subject: Some(Entity {
                kind: "Agent".into(),
                id: "local".into(),
                attrs: Value::Null,
            }),
            resource: Some(Entity {
                kind: "Action".into(),
                id: kind.to_string(),
                attrs: Value::Null,
            }),
        };
        self.evaluate_with(&req)
    }

    pub fn evaluate_abac(&self, req: &AbacRequest) -> Decision {
        self.evaluate_with(req)
    }

    fn evaluate_with(&self, req: &AbacRequest) -> Decision {
        let (needs_capability, capability) = self.required_capability(&req.action);
        let mut allow = self.cfg.allow_all || !needs_capability;
        let mut model: Option<Value> = Some(fallback_model(req));
        let mut explain = if self.cfg.allow_all {
            base_explain(&req.action, "allow_all", "allow")
        } else if needs_capability {
            base_explain(&req.action, "lease_required", "lease_required")
        } else {
            base_explain(&req.action, "policy_allow", "allow")
        };

        if let Some(cap) = &capability {
            explain["required_capability"] = json!(cap);
        }

        if let Some(engine) = &self.cedar {
            match engine.evaluate(&self.cfg, req, needs_capability, capability.as_deref()) {
                Ok(eval) => {
                    allow = matches!(eval.decision, cedar_policy::Decision::Allow);
                    explain["cedar"] = eval.diagnostics;
                    model = Some(eval.model);
                }
                Err(err) => {
                    explain["cedar_error"] = json!(err.to_string());
                }
            }
        }

        if self.cfg.allow_all {
            allow = true;
        }

        let require_capability = if needs_capability && !allow {
            capability.clone()
        } else {
            None
        };

        if needs_capability {
            if let Some(cap) = capability.as_ref() {
                if allow {
                    explain["reason"] = json!("lease_satisfied");
                    explain["mode"] = json!("lease_satisfied");
                    explain["message"] = json!(format!(
                        "Action {} allowed; capability {} satisfied.",
                        req.action, cap
                    ));
                } else {
                    explain["reason"] = json!("lease_required");
                    explain["mode"] = json!("lease_required");
                    explain["message"] = json!(format!(
                        "Action {} requires capability {}. Acquire a lease via POST /leases.",
                        req.action, cap
                    ));
                }
            }
        } else if self.cfg.allow_all {
            explain["message"] = json!(format!(
                "Action {} allowed via allow_all posture.",
                req.action
            ));
        } else {
            explain["reason"] = json!("allow");
            explain["message"] = json!(format!("Action {} allowed.", req.action));
        }

        Decision {
            allow,
            require_capability,
            explain,
            model,
        }
    }

    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(&self.cfg).unwrap_or(json!({}))
    }

    fn required_capability(&self, kind: &str) -> (bool, Option<String>) {
        if self.cfg.allow_all {
            return (false, None);
        }
        self.cfg
            .lease_rules
            .iter()
            .find(|r| kind.starts_with(&r.kind_prefix))
            .map(|rule| (true, Some(rule.capability.clone())))
            .unwrap_or((false, None))
    }
}

impl CedarEngine {
    fn from_config(cfg: &PolicyConfig) -> Option<Self> {
        let cedar_cfg = cfg.cedar.as_ref();
        let policy_src = cedar_cfg
            .and_then(|c| {
                c.policy_path
                    .as_ref()
                    .and_then(|p| fs::read_to_string(p).ok())
                    .or_else(|| c.policy.clone())
            })
            .unwrap_or(default_policy_source());
        let policies: PolicySet = policy_src.parse().ok()?;

        let schema = cedar_cfg.and_then(|c| {
            if let Some(path) = &c.schema_path {
                fs::read_to_string(path)
                    .ok()
                    .and_then(|raw| cedar_policy::Schema::from_str(&raw).ok())
            } else {
                c.schema
                    .as_ref()
                    .cloned()
                    .and_then(|value| cedar_policy::Schema::from_json_value(value).ok())
            }
        });

        let base_entities = cedar_cfg.and_then(|c| {
            if let Some(path) = &c.entities_path {
                fs::read_to_string(path)
                    .ok()
                    .and_then(|raw| serde_json::from_str(&raw).ok())
            } else {
                c.entities.clone()
            }
        });

        Some(Self {
            authorizer: Authorizer::new(),
            policies,
            base_entities,
            schema,
        })
    }

    fn evaluate(
        &self,
        cfg: &PolicyConfig,
        req: &AbacRequest,
        needs_capability: bool,
        required_capability: Option<&str>,
    ) -> Result<CedarEval> {
        let (subject_kind, subject_id) = req
            .subject
            .as_ref()
            .map(|s| (s.kind.as_str(), s.id.as_str()))
            .unwrap_or(("Agent", "local"));
        let (resource_kind, resource_id) = req
            .resource
            .as_ref()
            .map(|r| (r.kind.as_str(), r.id.as_str()))
            .unwrap_or(("Resource", req.action.as_str()));

        let principal_uid = make_uid(subject_kind, subject_id, "Agent", "local");
        let action_uid = make_uid("Action", &req.action, "Action", "action");
        let resource_uid = make_uid(resource_kind, resource_id, "Resource", "resource");

        let leases = leases_from_request(req);
        let principal_attrs = leases_to_attrs(&leases);
        let action_attrs = vec![(
            "kind".to_string(),
            RestrictedExpression::new_string(req.action.clone()),
        )]
        .into_iter()
        .collect();
        let resource_attrs: HashMap<String, RestrictedExpression> = HashMap::new();

        let principal = CedarEntity::new(principal_uid.clone(), principal_attrs, HashSet::new())
            .map_err(|err| anyhow!("failed to build principal entity: {err}"))?;
        let action = CedarEntity::new(action_uid.clone(), action_attrs, HashSet::new())
            .map_err(|err| anyhow!("failed to build action entity: {err}"))?;
        let resource = CedarEntity::new(resource_uid.clone(), resource_attrs, HashSet::new())
            .map_err(|err| anyhow!("failed to build resource entity: {err}"))?;

        let mut entities =
            Entities::from_entities(vec![principal, action, resource], self.schema.as_ref())
                .map_err(|err| anyhow!("failed to assemble cedar entities: {err}"))?;

        if let Some(extra) = &self.base_entities {
            entities = entities
                .add_entities_from_json_value(extra.clone(), self.schema.as_ref())
                .map_err(|err| anyhow!("failed to merge cedar base entities: {err}"))?;
        }

        let mut context_snapshot = serde_json::Map::new();
        context_snapshot.insert("kind".into(), Value::String(req.action.clone()));
        context_snapshot.insert("needs_capability".into(), Value::Bool(needs_capability));
        context_snapshot.insert("allow_all".into(), Value::Bool(cfg.allow_all));
        if let Some(cap) = required_capability {
            context_snapshot.insert("required_capability".into(), Value::String(cap.to_string()));
        }
        context_snapshot.insert(
            "granted_capabilities".into(),
            Value::Array(leases.iter().map(|s| Value::String(s.clone())).collect()),
        );

        let mut context_pairs = vec![
            (
                "kind".to_string(),
                RestrictedExpression::new_string(req.action.clone()),
            ),
            (
                "needs_capability".to_string(),
                RestrictedExpression::new_bool(needs_capability),
            ),
            (
                "allow_all".to_string(),
                RestrictedExpression::new_bool(cfg.allow_all),
            ),
            (
                "granted_capabilities".to_string(),
                RestrictedExpression::new_set(
                    leases.iter().cloned().map(RestrictedExpression::new_string),
                ),
            ),
        ];
        if let Some(cap) = required_capability {
            context_pairs.push((
                "required_capability".to_string(),
                RestrictedExpression::new_string(cap.to_string()),
            ));
        }
        let context = Context::from_pairs(context_pairs)
            .map_err(|err| anyhow!("failed to construct cedar context: {err}"))?;

        let request = cedar_policy::Request::new(
            principal_uid,
            action_uid,
            resource_uid,
            context,
            self.schema.as_ref(),
        )
        .map_err(|err| anyhow!("failed to construct cedar request: {err}"))?;

        let response = self
            .authorizer
            .is_authorized(&request, &self.policies, &entities);
        let diagnostics = json!({
            "policies": response
                .diagnostics()
                .reason()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
            "errors": response
                .diagnostics()
                .errors()
                .map(|err| err.to_string())
                .collect::<Vec<_>>(),
        });

        let resource_model = req.resource.as_ref().map(|r| {
            json!({
                "kind": r.kind,
                "id": r.id,
                "attrs": r.attrs,
            })
        });

        let mut model = serde_json::Map::new();
        model.insert(
            "principal".into(),
            json!({
                "kind": subject_kind,
                "id": subject_id,
                "leases": leases,
            }),
        );
        model.insert("action".into(), json!({ "id": req.action }));
        model.insert(
            "resource".into(),
            resource_model.unwrap_or_else(|| {
                json!({
                    "kind": resource_kind,
                    "id": resource_id,
                })
            }),
        );
        model.insert("context".into(), Value::Object(context_snapshot));
        model.insert(
            "policy".into(),
            json!({
                "allow_all": cfg.allow_all,
                "lease_rules": cfg.lease_rules,
            }),
        );
        model.insert(
            "cedar_decision".into(),
            json!(match response.decision() {
                cedar_policy::Decision::Allow => "allow",
                cedar_policy::Decision::Deny => "deny",
            }),
        );

        Ok(CedarEval {
            decision: response.decision(),
            diagnostics,
            model: Value::Object(model),
        })
    }
}

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

fn leases_to_attrs(leases: &[String]) -> HashMap<String, RestrictedExpression> {
    let mut attrs = HashMap::new();
    if !leases.is_empty() {
        attrs.insert(
            "leases".to_string(),
            RestrictedExpression::new_set(
                leases.iter().cloned().map(RestrictedExpression::new_string),
            ),
        );
    }
    attrs
}

fn leases_from_request(req: &AbacRequest) -> Vec<String> {
    req.subject
        .as_ref()
        .and_then(|s| s.attrs.as_object())
        .and_then(|obj| obj.get("leases"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn make_uid(kind: &str, id: &str, fallback_type: &str, fallback_id: &str) -> EntityUid {
    let type_name = EntityTypeName::from_str(kind)
        .or_else(|_| EntityTypeName::from_str(&sanitize_type(kind)))
        .unwrap_or_else(|_| {
            EntityTypeName::from_str(fallback_type).expect("fallback type should parse")
        });
    let entity_id = EntityId::from_str(id)
        .or_else(|_| EntityId::from_str(&sanitize_id(id)))
        .unwrap_or_else(|_| EntityId::from_str(fallback_id).expect("fallback id should parse"));
    EntityUid::from_type_name_and_id(type_name, entity_id)
}

fn sanitize_type(raw: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if out.is_empty() {
        "Resource".into()
    } else {
        out
    }
}

fn sanitize_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() => c,
            '_' | '-' | '.' => c,
            _ => '_',
        })
        .collect();
    if cleaned.is_empty() {
        "default".into()
    } else {
        cleaned
    }
}

fn default_policy_source() -> String {
    "permit(principal, action, resource)\nwhen {\n  !context.needs_capability || context.required_capability in context.granted_capabilities\n};\n".to_string()
}

fn base_explain(action: &str, mode: &str, reason: &str) -> Value {
    json!({
        "action": action,
        "mode": mode,
        "reason": reason,
    })
}

fn fallback_model(req: &AbacRequest) -> Value {
    json!({
        "principal": req
            .subject
            .as_ref()
            .map(|s| json!({ "kind": s.kind, "id": s.id, "attrs": s.attrs }))
            .unwrap_or(json!({"kind":"Agent","id":"local"})),
        "action": { "id": req.action },
        "resource": req
            .resource
            .as_ref()
            .map(|r| json!({ "kind": r.kind, "id": r.id, "attrs": r.attrs }))
            .unwrap_or(json!({"kind":"action","id": req.action})),
    })
}

fn posture_to_config(posture: &str) -> PolicyConfig {
    let p = posture.trim().to_ascii_lowercase();
    match p.as_str() {
        // Dev-friendly: wide open
        "relaxed" => PolicyConfig {
            allow_all: true,
            lease_rules: vec![],
            cedar: None,
        },
        // Default: gate sensitive areas with leases
        "standard" => PolicyConfig {
            allow_all: false,
            lease_rules: vec![
                LeaseRule {
                    kind_prefix: "net.http.".into(),
                    capability: "net:http".into(),
                },
                LeaseRule {
                    kind_prefix: "net.tcp.".into(),
                    capability: "net:tcp".into(),
                },
                LeaseRule {
                    kind_prefix: "fs.".into(),
                    capability: "fs".into(),
                },
                LeaseRule {
                    kind_prefix: "context.rehydrate".into(),
                    capability: "context:rehydrate:file".into(),
                },
                LeaseRule {
                    kind_prefix: "models.download".into(),
                    capability: "models:download".into(),
                },
                LeaseRule {
                    kind_prefix: "tools.browser.".into(),
                    capability: "browser".into(),
                },
                LeaseRule {
                    kind_prefix: "app.".into(),
                    capability: "app".into(),
                },
                LeaseRule {
                    kind_prefix: "shell.".into(),
                    capability: "shell".into(),
                },
                LeaseRule {
                    kind_prefix: "runtime.".into(),
                    capability: "runtime:manage".into(),
                },
            ],
            cedar: None,
        },
        // Hardened: require leases for most effects (network, fs, process, app)
        "strict" => PolicyConfig {
            allow_all: false,
            lease_rules: vec![
                LeaseRule {
                    kind_prefix: "net.".into(),
                    capability: "net".into(),
                },
                LeaseRule {
                    kind_prefix: "fs.".into(),
                    capability: "fs".into(),
                },
                LeaseRule {
                    kind_prefix: "context.".into(),
                    capability: "context".into(),
                },
                LeaseRule {
                    kind_prefix: "models.".into(),
                    capability: "models".into(),
                },
                LeaseRule {
                    kind_prefix: "tools.".into(),
                    capability: "tools".into(),
                },
                LeaseRule {
                    kind_prefix: "app.".into(),
                    capability: "app".into(),
                },
                LeaseRule {
                    kind_prefix: "shell.".into(),
                    capability: "shell".into(),
                },
                LeaseRule {
                    kind_prefix: "system.".into(),
                    capability: "system".into(),
                },
                LeaseRule {
                    kind_prefix: "runtime.".into(),
                    capability: "runtime:manage".into(),
                },
            ],
            cedar: None,
        },
        _ => posture_to_config("standard"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_all_short_circuits() {
        let engine = PolicyEngine::with_config(PolicyConfig {
            allow_all: true,
            lease_rules: vec![LeaseRule {
                kind_prefix: "net.".into(),
                capability: "net".into(),
            }],
            cedar: None,
        });
        let decision = engine.evaluate_action("net.http.get");
        assert!(decision.allow);
        assert!(decision.require_capability.is_none());
        assert_eq!(decision.explain["mode"], "allow_all");
        assert!(decision.model.is_some());
    }

    #[test]
    fn lease_rules_gate_disallowed_actions() {
        let engine = PolicyEngine::with_config(PolicyConfig {
            allow_all: false,
            lease_rules: vec![LeaseRule {
                kind_prefix: "net.http.".into(),
                capability: "net:http".into(),
            }],
            cedar: None,
        });
        let decision = engine.evaluate_action("net.http.get");
        assert!(!decision.allow);
        assert_eq!(decision.require_capability.as_deref(), Some("net:http"));
        assert_eq!(decision.explain["reason"], "lease_required");
        assert!(decision
            .explain
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("requires capability"));
    }

    #[test]
    fn abac_response_includes_model_context() {
        let engine = PolicyEngine::with_config(PolicyConfig {
            allow_all: false,
            lease_rules: vec![],
            cedar: None,
        });
        let req = AbacRequest {
            action: "context.rehydrate.file".into(),
            subject: Some(Entity {
                kind: "Agent".into(),
                id: "tester".into(),
                attrs: json!({"leases":["context:rehydrate:file"]}),
            }),
            resource: Some(Entity {
                kind: "project".into(),
                id: "alpha".into(),
                attrs: json!({"sensitivity": "private"}),
            }),
        };
        let decision = engine.evaluate_abac(&req);
        assert!(decision.allow);
        let model = decision.model.expect("model populated");
        assert_eq!(model["principal"]["id"], "tester");
        assert_eq!(model["resource"]["id"], "alpha");
        assert_eq!(model["action"]["id"], "context.rehydrate.file");
    }

    #[test]
    fn cedar_deny_applies_without_lease_requirement() {
        let policy = r#"
        permit(principal, action, resource);
        forbid(principal, action, resource) when { action.kind == "models.download" };
        "#;
        let engine = PolicyEngine::with_config(PolicyConfig {
            allow_all: false,
            lease_rules: vec![],
            cedar: Some(CedarConfig {
                policy: Some(policy.into()),
                ..Default::default()
            }),
        });

        let request = AbacRequest {
            action: "models.download".into(),
            ..Default::default()
        };

        let decision = engine.evaluate_abac(&request);
        assert!(
            !decision.allow,
            "cedar forbid verdict should deny action without lease requirement"
        );
        assert!(decision.require_capability.is_none());
    }
}
