use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashMap;

// In tests, prefer parking_lot's fast mutexes with timed try-locks
#[cfg(test)]
use parking_lot::{Mutex, MutexGuard};

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) mod env {
    use super::*;

    pub(crate) struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved: HashMap<String, Option<String>>,
    }

    pub(crate) fn guard() -> EnvGuard {
        // Avoid indefinite hangs if another test leaked the ENV lock.
        // Wait up to 10s, then panic with a clear message.
        #[cfg(test)]
        {
            if let Some(lock) = ENV_LOCK.try_lock_for(std::time::Duration::from_secs(10)) {
                return EnvGuard {
                    _lock: lock,
                    saved: HashMap::new(),
                };
            }
            panic!("test ENV lock could not be acquired within 10s; another test may be stuck while holding it");
        }
        #[allow(unreachable_code)]
        EnvGuard {
            _lock: ENV_LOCK.lock(),
            saved: HashMap::new(),
        }
    }

    impl EnvGuard {
        fn remember(&mut self, key: &str) {
            self.saved
                .entry(key.to_string())
                .or_insert_with(|| std::env::var(key).ok());
        }

        pub(crate) fn set(&mut self, key: &str, value: impl AsRef<str>) {
            self.remember(key);
            std::env::set_var(key, value.as_ref());
        }

        pub(crate) fn set_opt(&mut self, key: &str, value: Option<&str>) {
            self.remember(key);
            match value {
                Some(val) => std::env::set_var(key, val),
                None => std::env::remove_var(key),
            }
        }

        pub(crate) fn remove(&mut self, key: &str) {
            self.set_opt(key, None);
        }

        pub(crate) fn apply<'a, I>(&mut self, vars: I)
        where
            I: IntoIterator<Item = (&'a str, Option<&'a str>)>,
        {
            for (key, value) in vars {
                self.set_opt(key, value);
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain() {
                match value {
                    Some(val) => std::env::set_var(&key, val),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}

// Unified test context helper to streamline correct lock ordering and cleanup.
// Acquire the env guard first (to avoid races), then scope the state-dir guard.
#[cfg(test)]
pub(crate) struct TestCtx {
    pub env: env::EnvGuard,
    // Keep state guard last so env drops first
    _state: crate::util::StateDirTestGuard,
}

#[cfg(test)]
pub(crate) fn begin_state_env(path: &std::path::Path) -> TestCtx {
    let mut env = env::guard();
    let state = crate::util::scoped_state_dir_for_tests(path, &mut env);
    TestCtx { env, _state: state }
}

#[cfg(test)]
pub(crate) async fn build_state(
    path: &std::path::Path,
    env_guard: &mut env::EnvGuard,
) -> crate::AppState {
    env_guard.set("ARW_DEBUG", "1");
    crate::util::reset_state_dir_for_tests();
    env_guard.set("ARW_STATE_DIR", path.display().to_string());
    let bus = arw_events::Bus::new_with_replay(64, 64);
    let kernel = arw_kernel::Kernel::open(path).expect("init kernel for tests");
    let policy = arw_policy::PolicyEngine::load_from_env();
    let policy_handle = crate::policy::PolicyHandle::new(policy, bus.clone());
    let host: std::sync::Arc<dyn arw_wasi::ToolHost> = std::sync::Arc::new(arw_wasi::NoopHost);
    crate::AppState::builder(bus, kernel, policy_handle, host, true)
        .with_sse_capacity(64)
        .build()
        .await
}

// One-time tracing init for tests, honoring RUST_LOG if set.
pub(crate) fn init_tracing() {
    static START: OnceCell<()> = OnceCell::new();
    START.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
    });
}
