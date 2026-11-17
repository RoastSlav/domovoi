//! Source scans: R3 (process spawning), R4 (no shell), R2 (pure crates),
//! R8 (file size), R10 (crate root docs), INV-1 (approval call sites).

use std::path::Path;

use super::Violation;
use super::rules::{
    AGENT_LINK_ALLOWED_PREFIXES, AUTHORIZATION_HUMAN_ALLOWED_PREFIXES, AUTHORIZATION_MODULE,
    AUTHORIZATION_POLICY_ALLOWED_PREFIXES, HUMAN_APPROVAL_ALLOWED_PREFIXES,
    HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES, LINK_DEFINITION_PREFIXES, MAX_FILE_LINES,
    PLAN_HASH_V2_ALLOWED_PREFIXES, POLICY_PERMIT_ALLOWED_PREFIXES, PROCESS_ALLOWED_PREFIXES,
    PURE_CRATE_DIRS, PURE_FORBIDDEN_PATHS, RUN_CHECK_ALLOWED_PREFIXES, RUN_SKILL_ALLOWED_PREFIXES,
    RUN_TOOL_ALLOWED_PREFIXES,
};

/// Crates whose source may not start a skill or read a host outside a boundary (EXEC-1..3).
const EXECUTION_SCANNED_CRATES: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-engine/src/",
    "crates/domovoi-hub/src/",
    "crates/domovoi-store/src/",
];

/// Crates whose sources may not name the link types or take the raw link (EXEC-4). The
/// binary is included because it composes the hub: a first-party source there that reached
/// for the link would bypass every boundary, and only `tests/` is exempt.
const LINK_SCANNED_CRATES: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-engine/src/",
    "crates/domovoi-hub/src/",
    "crates/domovoi/src/",
];

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
fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// Check one file. `rel` is the path relative to the workspace root, with `/` separators.
pub fn check_file(rel: &str, text: &str) -> Vec<Violation> {
    let mut out = Vec::new();
    let test = is_test_path(rel);
    let in_runner = PROCESS_ALLOWED_PREFIXES
        .iter()
        .any(|p| path_matches(rel, p));
    let approval_ok = test
        || HUMAN_APPROVAL_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let restore_ok = test
        || HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let permit_ok = test
        || POLICY_PERMIT_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let authorization_human_ok = test
        || AUTHORIZATION_HUMAN_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let authorization_policy_ok = test
        || AUTHORIZATION_POLICY_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let plan_v2_ok = test
        || PLAN_HASH_V2_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let run_tool_ok = test
        || RUN_TOOL_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p))
        || LINK_DEFINITION_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let run_skill_ok = test
        || RUN_SKILL_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p))
        || LINK_DEFINITION_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let run_check_ok = test
        || RUN_CHECK_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p))
        || LINK_DEFINITION_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let link_name_ok = test
        || AGENT_LINK_ALLOWED_PREFIXES
            .iter()
            .any(|p| path_matches(rel, p));
    let execution_scope = EXECUTION_SCANNED_CRATES.iter().any(|p| rel.starts_with(p));
    let link_scope = LINK_SCANNED_CRATES.iter().any(|p| rel.starts_with(p));
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
        if execution_scope && !run_tool_ok && line.contains("run_tool(") {
            out.push(v(
                "EXEC-1",
                &at,
                "a host is read only through Evidence::collect_evidence, in crates/domovoi-engine/src/evidence/",
            ));
        }
        if execution_scope && !run_skill_ok {
            for call in ["run_skill(", "start_skill(", "run_capability("] {
                if line.contains(call) {
                    out.push(v(
                        "EXEC-2",
                        &at,
                        "a skill runs only through execute(AuthorizedExecution), in crates/domovoi-engine/src/actions/execute.rs",
                    ));
                }
            }
        }
        if execution_scope && !run_check_ok && line.contains("run_check(") {
            out.push(v(
                "EXEC-3",
                &at,
                "a check is re-run only by verification, in crates/domovoi-engine/src/actions/verify_wake.rs",
            ));
        }
        if link_scope && !link_name_ok {
            for named in ["AgentLink", "WsAgentLink", "PolicyLink"] {
                if line.contains(named) {
                    out.push(v(
                        "EXEC-4",
                        &at,
                        "a transport reads host state through AgentAdmin, never by naming or holding the link",
                    ));
                }
            }
            if names_the_raw_link_accessor(line) {
                out.push(v(
                    "EXEC-4",
                    &at,
                    "a transport may not take the agent link through the server's accessor, whatever its inferred type; read host state through AgentAdmin",
                ));
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

/// Whether `line` takes the raw link from the server: `server.link()`, `self.link()` or
/// `server.link_for_tests()`. The returned type need not appear, which is why the type-name
/// scan cannot see this on its own, and a transport is never allowed one.
fn names_the_raw_link_accessor(line: &str) -> bool {
    line.contains(".link()") || line.contains(".link_for_tests()")
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
