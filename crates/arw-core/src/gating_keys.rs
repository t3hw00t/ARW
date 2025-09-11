//! Central registry of gating keys for SoT and docs.
pub const QUEUE_ENQUEUE: &str = "queue:enqueue";
pub const EVENTS_TASK_COMPLETED: &str = "events:Task.Completed";

// Memory
pub const MEMORY_GET: &str = "memory:get";
pub const MEMORY_SAVE: &str = "memory:save";
pub const MEMORY_LOAD: &str = "memory:load";
pub const MEMORY_APPLY: &str = "memory:apply";
pub const MEMORY_LIMIT_GET: &str = "memory:limit:get";
pub const MEMORY_LIMIT_SET: &str = "memory:limit:set";

// Models
pub const MODELS_LIST: &str = "models:list";
pub const MODELS_REFRESH: &str = "models:refresh";
pub const MODELS_SAVE: &str = "models:save";
pub const MODELS_LOAD: &str = "models:load";
pub const MODELS_ADD: &str = "models:add";
pub const MODELS_DELETE: &str = "models:delete";
pub const MODELS_DEFAULT_GET: &str = "models:default:get";
pub const MODELS_DEFAULT_SET: &str = "models:default:set";
pub const MODELS_DOWNLOAD: &str = "models:download";

// Feedback
pub const FEEDBACK_STATE: &str = "feedback:state";
pub const FEEDBACK_SIGNAL: &str = "feedback:signal";
pub const FEEDBACK_ANALYZE: &str = "feedback:analyze";
pub const FEEDBACK_APPLY: &str = "feedback:apply";
pub const FEEDBACK_AUTO: &str = "feedback:auto";
pub const FEEDBACK_RESET: &str = "feedback:reset";

// Tools
pub const TOOLS_LIST: &str = "tools:list";
pub const TOOLS_RUN: &str = "tools:run";

// Chat
pub const CHAT_SEND: &str = "chat:send";
pub const CHAT_CLEAR: &str = "chat:clear";

// Governor
pub const GOVERNOR_SET: &str = "governor:set";
pub const GOVERNOR_HINTS_SET: &str = "governor:hints:set";

// Hierarchy
pub const HIERARCHY_HELLO: &str = "hierarchy:hello";
pub const HIERARCHY_OFFER: &str = "hierarchy:offer";
pub const HIERARCHY_ACCEPT: &str = "hierarchy:accept";
pub const HIERARCHY_STATE_GET: &str = "hierarchy:state:get";
pub const HIERARCHY_ROLE_SET: &str = "hierarchy:role:set";

// Introspection
pub const INTROSPECT_TOOLS: &str = "introspect:tools";
pub const INTROSPECT_SCHEMA: &str = "introspect:schema";
pub const INTROSPECT_STATS: &str = "introspect:stats";
pub const INTROSPECT_PROBE: &str = "introspect:probe";

/// Return all known static keys (dynamic keys like task:<id> are omitted).
pub fn list() -> Vec<&'static str> {
    vec![
        QUEUE_ENQUEUE,
        EVENTS_TASK_COMPLETED,
        MEMORY_GET,
        MEMORY_SAVE,
        MEMORY_LOAD,
        MEMORY_APPLY,
        MEMORY_LIMIT_GET,
        MEMORY_LIMIT_SET,
        MODELS_LIST,
        MODELS_REFRESH,
        MODELS_SAVE,
        MODELS_LOAD,
        MODELS_ADD,
        MODELS_DELETE,
        MODELS_DEFAULT_GET,
        MODELS_DEFAULT_SET,
        MODELS_DOWNLOAD,
        FEEDBACK_STATE,
        FEEDBACK_SIGNAL,
        FEEDBACK_ANALYZE,
        FEEDBACK_APPLY,
        FEEDBACK_AUTO,
        FEEDBACK_RESET,
        TOOLS_LIST,
        TOOLS_RUN,
        CHAT_SEND,
        CHAT_CLEAR,
        GOVERNOR_SET,
        GOVERNOR_HINTS_SET,
        HIERARCHY_HELLO,
        HIERARCHY_OFFER,
        HIERARCHY_ACCEPT,
        HIERARCHY_STATE_GET,
        HIERARCHY_ROLE_SET,
        INTROSPECT_TOOLS,
        INTROSPECT_SCHEMA,
        INTROSPECT_STATS,
        INTROSPECT_PROBE,
    ]
}
