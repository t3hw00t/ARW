use crate::AppState;
use axum::extract::FromRef;
use std::sync::OnceLock;

static GLOBAL_STATE: OnceLock<AppState> = OnceLock::new();

pub fn set_global_state(state: AppState) {
    let _ = GLOBAL_STATE.set(state);
}

impl FromRef<()> for AppState {
    fn from_ref(_: &()) -> AppState {
        GLOBAL_STATE
            .get()
            .expect("AppState not initialized")
            .clone()
    }
}
