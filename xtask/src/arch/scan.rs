//! Source scans: R3 (process spawning), R4 (no shell), R2 (pure crates), R8 (file size),
//! R10 (crate root docs), INV-1 (approval call sites), and the entry point for the
//! syntax-aware EXEC boundary checks in [`super::ast`].
//!
//! The rules left here are inherently local to one line of source (a call to a constructor, a
//! vocabulary word, a shell literal), so they read the text, through the lexer in
//! [`super::lex`]: comments and string contents are blanked before a rule looks for code, and
//! only comments are blanked before a rule looks for a string. The EXEC boundary rules count
//! syntax nodes instead: a line-based scan cannot see a call split across lines, a macro body
//! or an alias, and these rules protect the execution boundary.

use std::collections::BTreeMap;
use std::path::Path;

use cargo_metadata::{Metadata, TargetKind};

use super::Violation;
use super::lex::lex;
use super::reach::{DiskSource, Roots, explore};
use super::rules::{
    AUTHORIZATION_HUMAN_ALLOWED_PREFIXES, AUTHORIZATION_MODULE,
    AUTHORIZATION_POLICY_ALLOWED_PREFIXES, HUMAN_APPROVAL_ALLOWED_PREFIXES,
    HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES, MAX_FILE_LINES, PLAN_HASH_V2_ALLOWED_PREFIXES,
    POLICY_PERMIT_ALLOWED_PREFIXES, PROCESS_ALLOWED_PREFIXES, PURE_CRATE_DIRS,
    PURE_FORBIDDEN_PATHS,
};

/// Scan every `.rs` file under `crates/`, and every Rust file those files load from elsewhere
/// in the workspace, and check every rule.
///
/// Which files only tests compile is decided by module wiring and Cargo targets, never by
/// name: [`explore`] follows `mod`, `#[path]` and `include!` from every target root in two
/// contexts, and a file is test-only only when the test context alone reaches it. A file that
/// is not reached at all is production, and so is one both contexts reach.
///
/// Target kinds: `test` targets are test roots. Libraries, binaries, build scripts, examples
/// and benches are production roots: they are not exempt, because they ship in the repository
/// and a production module can load their files. Rust that production wiring loads from
/// outside `crates/` is read and scanned like the rest; a path that leaves the workspace or
/// enters `target/`, a missing file or an `include!` argument that is not a literal is an
/// SRC-1 violation naming the declaring file and line.
pub fn check_tree(root: &Path, metadata: &Metadata) -> anyhow::Result<Vec<Violation>> {
    let mut paths = Vec::new();
    collect_rs(&root.join("crates"), &mut paths)?;
    paths.sort();

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for path in paths {
        let rel = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path)?;
        files.insert(rel, text);
    }

    let mut source = DiskSource::new(root)?;
    let reach = explore(&mut source, &target_roots(metadata, root));
    for (rel, text) in &reach.texts {
        files.entry(rel.clone()).or_insert_with(|| text.clone());
    }
    let test_only = reach.test_only();

    let mut out = reach.problems;
    let mut production: Vec<(String, String)> = Vec::new();
    for (rel, text) in &files {
        let is_test = test_only.contains(rel);
        out.extend(check_file(rel, text, is_test));
        if !is_test {
            production.push((rel.clone(), text.clone()));
        }
    }
    out.extend(super::ast::exec_violations(&production));
    out.extend(super::modules::module_edge_violations(&production));
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

/// The roots Cargo compiles, workspace-relative with `/` separators: `test` targets apart from
/// every other kind (see [`check_tree`]).
fn target_roots(metadata: &Metadata, root: &Path) -> Roots {
    let workspace = metadata.workspace_root.as_std_path();
    let mut roots = Roots::default();
    for package in &metadata.packages {
        for target in &package.targets {
            let src = target.src_path.as_std_path();
            let rel = src
                .strip_prefix(workspace)
                .or_else(|_| src.strip_prefix(root));
            let Ok(rel) = rel else {
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            // The tool itself (`xtask/`) is not product code and is not scanned.
            if !rel.starts_with("crates/") {
                continue;
            }
            if target.kind.contains(&TargetKind::Test) {
                roots.test.insert(rel);
            } else {
                roots.production.insert(rel);
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

    let lexed = lex(text);
    for (i, (code, literals)) in lexed.code.lines().zip(lexed.literals.lines()).enumerate() {
        let at = format!("{rel}:{}", i + 1);
        let spawns = code.contains("std::process::Command")
            || code.contains("tokio::process::Command")
            || code.contains("use std::process")
            || code.contains("use tokio::process");
        if spawns && !in_runner && !test_only {
            out.push(v(
                "R3",
                &at,
                "only domovoi-runner may spawn processes; use the ProcessExecutor port",
            ));
        }
        if code.contains("allow(clippy::disallowed_methods)") && !in_runner {
            out.push(v(
                "R3",
                &at,
                "`allow(clippy::disallowed_methods)` is only permitted in domovoi-runner",
            ));
        }
        if !test_only && is_shell_invocation(literals) {
            out.push(v(
                "R4",
                &at,
                "never invoke a shell; build an argv vector from validated parameters",
            ));
        }
        if code.contains("HumanApproval::restore(") && !restore_ok {
            out.push(v(
                "INV-1",
                &at,
                "HumanApproval::restore may only be called in crates/domovoi-store/src/",
            ));
        }
        if code.contains("HumanApproval::new(") && !approval_ok {
            out.push(v(
                "INV-1",
                &at,
                "HumanApproval may only be constructed in crates/domovoi-hub/src/auth/",
            ));
        }
        if code.contains("PolicyPermit::issue(") && !permit_ok {
            out.push(v(
                "INV-1",
                &at,
                "PolicyPermit::issue may only be called in crates/domovoi-hub/src/policy/",
            ));
        }
        if code.contains("Authorization::human(") && !authorization_human_ok {
            out.push(v(
                "INV-1",
                &at,
                "Authorization::human may only be called in crates/domovoi-hub/src/auth/",
            ));
        }
        if code.contains("Authorization::policy(") && !authorization_policy_ok {
            out.push(v(
                "INV-1",
                &at,
                "Authorization::policy may only be called in crates/domovoi-hub/src/policy/",
            ));
        }
        if literals.contains("plan:v2") && !plan_v2_ok {
            out.push(v(
                "INV-1",
                &at,
                "the plan:v2 domain line has one implementation, in crates/domovoi-core/src/action/",
            ));
        }
        if (code.contains("pub enum Authorization {") || code.contains("pub enum Authorization{"))
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
                if code.contains(forbidden) {
                    out.push(v(
                        "R2",
                        &at,
                        &format!("pure crates must not use `{forbidden}`"),
                    ));
                }
            }
        }
    }

    out.extend(super::identity_rules::check_file(rel, text, test_only));
    out.extend(super::runner_scan::check_file(rel, text, test_only));

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
