use anyhow::Result;
use arw_core::{gating_keys, hello_core, introspect_tools, load_effective_paths};
use base64::Engine;
use clap::CommandFactory;
use clap::{Args, Parser, Subcommand};
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
    /// Print tool list (JSON)
    Tools(ToolsArgs),
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
}

#[derive(Subcommand)]
enum GateCmd {
    /// List known gating keys
    Keys(GateKeysArgs),
}

#[derive(Subcommand)]
enum CapCmd {
    /// Print a minimal capsule template (JSON)
    Template(TemplateArgs),
    /// Generate an ed25519 keypair (b64) and print
    GenEd25519(GenKeyArgs),
    /// Sign a capsule file with ed25519 secret key (b64) and print signature
    SignEd25519(SignArgs),
    /// Verify a capsule file signature with ed25519 public key (b64)
    VerifyEd25519(VerifyArgs),
}

#[derive(Args)]
struct PathsArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
struct ToolsArgs {
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

#[derive(Args)]
struct GateKeysArgs {
    /// Show grouped metadata and stability details
    #[arg(long, conflicts_with = "json")]
    details: bool,
    /// Emit JSON instead of text
    #[arg(long, conflicts_with = "details")]
    json: bool,
    /// Pretty-print JSON output
    #[arg(long, requires = "json")]
    pretty: bool,
}

#[derive(Args)]
struct GenKeyArgs {
    /// Write public key to this file (optional)
    #[arg(long)]
    out_pub: Option<String>,
    /// Write private key to this file (optional; keep secure)
    #[arg(long)]
    out_priv: Option<String>,
    /// Issuer string to include in JSON summary (default: local-admin)
    #[arg(long)]
    issuer: Option<String>,
}

#[derive(Args)]
struct SignArgs {
    /// Secret key (b64)
    sk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Write signature to this file (optional)
    #[arg(long)]
    out: Option<String>,
}

#[derive(Args)]
struct TemplateArgs {
    /// Pretty-print JSON (default on unless --compact)
    #[arg(long)]
    pretty: bool,
    /// Print compact JSON (overrides --pretty)
    #[arg(long)]
    compact: bool,
}

#[derive(Args)]
struct VerifyArgs {
    /// Public key (b64)
    pk_b64: String,
    /// Capsule JSON file
    capsule_json: String,
    /// Signature (b64)
    sig_b64: String,
}

#[derive(Args)]
struct CompletionsArgs {
    /// Target shell (bash, zsh, fish, powershell, elvish)
    shell: clap_complete::Shell,
    /// Output directory (writes a file). If not set, prints to stdout.
    #[arg(long)]
    out_dir: Option<String>,
}

#[derive(Args)]
struct PingArgs {
    /// Base URL of the service (e.g., http://127.0.0.1:8091)
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Admin token; falls back to ARW_ADMIN_TOKEN env
    #[arg(long)]
    admin_token: Option<String>,
    /// Timeout seconds
    #[arg(long, default_value_t = 5)]
    timeout: u64,
}

#[derive(Subcommand)]
enum SpecCmd {
    /// Fetch /spec/health and print JSON
    Health(SpecHealthArgs),
}

#[derive(Args)]
struct SpecHealthArgs {
    /// Base URL of the service
    #[arg(long, default_value = "http://127.0.0.1:8091")]
    base: String,
    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,
}

fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Paths(args)) => {
            let v = load_effective_paths();
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                );
            } else {
                println!("{}", v);
            }
        }
        Some(Commands::Tools(args)) => {
            let list = introspect_tools();
            if args.pretty {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".to_string())
                );
            } else {
                match serde_json::to_string(&list) {
                    Ok(s) => println!("{}", s),
                    Err(_) => println!("[]"),
                }
            }
        }
        Some(Commands::Gate { cmd }) => match cmd {
            GateCmd::Keys(args) => {
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
                } else if args.details {
                    let groups = gating_keys::groups();
                    let total_keys: usize = groups.iter().map(|g| g.keys.len()).sum();
                    println!(
                        "Total groups: {} | Total keys: {}\n",
                        groups.len(),
                        total_keys
                    );
                    for group in groups {
                        println!("{} — {}", group.name, group.summary);
                        for key in group.keys {
                            println!("  {:<24} {:<8} {}", key.id, key.stability, key.summary);
                        }
                        println!();
                    }
                } else {
                    for key in gating_keys::list() {
                        println!("{}", key);
                    }
                }
            }
        },
        Some(Commands::Capsule { cmd }) => match cmd {
            CapCmd::Template(args) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                let tpl = serde_json::json!({
                  "id":"example",
                  "version":"1",
                  "issued_at_ms": now,
                  "issuer": "local-admin",
                  "hop_ttl": 1,
                  "propagate": "children",
                  "denies": [],
                  "contracts": [
                    {"id":"block-tools","patterns":["tools:*"],"valid_from_ms":0}
                  ]
                });
                if args.compact {
                    println!("{}", serde_json::to_string(&tpl).unwrap());
                } else {
                    // default pretty unless explicitly compact
                    if args.pretty || !args.compact {
                        println!("{}", serde_json::to_string_pretty(&tpl).unwrap());
                    } else {
                        println!("{}", serde_json::to_string(&tpl).unwrap());
                    }
                }
            }
            CapCmd::GenEd25519(args) => {
                if let Err(e) = cmd_gen_ed25519(
                    args.out_pub.as_deref(),
                    args.out_priv.as_deref(),
                    args.issuer.as_deref(),
                ) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            CapCmd::SignEd25519(args) => {
                if let Err(e) =
                    cmd_sign_ed25519(&args.sk_b64, &args.capsule_json, args.out.as_deref())
                {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
            CapCmd::VerifyEd25519(args) => {
                if let Err(e) = cmd_verify_ed25519(&args.pk_b64, &args.capsule_json, &args.sig_b64)
                {
                    eprintln!("{}", e);
                    std::process::exit(1);
                } else {
                    println!("ok");
                }
            }
        },
        Some(Commands::Completions(args)) => {
            if let Err(e) = cmd_completions(args.shell, args.out_dir.as_deref()) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Ping(args)) => {
            if let Err(e) = cmd_ping(&args) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Spec { cmd: spec }) => match spec {
            SpecCmd::Health(args) => {
                if let Err(e) = cmd_spec_health(&args) {
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        },
        None => {
            println!("arw-cli {} — bootstrap", env!("CARGO_PKG_VERSION"));
            hello_core();
            println!("{}", load_effective_paths());
        }
    }
}

fn cmd_gen_ed25519(
    out_pub: Option<&str>,
    out_priv: Option<&str>,
    issuer: Option<&str>,
) -> Result<()> {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use rand_core::TryRngCore;
    let mut rng = OsRng;
    let mut sk_bytes = [0u8; 32];
    rng.try_fill_bytes(&mut sk_bytes)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let sk = SigningKey::from_bytes(&sk_bytes);
    let pk = sk.verifying_key();
    let sk_b64 = base64::engine::general_purpose::STANDARD.encode(sk.to_bytes());
    let pk_b64 = base64::engine::general_purpose::STANDARD.encode(pk.to_bytes());
    if let Some(p) = out_pub {
        std::fs::write(p, &pk_b64)?;
    }
    if let Some(p) = out_priv {
        std::fs::write(p, &sk_b64)?;
    }
    let iss = issuer.unwrap_or("local-admin");
    println!(
        "{}",
        serde_json::json!({"issuer": iss, "alg":"ed25519","pubkey_b64": pk_b64, "privkey_b64": sk_b64 })
    );
    eprintln!("Note: store private key securely; add pubkey to configs/trust_capsules.json");
    Ok(())
}

fn cmd_sign_ed25519(sk_b64: &str, capsule_file: &str, out: Option<&str>) -> Result<()> {
    use ed25519_dalek::{Signer, SigningKey};
    let sk_bytes = base64::engine::general_purpose::STANDARD.decode(sk_b64)?;
    let sk = SigningKey::from_bytes(&sk_bytes.as_slice().try_into()?);
    let mut cap: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(capsule_file)?)?;
    if let Some(obj) = cap.as_object_mut() {
        obj.remove("signature");
    }
    let msg = serde_json::to_vec(&cap)?;
    let sig = sk.sign(&msg);
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
    if let Some(p) = out {
        std::fs::write(p, &sig_b64)?;
    }
    println!("{}", sig_b64);
    Ok(())
}

fn cmd_verify_ed25519(pk_b64: &str, capsule_file: &str, sig_b64: &str) -> Result<()> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk_bytes = base64::engine::general_purpose::STANDARD.decode(pk_b64)?;
    let vk = VerifyingKey::from_bytes(&pk_bytes.as_slice().try_into()?)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let mut cap: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(capsule_file)?)?;
    if let Some(obj) = cap.as_object_mut() {
        obj.remove("signature");
    }
    let msg = serde_json::to_vec(&cap)?;
    let sig_bytes = base64::engine::general_purpose::STANDARD.decode(sig_b64)?;
    let sig = Signature::from_bytes(&sig_bytes.as_slice().try_into()?);
    vk.verify(&msg, &sig)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
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

fn cmd_ping(args: &PingArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(args.timeout))
        .build()?;
    let mut headers = reqwest::header::HeaderMap::new();
    let tok = args
        .admin_token
        .clone()
        .or_else(|| std::env::var("ARW_ADMIN_TOKEN").ok());
    if let Some(t) = tok.as_deref() {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", t)).unwrap(),
        );
    }
    let h = client
        .get(format!("{}/healthz", base))
        .headers(headers.clone())
        .send()?;
    let ok_health = h.status().is_success();
    let a = client
        .get(format!("{}/about", base))
        .headers(headers)
        .send()?;
    let about_json: serde_json::Value = a.json().unwrap_or_else(|_| serde_json::json!({}));
    let out = serde_json::json!({
        "base": base,
        "healthz": {"status": h.status().as_u16()},
        "about": about_json,
        "ok": ok_health,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_spec_health(args: &SpecHealthArgs) -> Result<()> {
    let base = args.base.trim_end_matches('/');
    let url = format!("{}/spec/health", base);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let resp = client.get(url).send()?;
    let txt = resp.text()?;
    if args.pretty {
        let v: serde_json::Value =
            serde_json::from_str(&txt).unwrap_or_else(|_| serde_json::json!({}));
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        println!("{}", txt);
    }
    Ok(())
}
