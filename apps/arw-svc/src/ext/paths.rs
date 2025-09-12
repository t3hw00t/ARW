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
pub(crate) fn downloads_metrics_path() -> PathBuf {
    state_dir().join("downloads.metrics.json")
}
// --- Self model paths ---
pub(crate) fn self_dir() -> PathBuf {
    state_dir().join("self")
}

// Sanitize a user-provided agent id/name to a safe filename stem
pub(crate) fn sanitize_agent_id(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return None;
    }
    // Allow letters, numbers, space, dash, underscore and dot
    let ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' || c == '.');
    if !ok {
        return None;
    }
    if trimmed.starts_with('.') {
        return None;
    }
    Some(trimmed.to_string())
}

pub(crate) fn self_model_path(agent: &str) -> Option<PathBuf> {
    sanitize_agent_id(agent).map(|s| self_dir().join(format!("{}.json", s)))
}
pub(crate) fn self_proposals_dir() -> PathBuf {
    self_dir().join("_proposals")
}
pub(crate) fn orch_path() -> PathBuf {
    state_dir().join("orchestration.json")
}
pub(crate) fn feedback_path() -> PathBuf {
    state_dir().join("feedback.json")
}
pub(crate) fn config_path() -> PathBuf {
    state_dir().join("config.json")
}
pub(crate) fn snapshots_dir() -> PathBuf {
    state_dir().join("snapshots")
}
pub(crate) fn audit_path() -> PathBuf {
    state_dir().join("audit.log")
}

// --- Egress & Review paths ---
pub(crate) fn egress_ledger_path() -> PathBuf {
    state_dir().join("egress.jsonl")
}
pub(crate) fn memory_quarantine_path() -> PathBuf {
    state_dir().join("memory.quarantine.json")
}
pub(crate) fn world_diffs_review_path() -> PathBuf {
    state_dir().join("world.diffs.review.json")
}

// --- World model paths ---
pub(crate) fn world_dir() -> PathBuf {
    state_dir().join("world")
}
pub(crate) fn world_path() -> PathBuf {
    world_dir().join("world.json")
}
pub(crate) fn world_versions_dir() -> PathBuf {
    world_dir().join("versions")
}

// --- Projects paths ---
pub(crate) fn projects_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ARW_PROJECTS_DIR") {
        if !p.trim().is_empty() {
            return PathBuf::from(p.replace('\\', "/"));
        }
    }
    state_dir().join("projects")
}

// Sanitize a user-provided project name to a safe directory name
pub(crate) fn sanitize_project_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 120 {
        return None;
    }
    // Allow letters, numbers, space, dash, underscore and dot
    let ok = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' || c == '.');
    if !ok {
        return None;
    }
    // Avoid names that could be traversal-like even if allowed chars
    if trimmed.starts_with('.') {
        return None;
    }
    Some(trimmed.to_string())
}

pub(crate) fn project_root(name: &str) -> Option<PathBuf> {
    let safe = sanitize_project_name(name)?;
    Some(projects_dir().join(safe))
}

pub(crate) fn project_notes_path(name: &str) -> Option<PathBuf> {
    project_root(name).map(|p| p.join("NOTES.md"))
}
