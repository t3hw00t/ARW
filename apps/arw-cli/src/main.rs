use arw_core::{hello_core, print_effective_paths};
use tracing_subscriber::{fmt, EnvFilter};

fn main() {
    // Minimal help handling to avoid noisy output on --help/-h
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("arw-cli 0.1.0\n\nUsage:\n  arw-cli            Print core bootstrap info\n  arw-cli --help     Show this help\n");
        return;
    }
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    println!("arw-cli 0.1.0 — bootstrap");
    hello_core();
    print_effective_paths();
}
