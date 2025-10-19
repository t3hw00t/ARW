use anyhow::{anyhow, bail, Context, Result};
use jsonschema::{Draft, Validator};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

static RECIPE_SCHEMA: Lazy<Validator> = Lazy::new(|| {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/schemas/recipe_manifest.json"
    ));
    let schema_json: Value = serde_json::from_str(raw).expect("recipe schema json to parse");
    jsonschema::options()
        .with_draft(Draft::Draft7)
        .build(&schema_json)
        .expect("recipe schema to compile")
});

/// Candidate manifest names searched when loading a recipe package directory.
pub const MANIFEST_CANDIDATES: &[&str] = &[
    "manifest.yaml",
    "manifest.yml",
    "recipe.yaml",
    "recipe.yml",
    "manifest.json",
    "recipe.json",
];

/// Public summary metadata for an installed recipe.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecipeSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub manifest_path: String,
}

/// Source type for an installed recipe manifest.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecipeSourceKind {
    File,
    Directory,
}

/// Loaded recipe manifest with convenience helpers and metadata.
#[derive(Debug, Clone)]
pub struct Recipe {
    summary: RecipeSummary,
    manifest: Value,
    manifest_path: PathBuf,
    source_root: PathBuf,
    kind: RecipeSourceKind,
}

impl Recipe {
    /// Load a recipe manifest from a file or directory.
    pub fn load(source: &Path) -> Result<Self> {
        let meta = fs::metadata(source)
            .with_context(|| format!("failed to access {}", source.display()))?;
        if meta.is_dir() {
            Self::load_from_dir(source)
        } else if meta.is_file() {
            Self::load_from_file(source)
        } else {
            bail!("{} is neither file nor directory", source.display());
        }
    }

    /// Return the manifest summary.
    pub fn summary(&self) -> &RecipeSummary {
        &self.summary
    }

    /// Return the full manifest JSON value.
    pub fn manifest(&self) -> &Value {
        &self.manifest
    }

    /// Canonical path to the manifest file.
    pub fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    /// Canonical root directory of the recipe.
    pub fn source_root(&self) -> &Path {
        &self.source_root
    }

    /// Whether the recipe was loaded from a single file or directory package.
    pub fn kind(&self) -> RecipeSourceKind {
        self.kind
    }

    /// Extract tool ids referenced by the manifest.
    pub fn tool_ids(&self) -> Vec<String> {
        self.manifest
            .get("tools")
            .and_then(|tools| tools.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| {
                        entry
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|id| id.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count workflow steps declared in the manifest.
    pub fn workflow_steps(&self) -> usize {
        self.manifest
            .get("workflows")
            .and_then(|wf| wf.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0)
    }

    /// Generate a human-readable summary of permission modes.
    pub fn permission_modes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(perms) = self.manifest.get("permissions").and_then(|p| p.as_object()) {
            for (key, value) in perms {
                let mode = match value {
                    Value::String(s) => s.to_string(),
                    Value::Object(map) => map
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unknown>")
                        .to_string(),
                    _ => "<unknown>".to_string(),
                };
                let ttl = match value {
                    Value::Object(map) => map
                        .get("ttl_secs")
                        .and_then(|v| v.as_i64())
                        .map(|v| format!("@{}s", v)),
                    _ => None,
                };
                if let Some(ttl) = ttl {
                    out.push(format!("{}={}{}", key, mode, ttl));
                } else {
                    out.push(format!("{}={}", key, mode));
                }
            }
            out.sort();
        }
        out
    }

    fn load_from_file(path: &Path) -> Result<Self> {
        let canonical = path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", path.display()))?;
        let manifest = load_manifest_from_file(&canonical)?;
        Self::from_manifest(
            manifest,
            canonical.clone(),
            canonical,
            RecipeSourceKind::File,
        )
    }

    fn load_from_dir(dir: &Path) -> Result<Self> {
        let root = dir
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", dir.display()))?;
        // Prefer common manifest names first.
        for candidate in MANIFEST_CANDIDATES {
            let candidate_path = root.join(candidate);
            if candidate_path.is_file() {
                if let Ok(manifest) = load_manifest_from_file(&candidate_path) {
                    return match Self::from_manifest(
                        manifest,
                        candidate_path.clone(),
                        root.clone(),
                        RecipeSourceKind::Directory,
                    ) {
                        Ok(recipe) => Ok(recipe),
                        Err(err) => Err(err.context(format!(
                            "manifest candidate {} invalid",
                            candidate_path.display()
                        ))),
                    };
                }
            }
        }

        // Fallback: scan for any YAML/JSON that validates.
        let mut last_err: Option<anyhow::Error> = None;
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && looks_like_manifest_candidate(&path) {
                entries.push(path);
            }
        }
        for path in entries {
            match load_manifest_from_file(&path) {
                Ok(manifest) => {
                    return match Self::from_manifest(
                        manifest,
                        path.clone(),
                        root.clone(),
                        RecipeSourceKind::Directory,
                    ) {
                        Ok(recipe) => Ok(recipe),
                        Err(err) => {
                            Err(err
                                .context(format!("manifest candidate {} invalid", path.display())))
                        }
                    };
                }
                Err(err) => {
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            anyhow!(
                "no manifest matching {:?} found under {}",
                MANIFEST_CANDIDATES,
                root.display()
            )
        }))
    }

    fn from_manifest(
        manifest: Value,
        manifest_path: PathBuf,
        source_root: PathBuf,
        kind: RecipeSourceKind,
    ) -> Result<Self> {
        let summary = build_summary(&manifest, &manifest_path);
        validate_manifest(&manifest).with_context(|| {
            format!(
                "manifest {} failed additional validation",
                manifest_path.display()
            )
        })?;
        Ok(Self {
            summary,
            manifest,
            manifest_path,
            source_root,
            kind,
        })
    }
}

/// Check whether a path could hold a recipe manifest.
pub fn looks_like_manifest_candidate(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "yaml" | "yml" | "json"),
        None => false,
    }
}

fn load_manifest_from_file(path: &Path) -> Result<Value> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&data)
        .with_context(|| format!("failed to parse manifest {}", path.display()))?;
    let schema_errors: Vec<_> = RECIPE_SCHEMA
        .iter_errors(&value)
        .map(|err| format!("{}: {}", err.instance_path, err))
        .collect();
    if !schema_errors.is_empty() {
        let joined = schema_errors.join("; ");
        bail!("{} does not satisfy schema: {}", path.display(), joined);
    }
    Ok(value)
}

fn validate_manifest(manifest: &Value) -> Result<()> {
    let tools = manifest
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("tools array missing after schema validation"))?;
    let mut tool_ids = HashSet::new();
    for entry in tools {
        if let Some(id) = entry.get("id").and_then(|v| v.as_str()) {
            tool_ids.insert(id.to_string());
        }
    }

    if let Some(workflows) = manifest.get("workflows").and_then(|v| v.as_array()) {
        let mut seen_steps: HashSet<String> = HashSet::new();
        for (idx, wf) in workflows.iter().enumerate() {
            let step = wf
                .get("step")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            if step.is_empty() {
                bail!("workflows[{idx}] step is empty after schema validation");
            }
            if !seen_steps.insert(step.clone()) {
                bail!("duplicate workflow step `{step}` detected");
            }
            let tool = wf
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if tool.is_empty() {
                bail!("workflow step `{step}` has empty tool reference");
            }
            if !tool_ids.contains(&tool) {
                bail!("workflow step `{step}` references undeclared tool `{tool}`");
            }
        }
    }

    Ok(())
}

fn build_summary(manifest: &Value, manifest_path: &Path) -> RecipeSummary {
    let id = manifest
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let name = manifest
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let preferred_model = manifest
        .get("model")
        .and_then(|m| m.get("preferred"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let fallback_model = manifest
        .get("model")
        .and_then(|m| m.get("fallback"))
        .and_then(|v| match v {
            Value::Null => None,
            _ => v.as_str(),
        })
        .map(|s| s.to_string());
    let tags = manifest
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    RecipeSummary {
        id,
        name,
        version,
        preferred_model,
        fallback_model,
        tags,
        manifest_path: manifest_path.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_manifest() -> &'static str {
        r#"id: sample-recipe
name: Sample Recipe
version: "1.0.0"
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Do sample things"
tools:
  - id: sample_tool
    params: {}
workflows:
  - step: "do"
    tool: sample_tool
"#
    }

    #[test]
    fn load_recipe_from_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("recipe.yaml");
        fs::write(&path, sample_manifest()).unwrap();
        let recipe = Recipe::load(&path).unwrap();
        assert_eq!(recipe.summary().id, "sample-recipe");
        assert_eq!(recipe.summary().name, "Sample Recipe");
        assert_eq!(recipe.tool_ids(), vec!["sample_tool"]);
        assert_eq!(recipe.workflow_steps(), 1);
    }

    #[test]
    fn load_recipe_from_directory() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("pkg");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.yaml"), sample_manifest()).unwrap();
        let recipe = Recipe::load(&dir).unwrap();
        assert_eq!(recipe.summary().id, "sample-recipe");
        assert_eq!(recipe.kind(), RecipeSourceKind::Directory);
        assert_eq!(recipe.source_root(), dir.canonicalize().unwrap().as_path());
    }

    #[test]
    fn reject_recipe_with_invalid_version() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("recipe.yaml");
        fs::write(
            &path,
            r#"id: invalid-version
name: Invalid Version
version: not-a-semver
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Test invalid version"
tools:
  - id: sample_tool
"#,
        )
        .unwrap();
        match Recipe::load(&path) {
            Ok(_) => panic!("expected schema validation to fail"),
            Err(err) => assert!(
                err.to_string().contains("does not satisfy schema"),
                "unexpected error: {err:?}"
            ),
        }
    }

    #[test]
    fn reject_recipe_with_duplicate_tools() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("recipe.yaml");
        fs::write(
            &path,
            r#"id: duplicate-tools
name: Duplicate Tools
version: 1.2.3
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Test duplicate tools"
tools:
  - id: sample_tool
workflows:
  - step: prepare
    tool: sample_tool
  - step: prepare
    tool: sample_tool
"#,
        )
        .unwrap();
        match Recipe::load(&path) {
            Ok(_) => panic!("expected validation to fail"),
            Err(err) => {
                let msg = err.to_string();
                let ok =
                    msg.contains("duplicate workflow step") || msg.contains("non-unique elements");
                assert!(ok, "unexpected error: {msg:?}");
            }
        }
    }
}
