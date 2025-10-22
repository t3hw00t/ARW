use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand};
use jsonschema::{self, Draft, Validator};
use once_cell::sync::Lazy;
use reqwest::blocking::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::commands::util::{resolve_admin_token, with_admin_headers};

static LOGIC_UNIT_SCHEMA: Lazy<Validator> = Lazy::new(|| {
    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../spec/schemas/logic_unit_manifest.json"
    ));
    let schema_json: Value = serde_json::from_str(raw).expect("logic unit schema json to parse");
    jsonschema::options()
        .with_draft(Draft::Draft7)
        .build(&schema_json)
        .expect("logic unit schema to compile")
});

const MANIFEST_CANDIDATES: &[&str] = &[
    "manifest.yaml",
    "manifest.yml",
    "logic-unit.yaml",
    "logic-unit.yml",
    "manifest.json",
    "logic-unit.json",
];

#[derive(Subcommand)]
pub enum LogicUnitsCmd {
    /// List logic units registered in the kernel
    List(LogicUnitsListArgs),
    /// Validate a manifest and print a summary
    Inspect(LogicUnitsInspectArgs),
    /// Install a manifest via the admin API
    Install(LogicUnitsInstallArgs),
}

#[derive(Args)]
pub struct LogicUnitBaseArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Request timeout (seconds)
    #[arg(long, default_value_t = 10)]
    timeout: u64,
}

impl LogicUnitBaseArgs {
    fn base_url(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}

#[derive(Args)]
pub struct LogicUnitsListArgs {
    #[command(flatten)]
    base: LogicUnitBaseArgs,
    /// Limit number of logic units returned
    #[arg(long)]
    limit: Option<i64>,
    /// Emit JSON response from the server
    #[arg(long)]
    json: bool,
    /// Pretty-print JSON output (requires --json)
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
pub struct LogicUnitsInspectArgs {
    /// Path to a manifest file or folder containing one
    pub source: PathBuf,
    /// Output as JSON (summary + manifest)
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct LogicUnitsInstallArgs {
    #[command(flatten)]
    base: LogicUnitBaseArgs,
    /// Path to a manifest file or folder containing one
    pub source: PathBuf,
    /// Override manifest id before installing
    #[arg(long)]
    pub id: Option<String>,
    /// Print payload without sending to the server
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogicUnitSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub category: Option<String>,
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    pub manifest_path: String,
    pub patch_count: usize,
}

struct LogicUnit {
    summary: LogicUnitSummary,
    manifest: Value,
    source_root: PathBuf,
}

pub fn run(cmd: LogicUnitsCmd) -> Result<()> {
    match cmd {
        LogicUnitsCmd::List(args) => list(args),
        LogicUnitsCmd::Inspect(args) => inspect(args),
        LogicUnitsCmd::Install(args) => install(args),
    }
}

fn list(args: LogicUnitsListArgs) -> Result<()> {
    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout.max(1)))
        .build()
        .context("building HTTP client")?;
    let base = args.base.base_url();
    let mut url = format!("{}/logic-units", base);
    if let Some(limit) = args.limit {
        if limit > 0 {
            url.push('?');
            url.push_str(&format!("limit={}", limit));
        }
    }
    let resp = with_admin_headers(client.get(&url), token.as_deref())
        .send()
        .with_context(|| format!("requesting logic units from {}", url))?;
    let status = resp.status();
    let text = resp.text().context("reading logic unit list response")?;
    if !status.is_success() {
        bail!(
            "logic units list failed: status {} body {}",
            status,
            text.trim()
        );
    }
    if args.json {
        if args.pretty {
            let json: Value = serde_json::from_str(&text).context("parsing server json")?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json).unwrap_or_else(|_| text.clone())
            );
        } else {
            println!("{}", text);
        }
        return Ok(());
    }
    let json: Value = serde_json::from_str(&text).context("parsing server json")?;
    let items = json
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        println!("(no logic units)");
        return Ok(());
    }
    print_remote_table(&items);
    Ok(())
}

fn inspect(args: LogicUnitsInspectArgs) -> Result<()> {
    let logic_unit = LogicUnit::load(&args.source)?;
    if args.json {
        let out = json!({
            "summary": logic_unit.summary,
            "manifest": logic_unit.manifest,
            "source_root": logic_unit.source_root.to_string_lossy(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&out).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        print_summary(&logic_unit);
    }
    Ok(())
}

fn install(args: LogicUnitsInstallArgs) -> Result<()> {
    let logic_unit = LogicUnit::load(&args.source)?;
    let mut manifest = logic_unit.manifest.clone();
    if let Some(id) = &args.id {
        manifest["id"] = json!(id);
    }
    if args.dry_run {
        println!(
            "{}",
            serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| manifest.to_string())
        );
        return Ok(());
    }

    let token = resolve_admin_token(&args.base.admin_token);
    let client = Client::builder()
        .timeout(Duration::from_secs(args.base.timeout.max(1)))
        .build()
        .context("building HTTP client")?;
    let url = format!("{}/logic-units/install", args.base.base_url());
    let resp = with_admin_headers(client.post(&url).json(&manifest), token.as_deref())
        .send()
        .with_context(|| format!("installing logic unit via {}", url))?;
    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if status == reqwest::StatusCode::CREATED || status == reqwest::StatusCode::OK {
        let id = manifest
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown>");
        println!("Installed logic unit {}", id);
        Ok(())
    } else {
        bail!(
            "logic unit install failed: status {} body {}",
            status,
            body.trim()
        )
    }
}

impl LogicUnit {
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
        let source_root = canonical
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| canonical.clone());
        Ok(Self::from_manifest(manifest, canonical, source_root))
    }

    fn load_from_dir(dir: &Path) -> Result<Self> {
        let root = dir
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {}", dir.display()))?;
        for candidate in MANIFEST_CANDIDATES {
            let candidate_path = root.join(candidate);
            if candidate_path.is_file() {
                if let Ok(manifest) = load_manifest_from_file(&candidate_path) {
                    return Ok(Self::from_manifest(manifest, candidate_path, root.clone()));
                }
            }
        }
        let mut last_err: Option<anyhow::Error> = None;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && looks_like_manifest_candidate(&path) {
                match load_manifest_from_file(&path) {
                    Ok(manifest) => return Ok(Self::from_manifest(manifest, path, root.clone())),
                    Err(err) => {
                        last_err = Some(err);
                    }
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

    fn from_manifest(manifest: Value, manifest_path: PathBuf, source_root: PathBuf) -> Self {
        let summary = build_summary(&manifest, &manifest_path);
        Self {
            summary,
            manifest,
            source_root,
        }
    }
}

fn load_manifest_from_file(path: &Path) -> Result<Value> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("failed to read manifest {}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let value: Value = if ext == "json" {
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse manifest {}", path.display()))?
    } else {
        serde_yaml::from_str(&data)
            .with_context(|| format!("failed to parse manifest {}", path.display()))?
    };
    let joined: Vec<_> = LOGIC_UNIT_SCHEMA
        .iter_errors(&value)
        .map(|err| format!("{}: {}", err.instance_path, err))
        .collect();
    if !joined.is_empty() {
        bail!(
            "{} does not satisfy schema: {}",
            path.display(),
            joined.join("; ")
        );
    }
    Ok(value)
}

fn build_summary(manifest: &Value, manifest_path: &Path) -> LogicUnitSummary {
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
    let category = manifest
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let kind = manifest
        .get("kind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let slots = manifest
        .get("slots")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let authors = manifest
        .get("authors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let patches = manifest
        .get("patches")
        .and_then(|v| v.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0);

    LogicUnitSummary {
        id,
        name,
        version,
        category,
        kind,
        slots,
        authors,
        manifest_path: manifest_path.to_string_lossy().to_string(),
        patch_count: patches,
    }
}

fn looks_like_manifest_candidate(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => matches!(ext.to_ascii_lowercase().as_str(), "yaml" | "yml" | "json"),
        None => false,
    }
}

fn print_summary(lu: &LogicUnit) {
    println!("Logic Unit: {}", lu.summary.id);
    println!("  Name: {}", lu.summary.name);
    println!("  Version: {}", lu.summary.version);
    if let Some(cat) = &lu.summary.category {
        println!("  Category: {}", cat);
    }
    if let Some(kind) = &lu.summary.kind {
        println!("  Kind: {}", kind);
    }
    if !lu.summary.slots.is_empty() {
        println!("  Slots: {}", lu.summary.slots.join(", "));
    }
    if !lu.summary.authors.is_empty() {
        println!("  Authors: {}", lu.summary.authors.join(", "));
    }
    println!("  Manifest: {}", lu.summary.manifest_path);
    println!("  Source: {}", lu.source_root.display());
    println!("  Patches: {}", lu.summary.patch_count);
}

fn print_remote_table(items: &[Value]) {
    let mut rows: Vec<(String, String, String, String)> = Vec::new();
    for entry in items {
        let id = entry
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let updated = entry
            .get("updated")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let category = entry
            .get("manifest")
            .and_then(|m| m.get("category"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        rows.push((id, status, category, updated));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let id_width = rows
        .iter()
        .map(|r| r.0.len())
        .chain(std::iter::once("ID".len()))
        .max()
        .unwrap_or(2);
    let status_width = rows
        .iter()
        .map(|r| r.1.len())
        .chain(std::iter::once("Status".len()))
        .max()
        .unwrap_or(6);
    let category_width = rows
        .iter()
        .map(|r| r.2.len())
        .chain(std::iter::once("Category".len()))
        .max()
        .unwrap_or(8);
    let updated_width = rows
        .iter()
        .map(|r| r.3.len())
        .chain(std::iter::once("Updated".len()))
        .max()
        .unwrap_or(7);
    println!(
        "{:<id_width$}  {:<status_width$}  {:<category_width$}  {:<updated_width$}",
        "ID",
        "Status",
        "Category",
        "Updated",
        id_width = id_width,
        status_width = status_width,
        category_width = category_width,
        updated_width = updated_width
    );
    println!(
        "{:-<id_width$}  {:-<status_width$}  {:-<category_width$}  {:-<updated_width$}",
        "",
        "",
        "",
        "",
        id_width = id_width,
        status_width = status_width,
        category_width = category_width,
        updated_width = updated_width
    );
    for (id, status, category, updated) in rows {
        println!(
            "{:<id_width$}  {:<status_width$}  {:<category_width$}  {:<updated_width$}",
            id,
            status,
            category,
            updated,
            id_width = id_width,
            status_width = status_width,
            category_width = category_width,
            updated_width = updated_width
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_manifest() -> &'static str {
        r#"id: sample-lu
name: Sample Logic Unit
version: "1.0.0"
category: reasoning
kind: config
patches:
  - target: agent://recipes/reasoning
    op: merge
    value:
      mode: Sample
"#
    }

    #[test]
    fn load_logic_unit_from_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("logic.yaml");
        fs::write(&path, sample_manifest()).unwrap();
        let lu = LogicUnit::load(&path).unwrap();
        assert_eq!(lu.summary.id, "sample-lu");
        assert_eq!(lu.summary.category.as_deref(), Some("reasoning"));
        assert_eq!(lu.summary.patch_count, 1);
    }

    #[test]
    fn load_logic_unit_from_directory() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("unit");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.yaml"), sample_manifest()).unwrap();
        let lu = LogicUnit::load(&dir).unwrap();
        assert_eq!(lu.summary.name, "Sample Logic Unit");
    }
}
