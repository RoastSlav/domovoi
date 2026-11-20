//! Shape rules for `[packages]`. Keys are only checked for shape here; the agent decodes
//! them and refuses to start on a key that does not match its id.

use crate::issue::ConfigIssue;
use crate::packages::PackagesSection;

fn is_key_id(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_base64_key(text: &str) -> bool {
    text.len() == 44
        && text.ends_with('=')
        && text
            .bytes()
            .take(43)
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
}

fn out_of_range(issues: &mut Vec<ConfigIssue>, field: &str, range: &str) {
    issues.push(ConfigIssue::error(
        format!("packages.limits.{field}"),
        format!("must be {range}"),
    ));
}

/// Pushes every problem in `packages` onto `issues`.
pub(crate) fn package_rules(packages: &PackagesSection, issues: &mut Vec<ConfigIssue>) {
    let mut seen = std::collections::BTreeSet::new();
    for (index, key) in packages.trusted_keys.iter().enumerate() {
        let path = format!("packages.trusted_keys[{index}]");
        if !is_key_id(&key.id) {
            issues.push(ConfigIssue::error(
                format!("{path}.id"),
                "must be 16 lowercase hex characters",
            ));
        } else if !seen.insert(key.id.as_str()) {
            issues.push(ConfigIssue::error(
                format!("{path}.id"),
                "is listed twice; each trusted key id must be unique",
            ));
        }
        if !is_base64_key(&key.public_key) {
            issues.push(ConfigIssue::error(
                format!("{path}.public_key"),
                "must be 32 bytes in standard padded base64 (44 characters)",
            ));
        }
        if let Some(label) = &key.label
            && (label.chars().count() > 64 || label.chars().any(char::is_control))
        {
            issues.push(ConfigIssue::error(
                format!("{path}.label"),
                "must be at most 64 characters with no control characters",
            ));
        }
    }
    for (index, id) in packages.revoked.iter().enumerate() {
        if !is_key_id(id) {
            issues.push(ConfigIssue::error(
                format!("packages.revoked[{index}]"),
                "must be 16 lowercase hex characters",
            ));
        } else if seen.contains(id.as_str()) {
            issues.push(ConfigIssue::warning(
                format!("packages.revoked[{index}]"),
                "is also a trusted key; revoked wins and its packages will not verify",
            ));
        }
    }
    if packages.allow_install && packages.trusted_keys.is_empty() {
        issues.push(ConfigIssue::warning(
            "packages.allow_install",
            "is true but no trusted_keys are set, so no package can verify",
        ));
    }
    limit_rules(packages, issues);
}

fn limit_rules(packages: &PackagesSection, issues: &mut Vec<ConfigIssue>) {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    let limits = &packages.limits;
    if !(KIB..=256 * MIB).contains(&limits.compressed_bytes) {
        out_of_range(issues, "compressed_bytes", "1 KiB to 256 MiB");
    }
    if !(KIB..=1024 * MIB).contains(&limits.uncompressed_bytes) {
        out_of_range(issues, "uncompressed_bytes", "1 KiB to 1 GiB");
    }
    if limits.file_bytes < 1 || limits.file_bytes > limits.uncompressed_bytes {
        out_of_range(
            issues,
            "file_bytes",
            "at least 1 and at most uncompressed_bytes",
        );
    }
    if !(1..=20_000).contains(&limits.entries) {
        out_of_range(issues, "entries", "1 to 20000");
    }
    if !(2..=32).contains(&limits.path_depth) {
        out_of_range(issues, "path_depth", "2 to 32");
    }
    if !(16..=1024).contains(&limits.path_bytes) {
        out_of_range(issues, "path_bytes", "16 to 1024");
    }
    let secs = limits.extraction_time.as_signed().as_secs();
    if !(1..=15 * 60).contains(&secs) {
        out_of_range(issues, "extraction_time", "1 second to 15 minutes");
    }
}
