use anyhow::Result;
use arw_core::load_effective_paths;
use clap::Args;

#[derive(Args)]
pub struct PathsArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

pub fn run(args: PathsArgs) -> Result<()> {
    let v = load_effective_paths();
    if args.pretty {
        println!(
            "{}",
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
        );
    } else {
        println!("{}", v);
    }
    Ok(())
}
