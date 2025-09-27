#[cfg(test)]
pub mod env {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    pub fn guard() -> EnvGuard {
        let lk = ENV_LOCK.get_or_init(|| Mutex::new(()));
        EnvGuard {
            _lock: lk.lock().expect("env lock"),
            saved: Vec::new(),
        }
    }

    impl EnvGuard {
        fn remember(&mut self, key: &'static str) {
            if self.saved.iter().any(|(k, _)| *k == key) {
                return;
            }
            self.saved.push((key, std::env::var(key).ok()));
        }
        pub fn set(&mut self, key: &'static str, value: &str) {
            self.remember(key);
            std::env::set_var(key, value);
        }
        pub fn remove(&mut self, key: &'static str) {
            self.remember(key);
            std::env::remove_var(key);
        }
        pub fn clear_keys(&mut self, keys: &[&'static str]) {
            for &k in keys {
                self.remove(k);
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, val) in self.saved.drain(..) {
                match val {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

