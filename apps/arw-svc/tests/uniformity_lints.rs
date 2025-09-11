use std::fs;
use std::path::Path;

fn read(path: &Path) -> String { fs::read_to_string(path).expect("read file") }

#[test]
fn no_new_ad_hoc_ok_json_in_ext_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ext");
    let mut offenders: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() { continue; }
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // Allowlist legacy files that still carry ad-hoc payloads; keep them stable
        let allowlist = ["mod.rs", "projects.rs", "hierarchy_api.rs", "chat.rs"]; // legacy
        let is_legacy = allowlist.iter().any(|f| name == *f);
        let content = read(p);
        let needle = "Json(json!({\"ok\"";
        if content.contains(needle) && !is_legacy {
            offenders.push(p.display().to_string());
        }
    }
    assert!(offenders.is_empty(), "New ad-hoc ok-json detected in: {:?}", offenders);
}

#[test]
fn no_new_ad_hoc_error_json_in_ext_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ext");
    let mut offenders: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() { continue; }
        let p = entry.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // Allowlist legacy files
        let allowlist = ["mod.rs", "projects.rs"]; // legacy
        let is_legacy = allowlist.iter().any(|f| name == *f);
        let c = read(p);
        // crude detection of StatusCode + ad-hoc error payload
        let has_status = c.contains("StatusCode::BAD_REQUEST")
            || c.contains("StatusCode::INTERNAL_SERVER_ERROR")
            || c.contains("StatusCode::NOT_FOUND")
            || c.contains("StatusCode::FORBIDDEN");
        let has_ad_hoc_err = c.contains("\"error\"") || c.contains("\"reason\"");
        if has_status && has_ad_hoc_err && !is_legacy {
            offenders.push(p.display().to_string());
        }
    }
    assert!(offenders.is_empty(), "New ad-hoc error-json detected in: {:?}", offenders);
}

