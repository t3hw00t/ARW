use std::path::PathBuf;

pub(crate) fn state_dir() -> PathBuf {
    let v = arw_core::load_effective_paths();
    let s = v.get("state_dir").and_then(|x| x.as_str()).unwrap_or(".");
    PathBuf::from(s.replace('\\', "/"))
}

pub(crate) fn memory_path() -> PathBuf {
    state_dir().join("memory.json")
}
pub(crate) fn models_path() -> PathBuf {
    state_dir().join("models.json")
}
pub(crate) fn orch_path() -> PathBuf {
    state_dir().join("orchestration.json")
}
pub(crate) fn feedback_path() -> PathBuf {
    state_dir().join("feedback.json")
}
pub(crate) fn audit_path() -> PathBuf {
    state_dir().join("audit.log")
}
