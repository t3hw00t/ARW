use arw_core::{hello_core, introspect_tools, load_effective_paths};
use tracing_subscriber::{fmt, EnvFilter};

fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Lightweight commands
    let mut args = std::env::args().skip(1);
    if let Some(cmd) = args.next() {
        match cmd.as_str() {
            "--help" | "-h" | "help" => {
                println!("arw-cli 0.1.0\n\nUsage:\n  arw-cli paths         Print effective paths (JSON)\n  arw-cli tools         Print tool list (JSON)\n  arw-cli               Bootstrap info\n");
                return;
            }
            "paths" => {
                let v = load_effective_paths();
                println!("{}", v);
                return;
            }
            "tools" => {
                let list = introspect_tools();
                match serde_json::to_string(&list) {
                    Ok(s) => println!("{}", s),
                    Err(_) => println!("[]"),
                }
                return;
            }
            _ => {}
        }
    }

    println!("arw-cli 0.1.0 — bootstrap");
    hello_core();
    println!("{}", load_effective_paths());
}
