use crate::AppState;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub struct ActionReq {
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub idem_key: Option<String>,
}

pub async fn actions_submit_post(State(state): State<AppState>, Json(req): Json<ActionReq>) -> impl IntoResponse {
    // Idempotency by idem_key when kernel is available
    let mut id: String = uuid::Uuid::new_v4().to_string();
    if let Some(k) = crate::ext::kernel() {
        if let Some(ref idem) = req.idem_key {
            if let Ok(Some(existing)) = k.find_action_by_idem(idem) {
                id = existing;
                let mut payload = json!({"id": id, "kind": req.kind, "status": "duplicate"});
                crate::ext::corr::ensure_corr(&mut payload);
                state
                    .bus
                    .publish(crate::ext::topics::TOPIC_ACTIONS_SUBMITTED, &payload);
                return crate::ext::ok(json!({"id": id, "duplicate": true}));
            }
        }
        let _ = k.insert_action(&id, &req.kind, &req.input, None, req.idem_key.as_deref(), "queued");
    }
    // Publish submitted event (bus remains SoT for live apps; kernel persists)
    let mut payload = json!({"id": id, "kind": req.kind, "status": "queued"});
    crate::ext::corr::ensure_corr(&mut payload);
    state
        .bus
        .publish(crate::ext::topics::TOPIC_ACTIONS_SUBMITTED, &payload);
    crate::ext::ok(json!({"id": id}))
}

