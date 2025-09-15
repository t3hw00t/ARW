//! Test helpers to stabilize environment-sensitive tests.

/// Set `ARW_STATE_DIR` to a unique temp dir for the scope of the returned guard.
/// Drops reset the env var to its previous value.
pub fn scoped_state_dir() -> (tempfile::TempDir, ScopedEnv) {
    let dir = tempfile::tempdir().expect("tempdir");
    let prev = std::env::var("ARW_STATE_DIR").ok();
    std::env::set_var("ARW_STATE_DIR", dir.path().to_string_lossy().to_string());
    (
        dir,
        ScopedEnv {
            key: "ARW_STATE_DIR",
            prev,
        },
    )
}

pub struct ScopedEnv {
    key: &'static str,
    prev: Option<String>,
}
impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
