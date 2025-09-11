use anyhow::Result;
use arw_core::{gating_keys, hello_core, introspect_tools, load_effective_paths};
use base64::Engine;
use tracing_subscriber::{fmt, EnvFilter};

fn help() {
    println!(
        "arw-cli 0.1.0\n\nUsage:\n  arw-cli paths                          Print effective paths (JSON)\n  arw-cli tools                          Print tool list (JSON)\n  arw-cli gate keys                      List known gating keys\n  arw-cli capsule template               Print minimal capsule JSON\n  arw-cli capsule gen-ed25519            Generate and print ed25519 keypair (b64)\n  arw-cli capsule sign-ed25519 <sk_b64> <capsule.json>  Sign capsule and print signature b64\n  arw-cli                                 Bootstrap info\n"
    );
}

fn main() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    // Lightweight commands
    let mut args = std::env::args().skip(1);
    if let Some(cmd) = args.next() {
        match cmd.as_str() {
            "--help" | "-h" | "help" => return help(),
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
            "gate" => {
                let sub = args.next().unwrap_or_default();
                if sub == "keys" {
                    for k in gating_keys::list() {
                        println!("{}", k);
                    }
                    return;
                }
                eprintln!("unknown 'gate' subcommand");
                std::process::exit(2);
            }
            "capsule" => {
                let sub = args.next().unwrap_or_default();
                match sub.as_str() {
                    "template" => {
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
                        println!("{}", serde_json::to_string_pretty(&tpl).unwrap());
                        return;
                    }
                    "gen-ed25519" => {
                        if let Err(e) = cmd_gen_ed25519() {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                        return;
                    }
                    "sign-ed25519" => {
                        let sk_b64 = args.next().unwrap_or_default();
                        let file = args.next().unwrap_or_default();
                        if sk_b64.is_empty() || file.is_empty() {
                            eprintln!(
                                "usage: arw-cli capsule sign-ed25519 <sk_b64> <capsule.json>"
                            );
                            std::process::exit(2);
                        }
                        if let Err(e) = cmd_sign_ed25519(&sk_b64, &file) {
                            eprintln!("{}", e);
                            std::process::exit(1);
                        }
                        return;
                    }
                    _ => {
                        eprintln!("unknown 'capsule' subcommand");
                        std::process::exit(2);
                    }
                }
            }
            _ => {}
        }
    }

    println!("arw-cli 0.1.0 — bootstrap");
    hello_core();
    println!("{}", load_effective_paths());
}

fn cmd_gen_ed25519() -> Result<()> {
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
    println!(
        "issuer=local-admin\nalg=ed25519\npubkey_b64={}\nprivkey_b64={}",
        pk_b64, sk_b64
    );
    eprintln!("Note: store private key securely; add pubkey to configs/trust_capsules.json");
    Ok(())
}

fn cmd_sign_ed25519(sk_b64: &str, capsule_file: &str) -> Result<()> {
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
    println!("{}", sig_b64);
    Ok(())
}
