pub fn kernel_enabled_from_env() -> bool {
    std::env::var("ARW_KERNEL_ENABLE")
        .map(|v| {
            let trimmed = v.trim();
            !(trimmed.eq_ignore_ascii_case("0") || trimmed.eq_ignore_ascii_case("false"))
        })
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::kernel_enabled_from_env;
    use once_cell::sync::Lazy;
    use std::{env, sync::Mutex};

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn default_true() {
        let _guard = ENV_LOCK.lock().unwrap();
        env::remove_var("ARW_KERNEL_ENABLE");
        assert!(kernel_enabled_from_env());
    }

    #[test]
    fn disabled_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        for value in ["0", "false", "False", "FALSE", " 0 ", " false "] {
            env::set_var("ARW_KERNEL_ENABLE", value);
            assert!(!kernel_enabled_from_env(), "value {value:?}");
        }
        env::remove_var("ARW_KERNEL_ENABLE");
    }

    #[test]
    fn enabled_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        for value in ["1", "true", "YES"] {
            env::set_var("ARW_KERNEL_ENABLE", value);
            assert!(kernel_enabled_from_env(), "value {value:?}");
        }
        env::remove_var("ARW_KERNEL_ENABLE");
    }
}
