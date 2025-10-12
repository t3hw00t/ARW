use anyhow::{anyhow, bail, Context, Result};
use arw_core::effective_paths;
use clap::{Args, Subcommand};
use jsonschema::{Draft, JSONSchema};
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

static RECIPE_SCHEMA: Lazy<JSONSchema> = Lazy::new(|| {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/schemas/recipe_manifest.json"
    ));
    let schema_json: Value = serde_json::from_str(raw).expect("recipe schema json to parse");
    JSONSchema::options()
        .with_draft(Draft::Draft7)
        .compile(&schema_json)
        .expect("recipe schema to compile")
});

const MANIFEST_CANDIDATES: &[&str] = &[
    "manifest.yaml",
    "manifest.yml",
    "recipe.yaml",
    "recipe.yml",
    "manifest.json",
    "recipe.json",
];

#[derive(Subcommand)]
pub enum RecipesCmd {
    /// List installed recipes in the current state directory
    List(RecipesListArgs),
    /// Inspect a recipe manifest (file or folder) and print details
    Inspect(RecipesInspectArgs),
    /// Install a recipe into the state directory, validating the manifest first
    Install(RecipesInstallArgs),
}

#[derive(Args)]
pub struct RecipesListArgs {
    /// Output as JSON instead of a table
    #[arg(long)]
    pub json: bool,
    /// Override the state directory (defaults to detected ARW state dir)
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
}

#[derive(Args)]
pub struct RecipesInspectArgs {
    /// Path to a recipe manifest file or folder
    pub source: PathBuf,
    /// Output as JSON (summary + manifest)
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct RecipesInstallArgs {
    /// Path to a recipe manifest file or folder
    pub source: PathBuf,
    /// Override the destination state directory
    #[arg(long)]
    pub state_dir: Option<PathBuf>,
    /// Overwrite an existing recipe with the same id
    #[arg(long)]
    pub force: bool,
    /// Override the destination recipe id (defaults to manifest id)
    #[arg(long)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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

struct Recipe {
    summary: RecipeSummary,
    manifest: Value,
    manifest_path: PathBuf,
    source_root: PathBuf,
    kind: RecipeSourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecipeSourceKind {
    File,
    Directory,
}

impl Recipe {
    fn load(source: &Path) -> Result<Self> {
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

    fn tool_ids(&self) -> Vec<String> {
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

    fn workflow_steps(&self) -> usize {
        self.manifest
            .get("workflows")
            .and_then(|wf| wf.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0)
    }

    fn permission_modes(&self) -> Vec<String> {
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
}

fn load_manifest_from_file(path: &Path) -> Result<Value> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&data)
        .with_context(|| format!("failed to parse manifest {}", path.display()))?;
    if let Err(errors) = RECIPE_SCHEMA.validate(&value) {
        let joined = errors
            .map(|err| format!("{}: {}", err.instance_path, err))
            .collect::<Vec<_>>()
            .join("; ");
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

fn looks_like_manifest_candidate(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "yaml" | "yml" | "json"),
        None => false,
    }
}

fn detect_state_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir);
    }
    let paths = effective_paths();
    Ok(PathBuf::from(paths.state_dir))
}

pub fn run(cmd: RecipesCmd) -> Result<()> {
    match cmd {
        RecipesCmd::List(args) => list(args),
        RecipesCmd::Inspect(args) => inspect(args),
        RecipesCmd::Install(args) => install(args),
    }
}

fn list(args: RecipesListArgs) -> Result<()> {
    let state_dir = detect_state_dir(args.state_dir)?;
    let recipes_dir = state_dir.join("recipes");
    if !recipes_dir.exists() {
        if args.json {
            println!("[]");
        } else {
            println!("(no recipes found in {})", recipes_dir.to_string_lossy());
        }
        return Ok(());
    }

    let mut recipes: Vec<Recipe> = Vec::new();
    for entry in fs::read_dir(&recipes_dir)
        .with_context(|| format!("failed to read {}", recipes_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() || (meta.is_file() && looks_like_manifest_candidate(&path)) {
            match Recipe::load(&path) {
                Ok(recipe) => recipes.push(recipe),
                Err(err) => {
                    eprintln!("skipping {}: {}", path.display(), err);
                }
            }
        }
    }

    recipes.sort_by(|a, b| a.summary.id.cmp(&b.summary.id));
    let summaries: Vec<RecipeSummary> = recipes
        .iter()
        .map(|recipe| recipe.summary.clone())
        .collect();

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summaries).unwrap_or_else(|_| "[]".to_string())
        );
    } else if summaries.is_empty() {
        println!("(no recipes found in {})", recipes_dir.to_string_lossy());
    } else {
        print_summary_table(&summaries);
    }
    Ok(())
}

fn inspect(args: RecipesInspectArgs) -> Result<()> {
    let recipe = Recipe::load(&args.source)?;
    if args.json {
        let out = json!({
            "summary": recipe.summary,
            "manifest": recipe.manifest,
            "source_root": recipe.source_root.to_string_lossy(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("Recipe: {}", recipe.summary.id);
        println!("  Name: {}", recipe.summary.name);
        println!("  Version: {}", recipe.summary.version);
        if let Some(model) = &recipe.summary.preferred_model {
            println!("  Model (preferred): {}", model);
        }
        if let Some(model) = &recipe.summary.fallback_model {
            println!("  Model (fallback): {}", model);
        }
        if !recipe.summary.tags.is_empty() {
            println!("  Tags: {}", recipe.summary.tags.join(", "));
        }
        println!("  Manifest: {}", recipe.summary.manifest_path);
        println!("  Source: {}", recipe.source_root.display());
        let tools = recipe.tool_ids();
        if tools.is_empty() {
            println!("  Tools: none");
        } else {
            println!("  Tools ({}): {}", tools.len(), tools.join(", "));
        }
        let workflows = recipe.workflow_steps();
        println!("  Workflow steps: {}", workflows);
        let perms = recipe.permission_modes();
        if perms.is_empty() {
            println!("  Permissions: none");
        } else {
            println!("  Permissions: {}", perms.join(", "));
        }
    }
    Ok(())
}

fn install(args: RecipesInstallArgs) -> Result<()> {
    let recipe = Recipe::load(&args.source)?;
    let state_dir = detect_state_dir(args.state_dir)?;
    let recipes_dir = state_dir.join("recipes");
    fs::create_dir_all(&recipes_dir)
        .with_context(|| format!("failed to create {}", recipes_dir.display()))?;

    let dest_id = args.id.unwrap_or_else(|| recipe.summary.id.clone());
    if dest_id.is_empty() {
        bail!("manifest id is empty; use --id to supply a destination id");
    }
    let dest_dir = recipes_dir.join(&dest_id);
    if dest_dir.exists() {
        let dest_real = dest_dir
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", dest_dir.display()))?;
        let source_real = match recipe.kind {
            RecipeSourceKind::File => recipe.manifest_path.clone(),
            RecipeSourceKind::Directory => recipe.source_root.clone(),
        };
        let source_real = source_real
            .canonicalize()
            .with_context(|| "failed to canonicalize source path")?;

        if dest_real == source_real
            || dest_real.starts_with(&source_real)
            || source_real.starts_with(&dest_real)
        {
            bail!(
                "refusing to overwrite {} because it overlaps with source {}",
                dest_dir.display(),
                args.source.display()
            );
        }

        if args.force {
            if dest_dir.is_dir() {
                fs::remove_dir_all(&dest_dir)
                    .with_context(|| format!("failed to remove {}", dest_dir.display()))?;
            } else {
                fs::remove_file(&dest_dir)
                    .with_context(|| format!("failed to remove {}", dest_dir.display()))?;
            }
        } else {
            bail!(
                "{} already exists (pass --force to overwrite)",
                dest_dir.display()
            );
        }
    }

    match recipe.kind {
        RecipeSourceKind::File => {
            fs::create_dir_all(&dest_dir)
                .with_context(|| format!("failed to create {}", dest_dir.display()))?;
            let manifest_name = recipe
                .manifest_path
                .file_name()
                .map(|s| s.to_os_string())
                .unwrap_or_else(|| "manifest.yaml".into());
            let dest_path = dest_dir.join(manifest_name);
            fs::copy(&recipe.manifest_path, &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    recipe.manifest_path.display(),
                    dest_path.display()
                )
            })?;
        }
        RecipeSourceKind::Directory => {
            copy_directory(&recipe.source_root, &dest_dir)?;
        }
    }

    println!(
        "Installed recipe {} → {}",
        recipe.summary.id,
        dest_dir.display()
    );
    Ok(())
}

fn copy_directory(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(src)
            .expect("walkdir strip_prefix succeeds");
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(relative);
        if entry.file_type().is_symlink() {
            bail!(
                "found symlink {} in recipe source; refusing to copy",
                entry.path().display()
            );
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create {}", target.display()))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry.path().display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn print_summary_table(entries: &[RecipeSummary]) {
    let id_width = entries
        .iter()
        .map(|e| e.id.len())
        .chain(std::iter::once("ID".len()))
        .max()
        .unwrap_or(2);
    let name_width = entries
        .iter()
        .map(|e| e.name.len())
        .chain(std::iter::once("Name".len()))
        .max()
        .unwrap_or(4);
    let version_width = entries
        .iter()
        .map(|e| e.version.len())
        .chain(std::iter::once("Version".len()))
        .max()
        .unwrap_or(7);
    let manifest_width = entries
        .iter()
        .map(|e| e.manifest_path.len())
        .chain(std::iter::once("Manifest".len()))
        .max()
        .unwrap_or(8);

    println!(
        "{:<id_width$}  {:<name_width$}  {:<version_width$}  {:<manifest_width$}",
        "ID",
        "Name",
        "Version",
        "Manifest",
        id_width = id_width,
        name_width = name_width,
        version_width = version_width,
        manifest_width = manifest_width
    );
    println!(
        "{:-<id_width$}  {:-<name_width$}  {:-<version_width$}  {:-<manifest_width$}",
        "",
        "",
        "",
        "",
        id_width = id_width,
        name_width = name_width,
        version_width = version_width,
        manifest_width = manifest_width
    );
    for entry in entries {
        println!(
            "{:<id_width$}  {:<name_width$}  {:<version_width$}  {:<manifest_width$}",
            entry.id,
            entry.name,
            entry.version,
            entry.manifest_path,
            id_width = id_width,
            name_width = name_width,
            version_width = version_width,
            manifest_width = manifest_width
        );
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
        assert_eq!(recipe.summary.id, "sample-recipe");
        assert_eq!(recipe.summary.name, "Sample Recipe");
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
        assert_eq!(recipe.summary.id, "sample-recipe");
        assert_eq!(recipe.kind, RecipeSourceKind::Directory);
    }

    #[test]
    fn install_recipe_as_directory_copy() {
        let temp = tempdir().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("manifest.yaml"), sample_manifest()).unwrap();
        fs::write(src.join("notes.txt"), "notes").unwrap();
        let state = temp.path().join("state");
        fs::create_dir_all(&state).unwrap();

        let recipe = Recipe::load(&src).unwrap();
        install(RecipesInstallArgs {
            source: src.clone(),
            state_dir: Some(state.clone()),
            force: false,
            id: None,
        })
        .unwrap();

        let dest = state.join("recipes").join(recipe.summary.id);
        assert!(dest.join("manifest.yaml").exists());
        assert!(dest.join("notes.txt").exists());
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
    fn documentation_example_matches_canonical_recipe() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let doc_path = repo_root.join("docs/guide/recipes.md");
        let doc = fs::read_to_string(&doc_path).expect("guide to load");

        let mut in_block = false;
        let mut snippet = Vec::new();
        for line in doc.lines() {
            if line.starts_with("```") {
                if in_block {
                    break;
                } else {
                    in_block = true;
                    continue;
                }
            }
            if in_block {
                snippet.push(line);
            }
        }
        let snippet = snippet.join("\n");
        assert!(
            !snippet.trim().is_empty(),
            "expected YAML code block in {}",
            doc_path.display()
        );

        let example_path = repo_root.join("examples/recipes/paperwork-helper.yaml");
        let example_src = fs::read_to_string(&example_path).expect("example recipe to load");

        let snippet_trimmed = snippet.trim();
        let doc_yaml = if let Some(include_line) = snippet_trimmed
            .lines()
            .find(|line| line.trim_start().starts_with("--8<--"))
        {
            let directive = include_line.trim_start();
            let path_part = directive
                .trim_start_matches("--8<--")
                .trim()
                .trim_matches('"');
            let include_path = repo_root.join(path_part);
            fs::read_to_string(&include_path)
                .unwrap_or_else(|_| panic!("snippet include {} missing", include_path.display()))
        } else {
            snippet_trimmed.to_string()
        };

        let doc_recipe: Value =
            serde_yaml::from_str(&doc_yaml).expect("doc recipe snippet to parse as YAML");
        let example_recipe: Value =
            serde_yaml::from_str(&example_src).expect("example recipe to parse as YAML");

        assert_eq!(
            doc_recipe,
            example_recipe,
            "documentation recipe example diverges from {}",
            example_path.display()
        );

        let validation_errors: Vec<String> = match RECIPE_SCHEMA.validate(&doc_recipe) {
            Ok(_) => Vec::new(),
            Err(errors) => errors
                .map(|err| format!("{}: {}", err.instance_path, err))
                .collect(),
        };
        if !validation_errors.is_empty() {
            panic!(
                "documentation recipe example failed schema validation: {}",
                validation_errors.join("; ")
            );
        }
    }

    #[test]
    fn reject_workflow_referencing_unknown_tool() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("recipe.yaml");
        fs::write(
            &path,
            r#"id: workflow-missing-tool
name: Workflow Missing Tool
version: 1.0.0
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Test missing tool"
tools:
  - id: sample_tool
workflows:
  - step: "Use missing tool"
    tool: missing_tool
"#,
        )
        .unwrap();
        match Recipe::load(&path) {
            Ok(_) => panic!("expected missing tool validation to fail"),
            Err(err) => {
                let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
                assert!(
                    chain
                        .iter()
                        .any(|msg| msg.contains("references undeclared tool `missing_tool`")),
                    "unexpected error chain: {chain:?}"
                );
            }
        }
    }

    #[test]
    fn reject_duplicate_workflow_steps() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("recipe.yaml");
        fs::write(
            &path,
            r#"id: workflow-duplicate-steps
name: Workflow Duplicate Steps
version: 1.0.0
model:
  preferred: "local:llama"
permissions:
  file.read: allow
prompts:
  system: "Test duplicate workflow steps"
tools:
  - id: sample_tool
  - id: sample_tool_b
workflows:
  - step: "Do thing"
    tool: sample_tool
  - step: "Do thing"
    tool: sample_tool_b
"#,
        )
        .unwrap();
        match Recipe::load(&path) {
            Ok(_) => panic!("expected duplicate workflow validation to fail"),
            Err(err) => {
                let chain: Vec<String> = err.chain().map(|e| e.to_string()).collect();
                assert!(
                    chain
                        .iter()
                        .any(|msg| msg.contains("duplicate workflow step `Do thing`")),
                    "unexpected error chain: {chain:?}"
                );
            }
        }
    }
}
