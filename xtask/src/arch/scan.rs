//! Source scans: R3 (process spawning), R4 (no shell), R2 (pure crates),
//! R8 (file size), R10 (crate root docs), INV-1 (approval call sites).

use std::path::Path;

use super::Violation;
use super::rules::{
    HUMAN_APPROVAL_ALLOWED_PREFIXES, MAX_FILE_LINES, PROCESS_ALLOWED_PREFIXES, PURE_CRATE_DIRS,
    PURE_FORBIDDEN_PATHS,
};

/// Scan every `.rs` file under `crates/`.
pub fn check_tree(root: &Path) -> anyhow::Result<Vec<Violation>> {
    let mut files = Vec::new();
    collect_rs(&root.join("crates"), &mut files)?;
    files.sort();
    let mut out = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path)?;
        out.extend(check_file(&rel, &text));
    }
    Ok(out)
}

fn collect_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                collect_rs(&path, out)?;
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Whether a path is test-only code (integration tests or `*_tests.rs` / `tests.rs` modules).
fn is_test_path(rel: &str) -> bool {
    rel.contains("/tests/") || rel.ends_with("/tests.rs") || rel.ends_with("_tests.rs")
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// Check one file. `rel` is the path relative to the workspace root, with `/` separators.
pub fn check_file(rel: &str, text: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let test = is_test_path(rel);
    let in_runner = PROCESS_ALLOWED_PREFIXES.iter().any(|p| rel.starts_with(p));
    let approval_ok = test
        || HUMAN_APPROVAL_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let pure = !test && PURE_CRATE_DIRS.iter().any(|p| rel.starts_with(p));

    for (i, line) in text.lines().enumerate() {
        let at = format!("{rel}:{}", i + 1);
        if is_comment(line) {
            continue;
        }
        let spawns = line.contains("std::process::Command")
            || line.contains("tokio::process::Command")
            || line.contains("use std::process")
            || line.contains("use tokio::process");
        if spawns && !in_runner && !test {
            out.push(v(
                "R3",
                &at,
                "only domovoi-runner may spawn processes; use the ProcessExecutor port",
            ));
        }
        if line.contains("allow(clippy::disallowed_methods)") && !in_runner {
            out.push(v(
                "R3",
                &at,
                "`allow(clippy::disallowed_methods)` is only permitted in domovoi-runner",
            ));
        }
        if !test && is_shell_invocation(line) {
            out.push(v(
                "R4",
                &at,
                "never invoke a shell; build an argv vector from validated parameters",
            ));
        }
        if line.contains("HumanApproval::new(") && !approval_ok {
            out.push(v(
                "INV-1",
                &at,
                "HumanApproval may only be constructed in crates/domovoi-hub/src/auth/",
            ));
        }
        if pure {
            for forbidden in PURE_FORBIDDEN_PATHS {
                if line.contains(forbidden) {
                    out.push(v(
                        "R2",
                        &at,
                        &format!("pure crates must not use `{forbidden}`"),
                    ));
                }
            }
        }
    }

    let lines = text.lines().count();
    if !test && rel.contains("/src/") && lines > MAX_FILE_LINES {
        out.push(v(
            "R8",
            rel,
            &format!("{lines} lines; split the module (limit {MAX_FILE_LINES})"),
        ));
    }

    let is_root = rel.ends_with("/src/lib.rs") || rel.ends_with("/src/main.rs");
    if is_root {
        let first = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        if !first.starts_with("//!") {
            out.push(v(
                "R10",
                rel,
                "crate root must start with a `//!` doc describing the crate's role",
            ));
        }
    }
    out
}

fn is_shell_invocation(line: &str) -> bool {
    let shells = [
        "\"sh\"",
        "\"bash\"",
        "\"/bin/sh\"",
        "\"/bin/bash\"",
        "\"zsh\"",
    ];
    let names_shell = shells.iter().any(|s| line.contains(s));
    line.contains("\"/bin/sh\"")
        || line.contains("\"/bin/bash\"")
        || (names_shell && line.contains("\"-c\""))
}

fn v(rule: &'static str, location: &str, message: &str) -> Violation {
    Violation {
        rule,
        location: location.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "//! Crate doc.\n";

    #[test]
    fn process_spawn_outside_runner_is_rejected() {
        let text = format!("{DOC}use std::process::Command;\n");
        let v = check_file("crates/domovoi-hub/src/lib.rs", &text);
        assert!(v.iter().any(|x| x.rule == "R3"));
    }

    #[test]
    fn process_spawn_in_runner_is_allowed() {
        let text = format!("{DOC}use std::process::Command;\n");
        assert!(check_file("crates/domovoi-runner/src/lib.rs", &text).is_empty());
    }

    #[test]
    fn shell_invocation_is_rejected() {
        let text = format!("{DOC}let argv = vec![\"sh\", \"-c\", cmd];\n");
        let v = check_file("crates/domovoi-runner/src/exec.rs", &text);
        assert!(v.iter().any(|x| x.rule == "R4"));
    }

    #[test]
    fn human_approval_outside_auth_is_rejected() {
        let text = "let a = HumanApproval::new(user);\n";
        let v = check_file("crates/domovoi-engine/src/approvals.rs", text);
        assert_eq!(v[0].rule, "INV-1");
        assert!(check_file("crates/domovoi-hub/src/auth/session.rs", text).is_empty());
        assert!(check_file("crates/domovoi-engine/tests/gates.rs", text).is_empty());
    }

    #[test]
    fn pure_crate_io_is_rejected() {
        let text = "let f = std::fs::read(\"x\");\n";
        assert_eq!(
            check_file("crates/domovoi-core/src/incident.rs", text)[0].rule,
            "R2"
        );
        assert!(check_file("crates/domovoi-store/src/db.rs", text).is_empty());
    }

    #[test]
    fn long_file_is_rejected_but_tests_are_exempt() {
        let text = "fn a() {}\n".repeat(MAX_FILE_LINES + 1);
        assert_eq!(
            check_file("crates/domovoi-engine/src/big.rs", &text)[0].rule,
            "R8"
        );
        assert!(check_file("crates/domovoi-engine/tests/big.rs", &text).is_empty());
    }

    #[test]
    fn crate_root_without_doc_is_rejected() {
        assert_eq!(
            check_file("crates/domovoi-core/src/lib.rs", "pub mod a;\n")[0].rule,
            "R10"
        );
    }

    #[test]
    fn comments_are_ignored() {
        let text = "// Never call std::process::Command here.\n";
        assert!(check_file("crates/domovoi-engine/src/a.rs", text).is_empty());
    }
}
