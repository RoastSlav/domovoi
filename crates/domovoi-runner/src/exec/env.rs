//! The environment a sandboxed process receives.

use std::collections::BTreeMap;

/// `PATH` every process gets.
pub const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
/// `LANG` every process gets.
pub const DEFAULT_LANG: &str = "C.UTF-8";
/// `HOME` every process gets: a directory that does not exist.
pub const DEFAULT_HOME: &str = "/nonexistent";

/// `PATH`, `LANG` and `HOME` with their default values.
#[must_use]
pub fn default_base_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_owned(), DEFAULT_HOME.to_owned()),
        ("LANG".to_owned(), DEFAULT_LANG.to_owned()),
        ("PATH".to_owned(), DEFAULT_PATH.to_owned()),
    ])
}

/// Whether `key` may be passed through from a spec: `^[A-Z][A-Z0-9_]*$` and not a
/// dynamic-loader variable (`LD_*`).
#[must_use]
pub fn is_allowed_env_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    let first_ok = bytes.next().is_some_and(|b| b.is_ascii_uppercase());
    first_ok
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !key.starts_with("LD_")
}

/// The base environment plus allowed `spec_env` keys; base keys win.
pub(super) fn build(
    base: &BTreeMap<String, String>,
    spec_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = base.clone();
    for (key, value) in spec_env {
        if is_allowed_env_key(key) && !base.contains_key(key) {
            env.insert(key.clone(), value.clone());
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_keys_are_filtered() {
        for ok in ["A", "DOMOVOI_PARAM_X", "X1_2"] {
            assert!(is_allowed_env_key(ok), "{ok}");
        }
        for bad in [
            "",
            "a",
            "1A",
            "A-B",
            "A B",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "Ä",
        ] {
            assert!(!is_allowed_env_key(bad), "{bad}");
        }
    }

    #[test]
    fn base_keys_cannot_be_overridden() {
        let spec = BTreeMap::from([
            ("PATH".to_owned(), "/tmp".to_owned()),
            ("DOMOVOI_SKILL".to_owned(), "x".to_owned()),
            ("bad".to_owned(), "y".to_owned()),
        ]);
        let env = build(&default_base_env(), &spec);
        assert_eq!(env["PATH"], DEFAULT_PATH);
        assert_eq!(env["DOMOVOI_SKILL"], "x");
        assert!(!env.contains_key("bad"));
        assert_eq!(env.len(), 4);
    }
}
