#[cfg(any(test, feature = "test_support"))]
pub mod env {
    use once_cell::sync::Lazy;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    pub struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    pub fn guard() -> EnvGuard {
        EnvGuard {
            _lock: ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner()),
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

#[cfg(all(feature = "nats", any(test, feature = "test_support")))]
pub mod nats {
    use crate::orchestrator_nats::NatsQueue;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    /// Resolve a nats-server binary from env override or PATH.
    pub fn find_nats_binary() -> Option<PathBuf> {
        if let Ok(path) = std::env::var("ARW_NATS_SERVER_BIN") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        #[cfg(feature = "test_support")]
        {
            if let Ok(path) = which::which("nats-server") {
                return Some(path);
            }
        }
        #[cfg(test)]
        {
            if let Ok(path) = which::which("nats-server") {
                return Some(path);
            }
        }
        None
    }

    /// Best-effort: connect to an existing broker at `url`, optionally spawning one locally when permitted.
    pub async fn connect_or_spawn(url: &str, allow_spawn: bool) -> Option<NatsHarness> {
        if let Some(queue) = try_connect(url).await {
            return Some(NatsHarness { queue, child: None });
        }
        if !allow_spawn {
            return None;
        }
        let Some(bin) = find_nats_binary() else {
            return None;
        };
        match spawn_nats(&bin, 4222) {
            Ok(mut child) => {
                tokio::time::sleep(Duration::from_millis(600)).await;
                if let Some(queue) = try_connect(url).await {
                    Some(NatsHarness {
                        queue,
                        child: Some(child),
                    })
                } else {
                    let _ = child.kill();
                    let _ = child.wait();
                    None
                }
            }
            Err(_) => None,
        }
    }

    async fn try_connect(url: &str) -> Option<NatsQueue> {
        match tokio::time::timeout(Duration::from_secs(2), NatsQueue::connect(url)).await {
            Ok(Ok(queue)) => Some(queue),
            _ => None,
        }
    }

    fn spawn_nats(bin: &Path, port: u16) -> std::io::Result<Child> {
        Command::new(bin)
            .args(["-p", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    pub struct NatsHarness {
        pub queue: NatsQueue,
        child: Option<Child>,
    }

    impl Drop for NatsHarness {
        fn drop(&mut self) {
            if let Some(child) = &mut self.child {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}
