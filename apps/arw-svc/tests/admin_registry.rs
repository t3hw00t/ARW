use std::collections::HashSet;

#[test]
fn admin_registry_baseline() {
    // Touch a symbol from arw-svc to ensure the crate links in tests
    arw_svc::linkme();

    let list = arw_core::list_admin_endpoints();
    let have: HashSet<String> = list.into_iter().map(|e| e.path.to_string()).collect();

    // Baseline coverage: ensure key admin endpoints are discoverable.
    let expect = [
        // Only endpoints defined in lib (ext/*) modules are asserted here.
        "/admin/introspect/stats",
        "/admin/governor/profile",
        "/admin/governor/hints",
        "/admin/memory",
        "/admin/memory/apply",
        "/admin/memory/save",
        "/admin/memory/load",
        "/admin/memory/limit",
        "/admin/models",
        "/admin/models/refresh",
        "/admin/models/save",
        "/admin/models/load",
        "/admin/models/add",
        "/admin/models/delete",
        "/admin/models/default",
        "/admin/tools",
        "/admin/tools/run",
        "/admin/hierarchy/state",
        "/admin/hierarchy/role",
        "/admin/projects/list",
        "/admin/projects/create",
        "/admin/projects/tree",
        "/admin/projects/notes",
        "/admin/feedback/suggestions",
        "/admin/feedback/updates",
        "/admin/feedback/policy",
        "/admin/feedback/versions",
        "/admin/feedback/rollback",
        "/admin/feedback/state",
        "/admin/feedback/signal",
        "/admin/feedback/analyze",
        "/admin/feedback/apply",
        "/admin/feedback/auto",
        "/admin/feedback/reset",
        "/admin/chat",
        "/admin/chat/send",
        "/admin/chat/clear",
        "/admin/chat/status",
    ];

    for p in expect {
        assert!(
            have.contains(p),
            "missing admin endpoint in registry: {}",
            p
        );
    }
}
