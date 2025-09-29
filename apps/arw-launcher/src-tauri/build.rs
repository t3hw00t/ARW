use std::collections::BTreeSet;
use std::env;
use std::error::Error;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    ensure_icon(&manifest_dir).expect("failed to prepare launcher icon");
    stage_external_bins(&manifest_dir).expect("failed to stage external binaries");

    tauri_build::build();
}

fn ensure_icon(manifest_dir: &Path) -> Result<(), Box<dyn Error>> {
    let icon_dir = manifest_dir.join("icons");
    let icon_png = icon_dir.join("icon.png");
    if fs::metadata(&icon_png).is_err() {
        fs::create_dir_all(&icon_dir)?;
        let file = fs::File::create(&icon_png)?;
        let w = BufWriter::new(file);
        let mut encoder = png::Encoder::new(w, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        let data: [u8; 4] = [255, 255, 255, 255];
        writer.write_image_data(&data)?;
    }
    Ok(())
}

fn stage_external_bins(manifest_dir: &Path) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=TAURI_ENV_TARGET_TRIPLE");
    println!("cargo:rerun-if-env-changed=CARGO_TARGET_DIR");
    println!("cargo:rerun-if-env-changed=PROFILE");

    let bin_dir = manifest_dir.join("bin");
    fs::create_dir_all(&bin_dir)?;

    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .ok_or("failed to determine workspace root")?;

    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));

    let profile = env::var("PROFILE").unwrap_or_else(|_| "release".to_string());
    let tauri_target = env::var("TAURI_ENV_TARGET_TRIPLE").ok();
    let target_triple = tauri_target
        .clone()
        .or_else(|| env::var("TARGET").ok())
        .unwrap_or_default();
    let require_bins = profile == "release"
        && (tauri_target.is_some()
            || env::var_os("TAURI_CONFIG").is_some()
            || env::var_os("ARW_REQUIRE_STAGE_BIN").is_some());

    let is_windows = target_triple.contains("windows");
    let ext = if is_windows { ".exe" } else { "" };

    let mut missing: Vec<String> = Vec::new();

    // Required binaries: unified server + CLI.
    for bin in ["arw-server", "arw-cli"] {
        let file_name = format!("{}{}", bin, ext);
        let Some(source) = locate_binary(&target_dir, &target_triple, &profile, &file_name) else {
            if require_bins {
                missing.push(file_name);
            } else {
                eprintln!(
                    "note: missing {file_name}; build `cargo build -p {bin}` before bundling to include it"
                );
            }
            continue;
        };

        copy_file(&source, &bin_dir.join(&file_name))?;

        for variant in platform_variants(&file_name, &target_triple) {
            copy_file(&source, &bin_dir.join(variant))?;
        }
    }

    if !missing.is_empty() {
        return Err(format!(
            "prebuilt binaries missing: {}. run `cargo build --release -p arw-server -p arw-cli` before packaging.",
            missing.join(", ")
        )
        .into());
    }

    Ok(())
}

fn locate_binary(
    target_dir: &Path,
    target_triple: &str,
    profile: &str,
    file_name: &str,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    candidates.push(target_dir.join(profile).join(file_name));
    if profile != "release" {
        candidates.push(target_dir.join("release").join(file_name));
    }
    if profile != "debug" {
        candidates.push(target_dir.join("debug").join(file_name));
    }

    if !target_triple.is_empty() {
        let triple_dir = target_dir.join(target_triple);
        candidates.push(triple_dir.join(profile).join(file_name));
        if profile != "release" {
            candidates.push(triple_dir.join("release").join(file_name));
        }
        if profile != "debug" {
            candidates.push(triple_dir.join("debug").join(file_name));
        }
    }

    candidates.into_iter().find(|p| p.exists())
}

fn platform_variants(file_name: &str, target_triple: &str) -> Vec<String> {
    if target_triple.is_empty() {
        return Vec::new();
    }
    let mut variants = BTreeSet::new();
    variants.insert(format!("{}-{}", file_name, target_triple));

    let path = Path::new(file_name);
    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        variants.insert(format!("{}-{}", stem, target_triple));
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            let ext_with_dot = format!(".{}", ext);
            variants.insert(format!("{}-{}{}", stem, target_triple, ext_with_dot));
            variants.insert(format!("{}-{}{}", file_name, target_triple, ext_with_dot));
        }
    }

    variants.into_iter().collect()
}

fn copy_file(src: &Path, dest: &Path) -> Result<(), Box<dyn Error>> {
    if src == dest {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dest)?;
    Ok(())
}
