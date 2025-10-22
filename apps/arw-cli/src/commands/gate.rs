use anyhow::Result;
use arw_core::{gating, gating_keys};
use chrono::Utc;

use clap::{Args, Subcommand};
use serde_json::Value as JsonValue;

#[derive(Subcommand)]
pub enum GateCmd {
    /// List known gating keys
    Keys(GateKeysArgs),
    /// Gating policy helpers
    Config {
        #[command(subcommand)]
        cmd: GateConfigCmd,
    },
}

#[derive(Subcommand)]
pub enum GateConfigCmd {
    /// Print the gating config JSON schema
    Schema(GateConfigSchemaArgs),
    /// Render the gating config reference (Markdown)
    Doc(GateConfigDocArgs),
}

#[derive(Args)]
pub struct GateKeysArgs {
    /// Show grouped metadata and stability details
    #[arg(long, conflicts_with_all = ["json", "doc"])]
    details: bool,
    /// Emit JSON instead of text
    #[arg(long, conflicts_with_all = ["details", "doc"])]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
    /// Render the Markdown reference (matches docs)
    #[arg(long, conflicts_with_all = ["json", "details"])]
    doc: bool,
}

#[derive(Args)]
pub struct GateConfigSchemaArgs {
    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
pub struct GateConfigDocArgs {}

pub fn execute(cmd: GateCmd) -> Result<()> {
    match cmd {
        GateCmd::Keys(args) => render_keys(args),
        GateCmd::Config { cmd } => match cmd {
            GateConfigCmd::Schema(args) => render_schema(args),
            GateConfigCmd::Doc(_) => render_doc(),
        },
    }
}

fn render_keys(args: GateKeysArgs) -> Result<()> {
    if args.json {
        let payload = gating_keys::render_json(None);
        if args.pretty {
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".into())
            );
        } else {
            println!(
                "{}",
                serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
            );
        }
        return Ok(());
    }

    if args.doc {
        let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
        print!("{}", gating_keys::render_markdown(&now));
        return Ok(());
    }

    if args.details {
        let groups = gating_keys::groups();
        let total_keys: usize = groups.iter().map(|g| g.keys.len()).sum();
        println!(
            "Total groups: {} | Total keys: {}\n",
            groups.len(),
            total_keys
        );
        for group in groups {
            println!("{} - {}", group.name, group.summary);
            for key in group.keys {
                println!("  {:<24} {:<8} {}", key.id, key.stability, key.summary);
            }
            println!();
        }
        return Ok(());
    }

    for key in gating_keys::list() {
        println!("{}", key);
    }
    Ok(())
}

fn render_schema(args: GateConfigSchemaArgs) -> Result<()> {
    let schema: JsonValue = gating::gating_config_schema_json();
    let rendered = if args.pretty {
        serde_json::to_string_pretty(&schema)
    } else {
        serde_json::to_string(&schema)
    };
    match rendered {
        Ok(doc) => println!("{}", doc),
        Err(err) => {
            eprintln!("failed to render gating config schema: {err}");
            println!("{{}}");
        }
    };
    Ok(())
}

fn render_doc() -> Result<()> {
    let now = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    print!("{}", gating::render_config_markdown(&now));
    Ok(())
}
