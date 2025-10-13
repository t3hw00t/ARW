use anyhow::{bail, Context, Result};
use arw_core::{
    effective_paths,
    recipes::{self, Recipe, RecipeSourceKind, RecipeSummary},
};
use clap::{Args, Subcommand};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

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
        if meta.is_dir() || (meta.is_file() && recipes::looks_like_manifest_candidate(&path)) {
            match Recipe::load(&path) {
                Ok(recipe) => recipes.push(recipe),
                Err(err) => {
                    eprintln!("skipping {}: {}", path.display(), err);
                }
            }
        }
    }

    recipes.sort_by(|a, b| {
        let a_sum = a.summary();
        let b_sum = b.summary();
        a_sum.id.cmp(&b_sum.id)
    });
    let summaries: Vec<RecipeSummary> = recipes
        .iter()
        .map(|recipe| recipe.summary().clone())
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
            "summary": recipe.summary().clone(),
            "manifest": recipe.manifest().clone(),
            "source_root": recipe.source_root().to_string_lossy(),
            "manifest_path": recipe.manifest_path().to_string_lossy(),
            "kind": recipe.kind(),
            "tools": recipe.tool_ids(),
            "workflow_steps": recipe.workflow_steps(),
            "permissions": recipe.permission_modes(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        let summary = recipe.summary();
        println!("Recipe: {}", summary.id);
        println!("  Name: {}", summary.name);
        println!("  Version: {}", summary.version);
        if let Some(model) = &summary.preferred_model {
            println!("  Model (preferred): {}", model);
        }
        if let Some(model) = &summary.fallback_model {
            println!("  Model (fallback): {}", model);
        }
        if !summary.tags.is_empty() {
            println!("  Tags: {}", summary.tags.join(", "));
        }
        println!("  Manifest: {}", summary.manifest_path);
        println!("  Source: {}", recipe.source_root().display());
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

    let dest_id = args.id.unwrap_or_else(|| recipe.summary().id.clone());
    if dest_id.is_empty() {
        bail!("manifest id is empty; use --id to supply a destination id");
    }
    let dest_dir = recipes_dir.join(&dest_id);
    if dest_dir.exists() {
        let dest_real = dest_dir
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", dest_dir.display()))?;
        let source_real = match recipe.kind() {
            RecipeSourceKind::File => recipe.manifest_path().to_path_buf(),
            RecipeSourceKind::Directory => recipe.source_root().to_path_buf(),
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

    match recipe.kind() {
        RecipeSourceKind::File => {
            fs::create_dir_all(&dest_dir)
                .with_context(|| format!("failed to create {}", dest_dir.display()))?;
            let manifest_name = recipe
                .manifest_path()
                .file_name()
                .map(|s| s.to_os_string())
                .unwrap_or_else(|| "manifest.yaml".into());
            let dest_path = dest_dir.join(manifest_name);
            fs::copy(recipe.manifest_path(), &dest_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    recipe.manifest_path().display(),
                    dest_path.display()
                )
            })?;
        }
        RecipeSourceKind::Directory => {
            copy_directory(recipe.source_root(), &dest_dir)?;
        }
    }

    println!(
        "Installed recipe {} → {}",
        recipe.summary().id,
        dest_dir.display()
    );
    Ok(())
}

fn detect_state_dir(override_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir);
    }
    let paths = effective_paths();
    Ok(PathBuf::from(paths.state_dir))
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
    use serde_json::Value;
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
    fn install_recipe_as_directory_copy() {
        let temp = tempdir().unwrap();
        let src = temp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("manifest.yaml"), sample_manifest()).unwrap();
        fs::write(src.join("notes.txt"), "notes").unwrap();
        let state = temp.path().join("state");
        fs::create_dir_all(&state).unwrap();

        install(RecipesInstallArgs {
            source: src.clone(),
            state_dir: Some(state.clone()),
            force: false,
            id: None,
        })
        .unwrap();

        let recipe = Recipe::load(&src).unwrap();
        let dest = state.join("recipes").join(recipe.summary().id.clone());
        assert!(dest.join("manifest.yaml").exists());
        assert!(dest.join("notes.txt").exists());
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

        let temp = tempdir().expect("doc temp dir");
        let manifest = temp.path().join("manifest.yaml");
        fs::write(&manifest, doc_yaml).expect("write doc manifest");
        recipes::Recipe::load(&manifest).expect("doc manifest to validate against schema");
    }
}
