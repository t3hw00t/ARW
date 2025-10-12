use anyhow::{bail, Context, Result};
use jsonschema::{Draft, JSONSchema};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn recipe_schema() -> &'static JSONSchema {
    static SCHEMA: once_cell::sync::Lazy<JSONSchema> = once_cell::sync::Lazy::new(|| {
        let raw = include_str!("../../../spec/schemas/recipe_manifest.json");
        let json: Value =
            serde_json::from_str(raw).expect("recipe manifest schema to parse as JSON");
        JSONSchema::options()
            .with_draft(Draft::Draft7)
            .compile(&json)
            .expect("recipe manifest schema to compile")
    });
    &SCHEMA
}

fn recipe_examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/recipes")
        .canonicalize()
        .expect("examples/recipes directory to resolve")
}

#[test]
fn recipe_examples_validate_against_schema() -> Result<()> {
    let schema = recipe_schema();
    let dir = recipe_examples_dir();
    let entries = fs::read_dir(&dir)
        .with_context(|| format!("reading recipe examples directory {}", dir.display()))?;

    let mut found = false;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry inside {}", dir.display()))?;
        if entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if !matches!(ext, "yaml" | "yml" | "json") {
            continue;
        }
        found = true;
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading recipe example {}", path.display()))?;
        let manifest: Value = if ext == "json" {
            serde_json::from_str(&raw)
                .with_context(|| format!("parsing recipe example JSON {}", path.display()))?
        } else {
            serde_yaml::from_str(&raw)
                .with_context(|| format!("parsing recipe example YAML {}", path.display()))?
        };
        if let Err(errors) = schema.validate(&manifest) {
            let collected = errors
                .map(|err| err.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            bail!(
                "validating recipe example {} failed: {}",
                path.display(),
                collected
            );
        };
    }

    assert!(
        found,
        "no recipe examples discovered under {}",
        dir.display()
    );
    Ok(())
}
