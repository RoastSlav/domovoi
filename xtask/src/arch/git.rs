//! Change-based rules that compare against a base ref: R13 (append-only
//! migrations) and ADR 0001 (rule changes need an ADR).

use std::path::Path;
use std::process::Command;

use super::Violation;
use super::rules::{ADR_DIR, MIGRATIONS_DIR, RULES_FILE};

/// One changed path from `git diff --name-status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Status letter: A, M, D, R, C, T.
    pub status: char,
    /// Path after the change (for renames, the new path).
    pub path: String,
    /// Path before the change, for renames and copies.
    pub old_path: Option<String>,
}

/// Run the change-based checks against `base`.
pub fn check(root: &Path, base: &str) -> anyhow::Result<Vec<Violation>> {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "diff",
            "--name-status",
            "--find-renames",
            &format!("{base}...HEAD"),
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff against `{base}` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let changes = parse(&String::from_utf8_lossy(&output.stdout));
    Ok(evaluate(&changes))
}

/// Parse `git diff --name-status` output.
pub fn parse(text: &str) -> Vec<Change> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let status = parts.next()?.chars().next()?;
            let first = parts.next()?.to_string();
            match parts.next() {
                Some(second) => Some(Change {
                    status,
                    path: second.to_string(),
                    old_path: Some(first),
                }),
                None => Some(Change {
                    status,
                    path: first,
                    old_path: None,
                }),
            }
        })
        .collect()
}

/// Apply the rules to a list of changes.
pub fn evaluate(changes: &[Change]) -> Vec<Violation> {
    let mut out = Vec::new();
    for c in changes {
        let touches_migration = c.path.starts_with(MIGRATIONS_DIR)
            || c.old_path
                .as_deref()
                .is_some_and(|p| p.starts_with(MIGRATIONS_DIR));
        if touches_migration && c.status != 'A' {
            out.push(Violation {
                rule: "R13",
                location: c.path.clone(),
                message: "migrations are append-only; add a new migration instead of editing one"
                    .into(),
            });
        }
    }
    let rules_changed = changes.iter().any(|c| c.path == RULES_FILE);
    let adr_added = changes.iter().any(|c| {
        c.status == 'A'
            && c.path.starts_with(ADR_DIR)
            && c.path.ends_with(".md")
            && is_numbered(&c.path)
    });
    if rules_changed && !adr_added {
        out.push(Violation {
            rule: "ADR",
            location: RULES_FILE.into(),
            message: "architecture rules changed without a new ADR in docs/adr".into(),
        });
    }
    out
}

fn is_numbered(path: &str) -> bool {
    let file = path.rsplit('/').next().unwrap_or_default();
    let b = file.as_bytes();
    b.len() > 5
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && !file.starts_with("0000")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_status_including_renames() {
        let c = parse("M\tsrc/a.rs\nR100\told.rs\tnew.rs\n");
        assert_eq!(
            c[0],
            Change {
                status: 'M',
                path: "src/a.rs".into(),
                old_path: None
            }
        );
        assert_eq!(c[1].path, "new.rs");
        assert_eq!(c[1].old_path.as_deref(), Some("old.rs"));
    }

    #[test]
    fn editing_a_migration_is_rejected() {
        let c = parse(&format!("M\t{MIGRATIONS_DIR}0001_init.sql\n"));
        assert_eq!(evaluate(&c)[0].rule, "R13");
    }

    #[test]
    fn adding_a_migration_is_allowed() {
        let c = parse(&format!("A\t{MIGRATIONS_DIR}0002_more.sql\n"));
        assert!(evaluate(&c).is_empty());
    }

    #[test]
    fn rule_change_needs_an_adr() {
        let c = parse(&format!("M\t{RULES_FILE}\n"));
        assert_eq!(evaluate(&c)[0].rule, "ADR");
        let c = parse(&format!("M\t{RULES_FILE}\nA\tdocs/adr/0013-new-crate.md\n"));
        assert!(evaluate(&c).is_empty());
    }
}
