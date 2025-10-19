use anyhow::{bail, Context, Result};
use jsonschema::{self, Draft, Validator};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn logic_unit_schema() -> &'static Validator {
    static SCHEMA: once_cell::sync::Lazy<Validator> = once_cell::sync::Lazy::new(|| {
        let raw = include_str!("../../../spec/schemas/logic_unit_manifest.json");
        let json: Value =
            serde_json::from_str(raw).expect("logic unit manifest schema to parse as JSON");
        jsonschema::options()
            .with_draft(Draft::Draft7)
            .build(&json)
            .expect("logic unit manifest schema to compile")
    });
    &SCHEMA
}

fn logic_units_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/logic-units")
        .canonicalize()
        .expect("examples/logic-units directory to resolve")
}

#[test]
fn logic_unit_examples_validate_against_schema() -> Result<()> {
    let schema = logic_unit_schema();
    let dir = logic_units_dir();
    let entries = fs::read_dir(&dir)
        .with_context(|| format!("reading logic unit examples directory {}", dir.display()))?;

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
            .with_context(|| format!("reading logic unit example {}", path.display()))?;
        let manifest: Value = if ext == "json" {
            serde_json::from_str(&raw)
                .with_context(|| format!("parsing logic unit example JSON {}", path.display()))?
        } else {
            serde_yaml::from_str(&raw)
                .with_context(|| format!("parsing logic unit example YAML {}", path.display()))?
        };
        let collected: Vec<_> = schema
            .iter_errors(&manifest)
            .map(|err| err.to_string())
            .collect();
        if !collected.is_empty() {
            bail!(
                "validating logic unit example {} failed: {}",
                path.display(),
                collected.join("; ")
            );
        };
    }

    assert!(
        found,
        "no logic unit examples discovered under {}",
        dir.display()
    );
    Ok(())
}
