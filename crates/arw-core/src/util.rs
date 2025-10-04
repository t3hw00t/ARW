/// Parse a boolean-like environment flag.
/// Accepts common values such as 1/0, true/false, yes/no, on/off (case-insensitive).
pub fn parse_bool_flag(raw: &str) -> Option<bool> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Read an environment variable and parse it as a boolean flag using [`parse_bool_flag`].
pub fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .and_then(|raw| parse_bool_flag(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bool_flag_recognizes_common_values() {
        assert_eq!(parse_bool_flag("true"), Some(true));
        assert_eq!(parse_bool_flag("YES"), Some(true));
        assert_eq!(parse_bool_flag("0"), Some(false));
        assert_eq!(parse_bool_flag("off"), Some(false));
        assert_eq!(parse_bool_flag("maybe"), None);
        assert_eq!(parse_bool_flag(""), None);
    }

    #[test]
    fn env_bool_reads_env() {
        std::env::set_var("ARW_TEST_BOOL", "on");
        assert_eq!(env_bool("ARW_TEST_BOOL"), Some(true));
        std::env::set_var("ARW_TEST_BOOL", "No");
        assert_eq!(env_bool("ARW_TEST_BOOL"), Some(false));
        std::env::remove_var("ARW_TEST_BOOL");
        assert_eq!(env_bool("ARW_TEST_BOOL"), None);
    }
}
