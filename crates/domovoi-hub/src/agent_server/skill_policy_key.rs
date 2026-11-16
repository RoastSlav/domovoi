//! The settings key that stores one host's disabled skills.

/// The settings key of a host's disabled skills, like the check overrides key.
#[must_use]
pub fn host_skills_key(host: &str) -> String {
    format!("host.{host}.skills")
}
