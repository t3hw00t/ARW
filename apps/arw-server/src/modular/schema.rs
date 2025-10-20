use jsonschema::Validator;
use once_cell::sync::Lazy;
use serde_json::Value;

pub(crate) static MODULAR_AGENT_MESSAGE_SCHEMA: Lazy<Validator> = Lazy::new(|| {
    let raw = include_str!("../../../../spec/schemas/modular_agent_message.json");
    let schema: Value =
        serde_json::from_str(raw).expect("spec/schemas/modular_agent_message.json must parse");
    jsonschema::validator_for(&schema).expect("modular_agent_message schema must be valid")
});

pub(crate) static MODULAR_TOOL_INVOCATION_SCHEMA: Lazy<Validator> = Lazy::new(|| {
    let raw = include_str!("../../../../spec/schemas/modular_tool_invocation.json");
    let schema: Value =
        serde_json::from_str(raw).expect("spec/schemas/modular_tool_invocation.json must parse");
    jsonschema::validator_for(&schema).expect("modular_tool_invocation schema must be valid")
});
