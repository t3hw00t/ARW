use utoipa::{OpenApi, ToSchema};

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct HealthOk {
    pub ok: bool,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct HttpInfo {
    pub bind: String,
    pub port: u16,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct AboutCounts {
    pub public: usize,
    pub admin: usize,
    pub total: usize,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct PerfPreset {
    pub tier: Option<String>,
    pub http_max_conc: Option<usize>,
    pub actions_queue_max: Option<i64>,
}

#[allow(dead_code)]
#[derive(ToSchema)]
pub struct AboutResponse {
    pub service: String,
    pub version: String,
    pub http: HttpInfo,
    #[schema(nullable, value_type = Option<String>)]
    pub docs_url: Option<String>,
    #[schema(nullable, value_type = Option<String>)]
    pub security_posture: Option<String>,
    pub counts: AboutCounts,
    #[schema(example = json!( ["GET /healthz", "GET /about"] ))]
    pub endpoints: Vec<String>,
    #[schema(value_type = Vec<serde_json::Value>)]
    pub endpoints_meta: Vec<serde_json::Value>,
    pub perf_preset: PerfPreset,
}

#[allow(dead_code)]
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api_meta::healthz,
        crate::api_meta::about,
        crate::api_state::state_models,
        crate::api_state::state_actions,
        crate::api_events::events_sse,
    ),
    components(
        schemas(HealthOk, HttpInfo, AboutCounts, PerfPreset, AboutResponse)
    ),
    tags(
        (name = "Meta", description = "Service metadata and health"),
        (name = "State", description = "Read‑models (actions, models, egress, episodes)"),
        (name = "Events", description = "Server‑Sent Events stream")
    )
)]
pub struct ApiDoc;
