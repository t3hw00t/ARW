use std::collections::HashSet;
use std::fmt;
use std::fs;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_yaml::Value;

#[derive(Debug, Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    cache: CacheSection,
}

#[derive(Debug, Default, Deserialize)]
struct CacheSection {
    #[serde(default)]
    action_cache: Option<ActionCacheSection>,
    #[serde(default)]
    read_models: Option<ReadModelsSection>,
}

#[derive(Debug, Default, Deserialize)]
struct ActionCacheSection {
    #[serde(default)]
    ttl: Option<Value>,
    #[serde(default)]
    ttl_secs: Option<u64>,
    #[serde(default)]
    capacity: Option<u64>,
    #[serde(default)]
    cap: Option<u64>,
    #[serde(default)]
    allow: Option<Vec<String>>,
    #[serde(default)]
    deny: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct ReadModelsSection {
    #[serde(default)]
    sse: Option<ReadModelSseSection>,
}

#[derive(Debug, Default, Deserialize)]
struct ReadModelSseSection {
    #[serde(default)]
    coalesce_ms: Option<u64>,
    #[serde(default)]
    idle_publish_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct CachePolicyOutcome {
    pub assignments: Vec<EnvAssignment>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EnvAssignment {
    pub key: &'static str,
    pub value: String,
    pub source: String,
    pub applied: bool,
    pub reason: Option<AssignmentReason>,
    pub existing: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentReason {
    AlreadySetSameValue,
    EnvOverride,
}

impl AssignmentReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssignmentReason::AlreadySetSameValue => "already_set_same_value",
            AssignmentReason::EnvOverride => "env_override",
        }
    }
}

impl fmt::Display for AssignmentReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn apply_manifest(path: &str) -> Result<CachePolicyOutcome> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("reading cache policy manifest at {path}"))?;
    let manifest: Manifest = serde_yaml::from_str(&contents)
        .with_context(|| format!("parsing cache policy manifest at {path}"))?;
    Ok(apply_manifest_inner(manifest))
}

fn apply_manifest_inner(manifest: Manifest) -> CachePolicyOutcome {
    let mut outcome = CachePolicyOutcome::default();

    if let Some(action) = manifest.cache.action_cache {
        if let Some(ttl) = action.ttl_secs {
            apply_env(
                &mut outcome,
                "ARW_TOOLS_CACHE_TTL_SECS",
                ttl.to_string(),
                "cache.action_cache.ttl_secs",
            );
        } else if let Some(raw) = action.ttl.as_ref() {
            match parse_duration_value(raw) {
                Some(ttl) => apply_env(
                    &mut outcome,
                    "ARW_TOOLS_CACHE_TTL_SECS",
                    ttl.to_string(),
                    "cache.action_cache.ttl",
                ),
                None => outcome.warnings.push(format!(
                    "failed to parse cache.action_cache.ttl value: {raw:?}"
                )),
            }
        }

        if let Some(capacity) = action.capacity.or(action.cap) {
            apply_env(
                &mut outcome,
                "ARW_TOOLS_CACHE_CAP",
                capacity.to_string(),
                "cache.action_cache.capacity",
            );
        }

        if let Some(list) = action.allow.as_ref().and_then(|vals| join_list(vals)) {
            apply_env(
                &mut outcome,
                "ARW_TOOLS_CACHE_ALLOW",
                list,
                "cache.action_cache.allow",
            );
        }

        if let Some(list) = action.deny.as_ref().and_then(|vals| join_list(vals)) {
            apply_env(
                &mut outcome,
                "ARW_TOOLS_CACHE_DENY",
                list,
                "cache.action_cache.deny",
            );
        }
    }

    if let Some(read_models) = manifest.cache.read_models {
        if let Some(sse) = read_models.sse {
            if let Some(coalesce) = sse.coalesce_ms {
                let value = coalesce.to_string();
                apply_env(
                    &mut outcome,
                    "ARW_ROUTE_STATS_COALESCE_MS",
                    value.clone(),
                    "cache.read_models.sse.coalesce_ms",
                );
                apply_env(
                    &mut outcome,
                    "ARW_MODELS_METRICS_COALESCE_MS",
                    value,
                    "cache.read_models.sse.coalesce_ms",
                );
            }

            if let Some(idle) = sse.idle_publish_ms {
                let value = idle.to_string();
                apply_env(
                    &mut outcome,
                    "ARW_ROUTE_STATS_PUBLISH_MS",
                    value.clone(),
                    "cache.read_models.sse.idle_publish_ms",
                );
                apply_env(
                    &mut outcome,
                    "ARW_MODELS_METRICS_PUBLISH_MS",
                    value,
                    "cache.read_models.sse.idle_publish_ms",
                );
            }
        }
    }

    outcome
}

fn apply_env(
    outcome: &mut CachePolicyOutcome,
    key: &'static str,
    value: String,
    source: impl Into<String>,
) {
    let source = source.into();
    let existing = std::env::var(key).ok();
    let trimmed_existing = existing
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let mut applied = false;
    let mut reason = None;

    match trimmed_existing.as_deref() {
        None => {
            std::env::set_var(key, &value);
            applied = true;
        }
        Some(existing) if existing == value.as_str() => {
            reason = Some(AssignmentReason::AlreadySetSameValue);
        }
        Some(_) => {
            reason = Some(AssignmentReason::EnvOverride);
        }
    }

    outcome.assignments.push(EnvAssignment {
        key,
        value,
        source,
        applied,
        reason,
        existing,
    });
}

fn join_list(values: &[String]) -> Option<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.to_string();
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(","))
    }
}

fn parse_duration_value(raw: &Value) -> Option<u64> {
    match raw {
        Value::Number(num) => {
            if let Some(u) = num.as_u64() {
                Some(u)
            } else if let Some(i) = num.as_i64() {
                (i >= 0).then(|| i as u64)
            } else {
                num.as_f64()
                    .and_then(|f| (f >= 0.0).then(|| f.round() as u64))
            }
        }
        Value::String(s) => parse_duration_str(s),
        _ => None,
    }
}

fn parse_duration_str(raw: &str) -> Option<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(secs);
    }

    let mut split_idx = None;
    for (idx, ch) in trimmed.char_indices() {
        if !(ch.is_ascii_digit() || ch == '.') {
            split_idx = Some(idx);
            break;
        }
    }

    let split_idx = split_idx?;
    let (number, unit) = trimmed.split_at(split_idx);
    let value: f64 = number.parse().ok()?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        "ms" => 0.001,
        _ => return None,
    };
    let secs = value * multiplier;
    if secs < 0.0 {
        return None;
    }
    Some(secs.round().max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    const ENV_KEYS: &[&str] = &[
        "ARW_TOOLS_CACHE_TTL_SECS",
        "ARW_TOOLS_CACHE_CAP",
        "ARW_TOOLS_CACHE_ALLOW",
        "ARW_TOOLS_CACHE_DENY",
        "ARW_ROUTE_STATS_COALESCE_MS",
        "ARW_ROUTE_STATS_PUBLISH_MS",
        "ARW_MODELS_METRICS_COALESCE_MS",
        "ARW_MODELS_METRICS_PUBLISH_MS",
    ];

    fn clear_env() {
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
    }

    fn run_manifest(yaml: &str) -> CachePolicyOutcome {
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        apply_manifest_inner(manifest)
    }

    #[test]
    #[serial]
    fn applies_manifest_defaults_when_unset() {
        clear_env();
        let outcome = run_manifest(
            r#"
cache:
  action_cache:
    ttl: 7d
    capacity: 4096
    allow: ["demo.echo", "demo.echo", " http.fetch "]
    deny: ["fs.patch", ""]
  read_models:
    sse:
      coalesce_ms: 250
      idle_publish_ms: 2000
"#,
        );

        assert_eq!(std::env::var("ARW_TOOLS_CACHE_TTL_SECS").unwrap(), "604800");
        assert_eq!(std::env::var("ARW_TOOLS_CACHE_CAP").unwrap(), "4096");
        assert_eq!(
            std::env::var("ARW_TOOLS_CACHE_ALLOW").unwrap(),
            "demo.echo,http.fetch"
        );
        assert_eq!(std::env::var("ARW_TOOLS_CACHE_DENY").unwrap(), "fs.patch");
        assert_eq!(std::env::var("ARW_ROUTE_STATS_COALESCE_MS").unwrap(), "250");
        assert_eq!(
            std::env::var("ARW_MODELS_METRICS_COALESCE_MS").unwrap(),
            "250"
        );
        assert_eq!(std::env::var("ARW_ROUTE_STATS_PUBLISH_MS").unwrap(), "2000");
        assert_eq!(
            std::env::var("ARW_MODELS_METRICS_PUBLISH_MS").unwrap(),
            "2000"
        );

        let ttl_assignment = outcome
            .assignments
            .iter()
            .find(|a| a.key == "ARW_TOOLS_CACHE_TTL_SECS")
            .unwrap();
        assert!(ttl_assignment.applied);
        assert_eq!(ttl_assignment.reason, None);
        assert_eq!(ttl_assignment.source, "cache.action_cache.ttl");
        clear_env();
    }

    #[test]
    #[serial]
    fn respects_existing_env_overrides() {
        clear_env();
        std::env::set_var("ARW_TOOLS_CACHE_TTL_SECS", "120");
        std::env::set_var("ARW_ROUTE_STATS_COALESCE_MS", "999");

        let outcome = run_manifest(
            r#"
cache:
  action_cache:
    ttl: 15m
  read_models:
    sse:
      coalesce_ms: 250
"#,
        );

        assert_eq!(std::env::var("ARW_TOOLS_CACHE_TTL_SECS").unwrap(), "120");
        assert_eq!(std::env::var("ARW_ROUTE_STATS_COALESCE_MS").unwrap(), "999");
        assert_eq!(
            std::env::var("ARW_MODELS_METRICS_COALESCE_MS").unwrap(),
            "250"
        );

        let ttl_assignment = outcome
            .assignments
            .iter()
            .find(|a| a.key == "ARW_TOOLS_CACHE_TTL_SECS")
            .unwrap();
        assert!(!ttl_assignment.applied);
        assert_eq!(ttl_assignment.reason, Some(AssignmentReason::EnvOverride));
        assert_eq!(ttl_assignment.existing.as_deref(), Some("120"));

        let coalesce_assignment = outcome
            .assignments
            .iter()
            .find(|a| a.key == "ARW_ROUTE_STATS_COALESCE_MS")
            .unwrap();
        assert!(!coalesce_assignment.applied);
        assert_eq!(
            coalesce_assignment.reason,
            Some(AssignmentReason::EnvOverride)
        );
        clear_env();
    }

    #[test]
    #[serial]
    fn records_warning_on_invalid_duration() {
        clear_env();
        let outcome = run_manifest(
            r#"
cache:
  action_cache:
    ttl: later
"#,
        );

        assert!(outcome
            .warnings
            .iter()
            .any(|msg| msg.contains("cache.action_cache.ttl")));
        assert!(std::env::var("ARW_TOOLS_CACHE_TTL_SECS").is_err());
        clear_env();
    }
}
