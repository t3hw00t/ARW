use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
use tracing::{info, warn};

use arw_core::cache_policy::{AssignmentReason, CachePolicyOutcome};

static EFFECTIVE_PATHS: Lazy<Mutex<Option<arw_core::EffectivePaths>>> =
    Lazy::new(|| Mutex::new(None));

pub struct InitialConfigState {
    pub value: Value,
    pub history: Vec<(String, Value)>,
    pub source: Option<String>,
}

impl Default for InitialConfigState {
    fn default() -> Self {
        Self {
            value: json!({}),
            history: Vec::new(),
            source: None,
        }
    }
}

pub fn apply_effective_paths() -> arw_core::EffectivePaths {
    let paths = arw_core::effective_paths();
    info!(
        state_dir = %paths.state_dir,
        cache_dir = %paths.cache_dir,
        logs_dir = %paths.logs_dir,
        "resolved effective runtime paths"
    );
    set_env_for_paths(&paths);
    *EFFECTIVE_PATHS.lock().unwrap() = Some(paths.clone());
    paths
}

pub fn effective_paths() -> Option<arw_core::EffectivePaths> {
    EFFECTIVE_PATHS.lock().unwrap().clone()
}

#[cfg(test)]
pub fn reset_effective_paths_for_tests() {
    *EFFECTIVE_PATHS.lock().unwrap() = None;
}

fn set_env_for_paths(paths: &arw_core::EffectivePaths) {
    std::env::set_var("ARW_STATE_DIR", &paths.state_dir);
    std::env::set_var("ARW_CACHE_DIR", &paths.cache_dir);
    std::env::set_var("ARW_LOGS_DIR", &paths.logs_dir);
}

pub fn load_initial_config_state() -> InitialConfigState {
    let mut state = InitialConfigState::default();
    let (path_opt, source) = discovered_config_path();

    if let Some(path) = path_opt {
        match arw_core::load_config(path.to_string_lossy().as_ref()) {
            Ok(cfg) => {
                let value = serde_json::to_value(&cfg).unwrap_or_else(|_| json!({}));
                info!(path = %path.display(), source, "loaded runtime config");
                state.source = Some(path.to_string_lossy().to_string());
                state
                    .history
                    .push((format!("bootstrap:{source}"), value.clone()));
                state.value = value;
            }
            Err(err) => {
                warn!(path = %path.display(), source, "failed to load runtime config: {err}")
            }
        }
    }

    state
}

fn discovered_config_path() -> (Option<PathBuf>, &'static str) {
    if let Ok(explicit) = std::env::var("ARW_CONFIG") {
        if !explicit.trim().is_empty() {
            return (Some(PathBuf::from(explicit)), "env");
        }
    }
    (
        arw_core::resolve_config_path("configs/default.toml"),
        "search",
    )
}

pub fn init_gating_from_configs() {
    let (path_opt, source) = discovered_gating_path();
    let loader_path = path_opt
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "configs/gating.toml".to_string());

    arw_core::gating::init_from_config(&loader_path);

    match path_opt {
        Some(path) if path.exists() => {
            info!(path = %path.display(), source, "loaded gating policy")
        }
        Some(path) => {
            warn!(path = %path.display(), source, "gating policy file missing; env overrides only")
        }
        None => info!("no gating policy discovered; relying on env overrides"),
    }
}

fn discovered_gating_path() -> (Option<PathBuf>, &'static str) {
    if let Ok(explicit) = std::env::var("ARW_GATING_FILE") {
        if !explicit.trim().is_empty() {
            return (Some(PathBuf::from(explicit)), "env");
        }
    }
    (
        arw_core::resolve_config_path("configs/gating.toml"),
        "search",
    )
}

pub fn init_cache_policy_from_manifest() {
    let (path_opt, source) = discovered_cache_policy_path();
    match path_opt {
        Some(path) if path.exists() => {
            let path_string = path.to_string_lossy().to_string();
            match arw_core::cache_policy::apply_manifest(&path_string) {
                Ok(outcome) => emit_cache_policy_logs(path.as_path(), source, outcome),
                Err(err) => {
                    warn!(path = %path.display(), source, "failed to load cache policy manifest: {err}")
                }
            }
        }
        Some(path) => {
            warn!(path = %path.display(), source, "cache policy manifest path missing; env defaults only")
        }
        None => info!("no cache policy manifest discovered; relying on env defaults"),
    }
}

fn discovered_cache_policy_path() -> (Option<PathBuf>, &'static str) {
    if let Ok(explicit) = std::env::var("ARW_CACHE_POLICY_FILE") {
        if !explicit.trim().is_empty() {
            return (Some(PathBuf::from(explicit)), "env");
        }
    }

    if let Some(path) = arw_core::resolve_config_path("configs/cache_policy.yaml") {
        return (Some(path), "search");
    }

    (None, "search")
}

fn emit_cache_policy_logs(path: &Path, source: &'static str, outcome: CachePolicyOutcome) {
    let applied: Vec<String> = outcome
        .assignments
        .iter()
        .filter(|a| a.applied)
        .map(|a| format!("{}={}", a.key, a.value))
        .collect();

    if applied.is_empty() {
        info!(path = %path.display(), source, "cache policy manifest loaded (no env changes)");
    } else {
        info!(path = %path.display(), source, applied = %applied.join(","), "cache policy manifest applied");
    }

    let overridden: Vec<&str> = outcome
        .assignments
        .iter()
        .filter(|a| matches!(a.reason, Some(AssignmentReason::EnvOverride)))
        .map(|a| a.key)
        .collect();

    if !overridden.is_empty() {
        info!(path = %path.display(), source, overrides = ?overridden, "environment overrides take precedence over cache policy entries");
    }

    let reuse_matches = outcome
        .assignments
        .iter()
        .filter(|a| matches!(a.reason, Some(AssignmentReason::AlreadySetSameValue)))
        .map(|a| a.key)
        .collect::<Vec<_>>();

    if !reuse_matches.is_empty() {
        info!(path = %path.display(), source, retained = ?reuse_matches, "cache policy entries already satisfied by existing env values");
    }

    for warning in outcome.warnings {
        warn!(path = %path.display(), source, warning = %warning, "cache policy manifest warning");
    }
}

pub fn kernel_enabled_from_env() -> bool {
    std::env::var("ARW_KERNEL_ENABLE")
        .map(|v| {
            let trimmed = v.trim();
            !(trimmed.eq_ignore_ascii_case("0") || trimmed.eq_ignore_ascii_case("false"))
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_effective_paths, init_cache_policy_from_manifest, kernel_enabled_from_env,
        load_initial_config_state, reset_effective_paths_for_tests,
    };
    use crate::test_support::env as test_env;
    use std::{env, fs};
    use tempfile::tempdir;

    #[test]
    fn default_true() {
        let mut guard = test_env::guard();
        guard.remove("ARW_KERNEL_ENABLE");
        assert!(kernel_enabled_from_env());
    }

    #[test]
    fn disabled_values() {
        let mut guard = test_env::guard();
        for value in ["0", "false", "False", "FALSE", " 0 ", " false "] {
            guard.set("ARW_KERNEL_ENABLE", value);
            assert!(!kernel_enabled_from_env(), "value {value:?}");
        }
    }

    #[test]
    fn enabled_values() {
        let mut guard = test_env::guard();
        for value in ["1", "true", "YES"] {
            guard.set("ARW_KERNEL_ENABLE", value);
            assert!(kernel_enabled_from_env(), "value {value:?}");
        }
    }

    #[test]
    fn apply_effective_paths_sets_env() {
        let mut guard = test_env::guard();
        reset_effective_paths_for_tests();
        guard.remove("ARW_CONFIG");
        guard.remove("ARW_STATE_DIR");
        guard.remove("ARW_CACHE_DIR");
        guard.remove("ARW_LOGS_DIR");

        let tmp = tempdir().unwrap();
        let cfg = tmp.path().join("bootstrap.toml");
        fs::write(
            &cfg,
            r#"
                [runtime]
                state_dir = "./tmp_state"
                cache_dir = "./tmp_cache"
                logs_dir  = "./tmp_logs"
            "#,
        )
        .unwrap();
        let cfg_str = cfg.to_string_lossy();
        guard.set("ARW_CONFIG", cfg_str.as_ref());

        let paths = apply_effective_paths();
        assert_eq!(paths.state_dir, "./tmp_state".replace('\\', "/"));
        assert_eq!(env::var("ARW_STATE_DIR").unwrap(), paths.state_dir);
        assert_eq!(env::var("ARW_CACHE_DIR").unwrap(), paths.cache_dir);
        assert_eq!(env::var("ARW_LOGS_DIR").unwrap(), paths.logs_dir);

        reset_effective_paths_for_tests();
    }

    #[test]
    fn load_initial_config_state_reads_file() {
        let mut guard = test_env::guard();
        guard.remove("ARW_CONFIG");

        let tmp = tempdir().unwrap();
        let cfg = tmp.path().join("bootstrap.toml");
        fs::write(
            &cfg,
            r#"
                [runtime]
                portable = true
            "#,
        )
        .unwrap();
        let cfg_str = cfg.to_string_lossy();
        guard.set("ARW_CONFIG", cfg_str.as_ref());

        let initial = load_initial_config_state();
        assert_eq!(
            initial.source.as_deref(),
            Some(cfg.to_string_lossy().as_ref())
        );
        assert!(initial.value.get("runtime").is_some());
        assert_eq!(initial.history.len(), 1);
    }

    #[test]
    fn init_cache_policy_from_manifest_sets_env() {
        let mut guard = test_env::guard();
        guard.remove("ARW_CACHE_POLICY_FILE");
        guard.remove("ARW_TOOLS_CACHE_TTL_SECS");
        guard.remove("ARW_ROUTE_STATS_COALESCE_MS");
        guard.remove("ARW_MODELS_METRICS_COALESCE_MS");

        let tmp = tempdir().unwrap();
        let manifest = tmp.path().join("cache_policy.yaml");
        fs::write(
            &manifest,
            r#"
cache:
  action_cache:
    ttl: 15m
  read_models:
    sse:
      coalesce_ms: 300
"#,
        )
        .unwrap();

        let manifest_str = manifest.to_string_lossy();
        guard.set("ARW_CACHE_POLICY_FILE", manifest_str.as_ref());

        init_cache_policy_from_manifest();

        assert_eq!(env::var("ARW_TOOLS_CACHE_TTL_SECS").unwrap(), "900");
        assert_eq!(env::var("ARW_ROUTE_STATS_COALESCE_MS").unwrap(), "300");
        assert_eq!(env::var("ARW_MODELS_METRICS_COALESCE_MS").unwrap(), "300");
    }
}
