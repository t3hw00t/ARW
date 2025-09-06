use arw_core::hello_core;
use tracing_subscriber::{EnvFilter, fmt};

fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    println!("arw-cli 0.1.0 — bootstrap");
    hello_core();
}
