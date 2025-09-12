use std::fs;
use std::path::Path;

fn read(path: &Path) -> String {
    fs::read_to_string(path).expect("read file")
}

#[test]
fn no_new_ad_hoc_ok_json_in_ext_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ext");
    let mut offenders: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        // No allowlist; all ext modules must use ok() / ApiError
        let content = read(p);
        let needle = "Json(json!({\"ok\"";
        if content.contains(needle) {
            offenders.push(p.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "New ad-hoc ok-json detected in: {:?}",
        offenders
    );
}

#[test]
fn no_new_ad_hoc_error_json_in_ext_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ext");
    let mut offenders: Vec<String> = Vec::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let c = read(p);
        let needle = "Json(json!({";
        let mut idx = 0usize;
        while let Some(pos) = c[idx..].find(needle) {
            let start = idx + pos;
            let end = (start + 512).min(c.len());
            let window = &c[start..end];
            if window.contains("\"error\"") || window.contains("\"reason\"") {
                offenders.push(p.display().to_string());
                break;
            }
            idx = start + needle.len();
        }
    }
    assert!(
        offenders.is_empty(),
        "New ad-hoc error-json detected in: {:?}",
        offenders
    );
}
