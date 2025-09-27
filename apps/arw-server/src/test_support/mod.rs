use once_cell::sync::Lazy;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) mod env {
    use super::*;

    pub(crate) struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved: HashMap<String, Option<String>>,
    }

    pub(crate) fn guard() -> EnvGuard {
        EnvGuard {
            _lock: ENV_LOCK.lock().expect("env lock poisoned"),
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
// Acquire the state-dir guard first, then the env guard. Hold both until drop.
#[cfg(test)]
pub(crate) struct TestCtx {
    pub env: env::EnvGuard,
    // Keep state guard last so env drops first
    _state: crate::util::StateDirTestGuard,
}

#[cfg(test)]
pub(crate) fn begin_state_env(path: &std::path::Path) -> TestCtx {
    let state = crate::util::scoped_state_dir_for_tests(path);
    let env = env::guard();
    TestCtx { env, _state: state }
}
