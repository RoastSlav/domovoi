//! Source scans: R3 (process spawning), R4 (no shell), R2 (pure crates), R8 (file size),
//! R10 (crate root docs), INV-1 (approval call sites), and the entry point for the
//! syntax-aware EXEC boundary checks in [`super::ast`].
//!
//! The rules left here are inherently local to one line of source (a call to a constructor, a
//! vocabulary word, a shell literal), so they read the text. The EXEC boundary rules count
//! syntax nodes instead: a line-based scan cannot see a call split across lines, a macro body
//! or an alias, and these rules protect the execution boundary.

use std::collections::BTreeSet;
use std::path::Path;

use cargo_metadata::{Metadata, TargetKind};

use super::Violation;
use super::rules::{
    AUTHORIZATION_HUMAN_ALLOWED_PREFIXES, AUTHORIZATION_MODULE,
    AUTHORIZATION_POLICY_ALLOWED_PREFIXES, HUMAN_APPROVAL_ALLOWED_PREFIXES,
    HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES, MAX_FILE_LINES, PLAN_HASH_V2_ALLOWED_PREFIXES,
    POLICY_PERMIT_ALLOWED_PREFIXES, PROCESS_ALLOWED_PREFIXES, PURE_CRATE_DIRS,
    PURE_FORBIDDEN_PATHS,
};

/// Scan every `.rs` file under `crates/` and check every rule.
///
/// Test-only code is told apart by module wiring and Cargo's test targets, not by filename:
/// `test_only_files` follows the test targets and `#[cfg(test)]` declarations, and everything
/// they do not reach is production, whatever it is called.
pub fn check_tree(root: &Path, metadata: &Metadata) -> anyhow::Result<Vec<Violation>> {
    let mut paths = Vec::new();
    collect_rs(&root.join("crates"), &mut paths)?;
    paths.sort();

    let mut files: Vec<(String, String)> = Vec::with_capacity(paths.len());
    for path in paths {
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path)?;
        files.push((rel, text));
    }

    let roots = test_target_roots(metadata, root);
    let test_only = super::ast::test_only_files(&files, &roots);

    let mut out = Vec::new();
    for (rel, text) in &files {
        out.extend(check_file(rel, text, test_only.contains(rel)));
    }
    let production: Vec<(String, String)> = files
        .iter()
        .filter(|(rel, _)| !test_only.contains(rel))
        .cloned()
        .collect();
    out.extend(super::ast::exec_violations(&production));
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

/// The roots Cargo compiles as test targets, workspace-relative with `/` separators.
fn test_target_roots(metadata: &Metadata, root: &Path) -> BTreeSet<String> {
    let workspace = metadata.workspace_root.as_std_path();
    let mut roots = BTreeSet::new();
    for package in &metadata.packages {
        for target in &package.targets {
            if !target.kind.contains(&TargetKind::Test) {
                continue;
            }
            let src = target.src_path.as_std_path();
            let rel = src
                .strip_prefix(workspace)
                .or_else(|_| src.strip_prefix(root));
            if let Ok(rel) = rel {
                roots.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    roots
}

/// Whether a path allowlist entry covers `rel`: an entry ending in `/` is a directory scope,
/// any other entry names one file exactly, so a sibling file cannot inherit the exception.
fn path_matches(rel: &str, allowed: &str) -> bool {
    if allowed.ends_with('/') {
        rel.starts_with(allowed)
    } else {
        rel == allowed
    }
}

/// Whether `line` is inside a comment: a rule never counts a mention in prose.
///
/// This is the one heuristic left, and only for the line rules: they read text, and a
/// doc comment mentioning `HumanApproval::new` or `std::fs` must not count.
fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// Check one file's line rules. `rel` is the path relative to the workspace root, with `/`
/// separators; `test_only` says whether only test builds compile it.
pub fn check_file(rel: &str, text: &str, test_only: bool) -> Vec<Violation> {
    let mut out = Vec::new();
    let in_runner = PROCESS_ALLOWED_PREFIXES
        .iter()
        .any(|p| path_matches(rel, p));
    let approval_ok = test_only
        || HUMAN_APPROVAL_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let restore_ok = test_only
        || HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let permit_ok = test_only
        || POLICY_PERMIT_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let authorization_human_ok = test_only
        || AUTHORIZATION_HUMAN_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let authorization_policy_ok = test_only
        || AUTHORIZATION_POLICY_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let plan_v2_ok = test_only
        || PLAN_HASH_V2_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let pure = !test_only && PURE_CRATE_DIRS.iter().any(|p| rel.starts_with(p));

    for (i, line) in text.lines().enumerate() {
        let at = format!("{rel}:{}", i + 1);
        if is_comment(line) {
            continue;
        }
        let spawns = line.contains("std::process::Command")
            || line.contains("tokio::process::Command")
            || line.contains("use std::process")
            || line.contains("use tokio::process");
        if spawns && !in_runner && !test_only {
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
        if !test_only && is_shell_invocation(line) {
            out.push(v(
                "R4",
                &at,
                "never invoke a shell; build an argv vector from validated parameters",
            ));
        }
        if line.contains("HumanApproval::restore(") && !restore_ok {
            out.push(v(
                "INV-1",
                &at,
                "HumanApproval::restore may only be called in crates/domovoi-store/src/",
            ));
        }
        if line.contains("HumanApproval::new(") && !approval_ok {
            out.push(v(
                "INV-1",
                &at,
                "HumanApproval may only be constructed in crates/domovoi-hub/src/auth/",
            ));
        }
        if line.contains("PolicyPermit::issue(") && !permit_ok {
            out.push(v(
                "INV-1",
                &at,
                "PolicyPermit::issue may only be called in crates/domovoi-hub/src/policy/",
            ));
        }
        if line.contains("Authorization::human(") && !authorization_human_ok {
            out.push(v(
                "INV-1",
                &at,
                "Authorization::human may only be called in crates/domovoi-hub/src/auth/",
            ));
        }
        if line.contains("Authorization::policy(") && !authorization_policy_ok {
            out.push(v(
                "INV-1",
                &at,
                "Authorization::policy may only be called in crates/domovoi-hub/src/policy/",
            ));
        }
        if line.contains("plan:v2") && !plan_v2_ok {
            out.push(v(
                "INV-1",
                &at,
                "the plan:v2 domain line has one implementation, in crates/domovoi-core/src/action/",
            ));
        }
        if (line.contains("pub enum Authorization {") || line.contains("pub enum Authorization{"))
            && rel != AUTHORIZATION_MODULE
        {
            out.push(v(
                "INV-1",
                &at,
                "Authorization is a wrapper with private internals; do not make it a public enum",
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
    if !test_only && rel.contains("/src/") && lines > MAX_FILE_LINES {
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
#[path = "scan_tests.rs"]
mod tests;
