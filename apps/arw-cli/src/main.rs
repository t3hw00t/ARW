use anyhow::Result;
mod commands;
mod logic_units;
mod recipes;
mod research_watcher;
use arw_core::{hello_core, load_effective_paths};
use clap::CommandFactory;
use clap::{Args, Parser, Subcommand};
use commands::{
    AdminCmd, CapCmd, ContextCmd, EventsCmd, GateCmd, HttpCmd, OrchestratorCmd, PathsArgs,
    PingArgs, RuntimeCmd, ScreenshotsCmd, SmokeCmd, SpecCmd, StateCmd, ToolsListArgs,
    ToolsSubcommand,
};
use logic_units::LogicUnitsCmd;
use recipes::RecipesCmd;
use research_watcher::ResearchWatcherCmd;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "arw-cli", version, about = "ARW CLI utilities")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Print effective runtime/cache/logs paths (JSON)
    Paths(PathsArgs),
    /// Tool helpers (list, cache stats)
    Tools {
        #[command(flatten)]
        list: ToolsListArgs,
        #[command(subcommand)]
        cmd: Option<ToolsSubcommand>,
    },
    /// Admin helpers
    Admin {
        #[command(subcommand)]
        cmd: AdminCmd,
    },
    /// Gating helpers
    Gate {
        #[command(subcommand)]
        cmd: GateCmd,
    },
    /// Policy capsules (templates, keys, signatures)
    Capsule {
        #[command(subcommand)]
        cmd: CapCmd,
    },
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Ping the service and print status
    Ping(PingArgs),
    /// Spec helpers
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },
    /// Recipe helpers (validate, install, inspect)
    Recipes {
        #[command(subcommand)]
        cmd: RecipesCmd,
    },
    /// HTTP helpers (net.http actions)
    Http {
        #[command(subcommand)]
        cmd: HttpCmd,
    },
    /// Orchestrator helpers (catalog, jobs, training)
    Orchestrator {
        #[command(subcommand)]
        cmd: OrchestratorCmd,
    },
    /// Logic unit helpers (inspect, install, list)
    LogicUnits {
        #[command(subcommand)]
        cmd: LogicUnitsCmd,
    },
    /// Research watcher helpers (list, approve, archive)
    ResearchWatcher {
        #[command(subcommand)]
        cmd: ResearchWatcherCmd,
    },
    /// Screenshots maintenance commands
    Screenshots {
        #[command(subcommand)]
        cmd: ScreenshotsCmd,
    },
    /// Managed runtime supervisor helpers
    Runtime {
        #[command(subcommand)]
        cmd: RuntimeCmd,
    },
    /// State snapshots
    State {
        #[command(subcommand)]
        cmd: StateCmd,
    },
    /// Context and training telemetry helpers
    Context {
        #[command(subcommand)]
        cmd: ContextCmd,
    },
    /// Event journal helpers
    Events {
        #[command(subcommand)]
        cmd: EventsCmd,
    },
    /// Smoke checks for local validation
    Smoke {
        #[command(subcommand)]
        cmd: SmokeCmd,
    },
}

#[derive(Args)]
struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, powershell, elvish)
    shell: clap_complete::Shell,
    /// Output directory (writes a file). If not set, prints to stdout.
    #[arg(long)]
    out_dir: Option<String>,
}
fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Paths(args)) => {
            if let Err(e) = commands::paths::run(args) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Tools { list, cmd }) => {
            if let Err(e) = commands::tools::execute(list, cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Admin { cmd }) => {
            if let Err(e) = commands::admin::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Gate { cmd }) => {
            if let Err(e) = commands::gate::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Capsule { cmd }) => {
            if let Err(e) = commands::capsule::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Completions(args)) => {
            if let Err(e) = cmd_completions(args.shell, args.out_dir.as_deref()) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Ping(args)) => {
            if let Err(e) = commands::status::run_ping(&args) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Spec { cmd: spec }) => {
            if let Err(e) = commands::status::execute_spec(spec) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Http { cmd }) => {
            if let Err(e) = commands::http::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Orchestrator { cmd }) => {
            if let Err(e) = commands::orchestrator::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Recipes { cmd }) => {
            if let Err(e) = recipes::run(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::LogicUnits { cmd }) => {
            if let Err(e) = logic_units::run(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::ResearchWatcher { cmd }) => {
            if let Err(e) = research_watcher::run(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Screenshots { cmd }) => {
            if let Err(e) = commands::screenshots::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Runtime { cmd }) => {
            if let Err(e) = commands::runtime::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::State { cmd }) => {
            if let Err(e) = commands::state::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Context { cmd }) => {
            if let Err(e) = commands::context::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Events { cmd }) => {
            if let Err(e) = commands::events::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Smoke { cmd }) => {
            if let Err(e) = commands::smoke::execute(cmd) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        None => {
            println!("arw-cli {} — bootstrap", env!("CARGO_PKG_VERSION"));
            hello_core();
            println!("{}", load_effective_paths());
        }
    }
}
fn cmd_completions(shell: clap_complete::Shell, out_dir: Option<&str>) -> Result<()> {
    use clap_complete::{generate, generate_to};
    use std::io::stdout;
    let mut cmd = Cli::command();
    let bin = "arw-cli";
    if let Some(dir) = out_dir {
        let dir_path = std::path::Path::new(dir);
        std::fs::create_dir_all(dir_path).ok();
        let _path = generate_to(shell, &mut cmd, bin, dir_path)?;
    } else {
        generate(shell, &mut cmd, bin, &mut stdout());
    }
    Ok(())
}
