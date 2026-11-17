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

/// Crates in which a transport must not even name the link types (EXEC-4).
const LINK_SCANNED_CRATES: &[&str] = &[
    "crates/domovoi-core/src/",
    "crates/domovoi-engine/src/",
    "crates/domovoi-hub/src/",
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
    let restore_ok = test
        || HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let permit_ok = test
        || POLICY_PERMIT_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let authorization_human_ok = test
        || AUTHORIZATION_HUMAN_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let authorization_policy_ok = test
        || AUTHORIZATION_POLICY_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let plan_v2_ok = test
        || PLAN_HASH_V2_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
    let run_tool_ok = test
        || RUN_TOOL_ALLOWED_PREFIXES.iter().any(|p| rel.starts_with(p))
        || LINK_DEFINITION_PREFIXES.iter().any(|p| rel.starts_with(p));
    let run_skill_ok = test
        || RUN_SKILL_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p))
        || LINK_DEFINITION_PREFIXES.iter().any(|p| rel.starts_with(p));
    let run_check_ok = test
        || RUN_CHECK_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p))
        || LINK_DEFINITION_PREFIXES.iter().any(|p| rel.starts_with(p));
    let link_name_ok = test
        || AGENT_LINK_ALLOWED_PREFIXES
            .iter()
            .any(|p| rel.starts_with(p));
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
            if line.contains("link_for_tests(") {
                out.push(v(
                    "EXEC-4",
                    &at,
                    "link_for_tests is for test suites; a transport reaches agents through AgentAdmin",
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
    fn human_approval_restore_outside_store_is_rejected() {
        let text = "let a = HumanApproval::restore(user, channel, at);
";
        let v = check_file("crates/domovoi-hub/src/auth/session.rs", text);
        assert_eq!(v[0].rule, "INV-1");
        assert_eq!(
            check_file("crates/domovoi-engine/src/actions.rs", text)[0].rule,
            "INV-1"
        );
        assert!(check_file("crates/domovoi-store/src/rows/action.rs", text).is_empty());
        assert!(check_file("crates/domovoi-store/tests/actions.rs", text).is_empty());
    }

    #[test]
    fn authorization_constructors_are_confined() {
        let cases = [
            ("HumanApproval::new(", "crates/domovoi-hub/src/auth/"),
            ("HumanApproval::restore(", "crates/domovoi-store/src/"),
            ("PolicyPermit::issue(", "crates/domovoi-hub/src/policy/"),
            ("Authorization::human(", "crates/domovoi-hub/src/auth/"),
            ("Authorization::policy(", "crates/domovoi-hub/src/policy/"),
        ];
        for (call, allowed) in cases {
            let text = format!("let a = {call}value);\n");
            assert!(
                check_file("crates/domovoi-engine/src/actions.rs", &text)
                    .iter()
                    .any(|x| x.rule == "INV-1"),
                "{call} was accepted in the engine"
            );
            if allowed != "crates/domovoi-store/src/" {
                assert!(
                    check_file("crates/domovoi-store/src/repos/actions.rs", &text)
                        .iter()
                        .any(|x| x.rule == "INV-1"),
                    "{call} was accepted in the store"
                );
            }
            assert!(
                check_file(&format!("{allowed}gate.rs"), &text).is_empty(),
                "{call} was rejected in its allowed prefix"
            );
            assert!(
                check_file("crates/domovoi-engine/tests/gates.rs", &text).is_empty(),
                "{call} was rejected in a test file"
            );
        }
    }

    #[test]
    fn plan_hash_v2_has_one_implementation() {
        let text = "let text = \"plan:v2\\n\";\n";
        assert_eq!(
            check_file("crates/domovoi-engine/src/actions/executor.rs", text)[0].rule,
            "INV-1"
        );
        assert!(check_file("crates/domovoi-core/src/action/plan.rs", text).is_empty());
        assert!(check_file("crates/domovoi-core/tests/plan_v2.rs", text).is_empty());
    }

    #[test]
    fn authorization_is_not_a_public_enum() {
        let text = "pub enum Authorization { Human, Policy }\n";
        assert_eq!(
            check_file("crates/domovoi-core/src/action/mod.rs", text)[0].rule,
            "INV-1"
        );
        assert!(check_file(AUTHORIZATION_MODULE, text).is_empty());
    }

    #[test]
    fn a_host_read_outside_evidence_is_rejected() {
        let text = "let out = link.run_tool(host, call, deadline).await;\n";
        assert_eq!(
            check_file("crates/domovoi-hub/src/profile/collect.rs", text)[0].rule,
            "EXEC-1"
        );
        assert!(check_file("crates/domovoi-engine/src/evidence/mod.rs", text).is_empty());
        assert!(
            check_file("crates/domovoi-engine/src/evidence/evidence_tests.rs", text).is_empty()
        );
        assert!(
            check_file("crates/domovoi-agent/src/handler.rs", text).is_empty(),
            "the agent's own request handler is not a hub read"
        );
    }

    #[test]
    fn a_skill_start_outside_execute_is_rejected() {
        for text in [
            "link.run_skill(host, req).await;\n",
            "link.start_skill(host, req, session, epoch, deadline).await;\n",
            "link.run_capability(host, req).await;\n",
        ] {
            assert_eq!(
                check_file("crates/domovoi-engine/src/triage/tools.rs", text)[0].rule,
                "EXEC-2",
                "{text} was accepted outside the execution boundary"
            );
            assert!(check_file("crates/domovoi-engine/src/actions/execute.rs", text).is_empty());
            assert!(check_file("crates/domovoi-engine/src/triage/tools_tests.rs", text).is_empty());
        }
    }

    #[test]
    fn a_check_rerun_outside_verification_is_rejected() {
        let text = "link.run_check(host, check, None, deadline).await;\n";
        assert_eq!(
            check_file("crates/domovoi-engine/src/triage/tools.rs", text)[0].rule,
            "EXEC-3"
        );
        assert!(check_file("crates/domovoi-engine/src/actions/verify_wake.rs", text).is_empty());
    }

    #[test]
    fn a_transport_naming_the_link_is_rejected() {
        for text in [
            "let link: Arc<dyn AgentLink> = server.link();\n",
            "let link = WsAgentLink::new(registry, clock, hosts);\n",
            "let link = PolicyLink::wrap(inner, policy);\n",
            "let link = server.link_for_tests();\n",
        ] {
            assert_eq!(
                check_file("crates/domovoi-hub/src/api/hosts.rs", text)[0].rule,
                "EXEC-4",
                "{text} was accepted in a transport"
            );
            assert!(check_file("crates/domovoi-hub/src/agent_server/mod.rs", text).is_empty());
            assert!(check_file("crates/domovoi-hub/tests/hub/sessions.rs", text).is_empty());
            assert!(check_file("crates/domovoi-engine/src/actions/mod.rs", text).is_empty());
        }
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
