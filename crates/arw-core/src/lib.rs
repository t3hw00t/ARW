use std::{env, fs, path::{Path, PathBuf}};
use tracing::info;
use serde::Deserialize;
use regex::Regex;

#[derive(Deserialize, Default, Debug)]
struct RuntimeCfg {
    portable: Option<bool>,
    state_dir: Option<String>,
    cache_dir: Option<String>,
    logs_dir: Option<String>
}

#[derive(Deserialize, Default, Debug)]
struct RootCfg {
    runtime: Option<RuntimeCfg>
}

/// Holds the effective runtime directories after config/env resolution.
#[derive(Debug, Clone)]
pub struct EffectivePaths {
    pub portable: bool,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf
}

/// Simple env expander for Windows-style %VAR% tokens.
fn expand_env(input: &str) -> String {
    let re = Regex::new(r"%([A-Za-z0-9_]+)%").unwrap();
    re.replace_all(input, |caps: &regex::Captures| {
        env::var(&caps[1]).unwrap_or_default()
    }).to_string()
}

fn resolve_relative(base: &Path, p: &str) -> PathBuf {
    let s = expand_env(p);
    let pb = PathBuf::from(s);
    if pb.is_absolute() { pb } else { base.join(pb) }
}

fn default_localappdata() -> PathBuf {
    if let Ok(v) = env::var("LOCALAPPDATA") {
        return PathBuf::from(v);
    }
    if let Ok(u) = env::var("USERPROFILE") {
        return PathBuf::from(u).join("AppData").join("Local");
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn find_config_file() -> Option<PathBuf> {
    if let Ok(p) = env::var("ARW_CONFIG") {
        let pb = PathBuf::from(p);
        if pb.exists() { return Some(pb); }
    }
    let cwd = env::current_dir().ok()?;
    let a = cwd.join("configs").join("default.toml");
    if a.exists() { return Some(a); }
    let b = PathBuf::from(env::var("USERPROFILE").ok()?)
        .join("arw").join("configs").join("default.toml");
    if b.exists() { return Some(b); }
    None
}

fn load_root_cfg(path: &Path) -> RootCfg {
    match fs::read_to_string(path) {
        Ok(s) => toml::from_str::<RootCfg>(&s).unwrap_or_default(),
        Err(_) => RootCfg::default()
    }
}

/// Compute effective runtime paths. Portable defaults to true when config is missing.
pub fn load_effective_paths() -> EffectivePaths {
    let cfg_path = find_config_file();
    let (base_dir, cfg) = if let Some(ref p) = cfg_path {
        let base = p.parent().unwrap_or_else(|| Path::new("."));
        (base.to_path_buf(), load_root_cfg(p))
    } else {
        (env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), RootCfg::default())
    };

    let rc = cfg.runtime.unwrap_or_default();
    let portable = rc.portable.unwrap_or(true);

    let default_state = default_localappdata().join("arw");
    let state_dir = rc.state_dir
        .as_deref()
        .map(|p| resolve_relative(&base_dir, p))
        .unwrap_or(default_state.clone());

    let cache_dir = rc.cache_dir
        .as_deref()
        .map(|p| resolve_relative(&base_dir, p))
        .unwrap_or(state_dir.join("cache"));

    let logs_dir = rc.logs_dir
        .as_deref()
        .map(|p| resolve_relative(&base_dir, p))
        .unwrap_or(state_dir.join("logs"));

    EffectivePaths { portable, state_dir, cache_dir, logs_dir }
}

pub fn ensure_dirs(ep: &EffectivePaths) {
    for d in [&ep.state_dir, &ep.cache_dir, &ep.logs_dir] {
        if let Err(e) = fs::create_dir_all(d) {
            eprintln!("warn: failed to create {}: {}", d.display(), e);
        }
    }
}

pub fn hello_core() {
    info!("arw-core: hello");
}

pub fn print_effective_paths() {
    let ep = load_effective_paths();
    ensure_dirs(&ep);
    println!("ARW runtime:");
    println!("  portable : {}", ep.portable);
    println!("  state_dir: {}", ep.state_dir.display());
    println!("  cache_dir: {}", ep.cache_dir.display());
    println!("  logs_dir : {}", ep.logs_dir.display());
}

use serde::Serialize;

/// Minimal tool descriptor for introspection.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub id: String,
    pub version: String,
    pub summary: String,
    pub stability: String,
    pub capabilities: Vec<String>
}

/// Return the current catalog of tools (stubbed for now).
pub fn introspect_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            id: "memory.probe".to_string(),
            version: "1.0.0".to_string(),
            summary: "Read-only memory probe (shows applied memories and paths)".to_string(),
            stability: "experimental".to_string(),
            capabilities: vec!["read-only".to_string()]
        },
        ToolInfo {
            id: "introspect.tools".to_string(),
            version: "1.0.0".to_string(),
            summary: "List available tools with metadata".to_string(),
            stability: "experimental".to_string(),
            capabilities: vec!["read-only".to_string()]
        }
    ]
}

