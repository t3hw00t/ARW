use anyhow::{Context, Result};
use arw_runtime::RuntimeModality;
use arw_runtime_adapter::load_manifest_with_report;
use arw_runtime_adapter::manifest::{
    AdapterConsent, AdapterEntrypoint, AdapterHealthSpec, AdapterMetric, AdapterResources,
    RuntimeAdapterManifest,
};
use arw_runtime_adapter::ValidationReport;
use clap::{Args, Subcommand};
use schemars::schema_for;
use serde::Serialize;

#[derive(Subcommand)]
pub enum AdaptersCmd {
    /// Validate an adapter manifest file
    Validate(AdaptersValidateArgs),
    /// Emit JSON Schema for adapter manifests
    Schema(AdaptersSchemaArgs),
    /// Scaffold a new adapter manifest file
    Init(AdaptersInitArgs),
}

#[derive(Args, Clone)]
pub struct AdaptersValidateArgs {
    /// Path to manifest file (JSON or TOML)
    #[arg(long)]
    pub manifest: String,
    /// Emit JSON instead of a human summary
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON (only with --json)
    #[arg(long, requires = "json")]
    pub pretty: bool,
    /// Treat warnings as errors (non-zero exit)
    #[arg(long)]
    pub strict_warnings: bool,
}

#[derive(Args, Clone)]
pub struct AdaptersSchemaArgs {
    /// Output path (writes file). If not set, prints to stdout.
    #[arg(long)]
    pub out: Option<String>,
}

pub fn execute(cmd: AdaptersCmd) -> Result<()> {
    match cmd {
        AdaptersCmd::Validate(args) => cmd_validate(args),
        AdaptersCmd::Schema(args) => cmd_schema(args),
        AdaptersCmd::Init(args) => cmd_init(args),
    }
}

fn cmd_validate(args: AdaptersValidateArgs) -> Result<()> {
    let path = args.manifest;
    let (manifest, report) = load_manifest_with_report(&path)
        .with_context(|| format!("loading manifest at {}", path))?;

    if args.json {
        let out = ManifestReportOut {
            manifest,
            report: report.clone(),
        };
        if args.pretty {
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!("{}", serde_json::to_string(&out)?);
        }
    } else {
        print_human(&manifest, &report);
    }

    if !report.errors.is_empty() {
        anyhow::bail!(
            "manifest has {} error(s); fix and retry",
            report.errors.len()
        );
    }
    if args.strict_warnings && !report.warnings.is_empty() {
        anyhow::bail!(
            "manifest has {} warning(s) (strict); address or drop --strict-warnings",
            report.warnings.len()
        );
    }
    Ok(())
}

fn print_human(manifest: &RuntimeAdapterManifest, report: &ValidationReport) {
    println!(
        "Adapter: {} v{}{}",
        manifest.id,
        manifest.version,
        manifest
            .name
            .as_ref()
            .map(|n| format!(" ({})", n))
            .unwrap_or_default()
    );
    if !manifest.modalities.is_empty() {
        let list = manifest
            .modalities
            .iter()
            .map(|m| format!("{:?}", m))
            .collect::<Vec<_>>()
            .join(", ");
        println!("- modalities: {}", list);
    }
    if !manifest.tags.is_empty() {
        println!("- tags: {}", manifest.tags.join(", "));
    }
    if let Some(consent) = &manifest.consent {
        if !consent.summary.trim().is_empty() {
            println!("- consent: {}", consent.summary.trim());
        }
    }
    if !report.errors.is_empty() {
        println!("Errors ({}):", report.errors.len());
        for e in &report.errors {
            println!("  - {}: {}", e.field, e.message);
        }
    } else {
        println!("Errors: none");
    }
    if !report.warnings.is_empty() {
        println!("Warnings ({}):", report.warnings.len());
        for w in &report.warnings {
            println!("  - {}: {}", w.field, w.message);
        }
    } else {
        println!("Warnings: none");
    }
}

#[derive(Serialize)]
struct ManifestReportOut {
    manifest: RuntimeAdapterManifest,
    report: ValidationReport,
}

fn cmd_schema(args: AdaptersSchemaArgs) -> Result<()> {
    let schema = schema_for!(RuntimeAdapterManifest);
    let json = serde_json::to_string_pretty(&schema)?;
    if let Some(path) = args.out {
        std::fs::create_dir_all(
            std::path::Path::new(&path)
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        )
        .ok();
        std::fs::write(&path, json).with_context(|| format!("writing schema to {}", path))?;
    } else {
        println!("{}", json);
    }
    Ok(())
}

#[derive(Args, Clone)]
pub struct AdaptersInitArgs {
    /// Output path for the manifest (e.g., adapters/my.adapter.json|toml)
    #[arg(long)]
    pub out: String,
    /// Adapter id (letters/digits/._-)
    #[arg(long)]
    pub id: String,
    /// Human friendly adapter name
    #[arg(long)]
    pub name: Option<String>,
    /// Serialization format (json|toml)
    #[arg(long, value_parser = ["json", "toml"], default_value = "json")]
    pub format: String,
}

fn cmd_init(args: AdaptersInitArgs) -> Result<()> {
    use std::fs;
    use std::path::Path;

    let manifest = RuntimeAdapterManifest {
        id: args.id.clone(),
        version: "0.1.0".into(),
        name: args.name.clone(),
        description: Some("Scaffolded adapter manifest".into()),
        modalities: vec![RuntimeModality::Text],
        tags: vec!["demo".into()],
        entrypoint: AdapterEntrypoint {
            crate_name: "your_adapter_crate".into(),
            symbol: "create_adapter".into(),
            kind: None,
        },
        resources: AdapterResources {
            accelerator: None,
            recommended_memory_mb: Some(1024),
            recommended_cpu_threads: Some(4),
            requires_network: Some(false),
        },
        consent: Some(AdapterConsent {
            summary: "Processes local text prompts".into(),
            details_url: None,
            capabilities: vec!["read_files".into()],
        }),
        metrics: vec![AdapterMetric {
            name: "tokens_processed_total".into(),
            description: Some("Total tokens".into()),
            unit: Some("count".into()),
        }],
        health: AdapterHealthSpec::default(),
        metadata: Default::default(),
    };

    // Validate before writing to help users catch id/name issues
    let report = manifest.validate();
    if !report.errors.is_empty() {
        anyhow::bail!("invalid id or fields: {} error(s)", report.errors.len());
    }

    let out_path = Path::new(&args.out);
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).ok();
    }

    let lower = args.format.to_ascii_lowercase();
    match lower.as_str() {
        "json" => {
            // Insert $schema for editor validation
            let mut value = serde_json::to_value(&manifest)?;
            if let serde_json::Value::Object(map) = &mut value {
                map.insert(
                    "$schema".into(),
                    serde_json::Value::String(
                        "https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json"
                            .into(),
                    ),
                );
            }
            let s = serde_json::to_string_pretty(&value)?;
            fs::write(out_path, s)?;
        }
        "toml" => {
            let s = toml::to_string_pretty(&manifest)?;
            fs::write(out_path, s)?;
        }
        _ => unreachable!(),
    }

    println!("Created scaffold at {}", out_path.display());
    Ok(())
}
