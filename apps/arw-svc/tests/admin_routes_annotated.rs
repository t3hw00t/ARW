use std::collections::HashSet;

// Parse ext/mod.rs route registrations and ensure each one is present
// in the admin endpoint registry with "/admin" prefix via #[arw_admin].
#[test]
fn all_ext_routes_have_admin_registry_entries() {
    // Force linking so macros from this crate are loaded
    arw_svc::linkme();

    let src = include_str!("../src/ext/mod.rs");
    let mut paths: Vec<String> = Vec::new();

    for line in src.lines() {
        let line = line.trim();
        if let Some(i) = line.find(".route(\"") {
            let rest = &line[i + 8..];
            if let Some(j) = rest.find('\"') {
                let p = &rest[..j];
                // Skip empty or clearly non-admin placeholders
                if p.is_empty() {
                    continue;
                }
                // We build under /admin, so prefix unless already an admin path
                let full = if p.starts_with("/admin/") {
                    p.to_string()
                } else if p.starts_with('/') {
                    format!("/admin{}", p)
                } else {
                    format!("/admin/{}", p)
                };
                paths.push(full);
            }
        }
    }

    // Dedup
    paths.sort();
    paths.dedup();

    // Gather registered admin endpoints
    let have: HashSet<String> = arw_core::list_admin_endpoints()
        .into_iter()
        .map(|e| e.path.to_string())
        .collect();

    // Ensure every ext route appears in registry
    for p in paths {
        assert!(
            have.contains(&p),
            "missing #[arw_admin] registry entry for: {}",
            p
        );
    }
}
