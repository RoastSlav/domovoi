//! Tests for the line-based architecture rules: every rule, its allowed areas and the
//! ordinary second path a rule exists to refuse. The EXEC boundary rules are syntax-aware and
//! are tested in `ast_tests.rs`.

use super::*;

const DOC: &str = "//! Crate doc.\n";

#[test]
fn process_spawn_outside_runner_is_rejected() {
    let text = format!("{DOC}use std::process::Command;\n");
    let v = check_file("crates/domovoi-hub/src/lib.rs", &text, false);
    assert!(v.iter().any(|x| x.rule == "R3"));
}

#[test]
fn process_spawn_in_runner_is_allowed() {
    let text = format!("{DOC}use std::process::Command;\n");
    assert!(check_file("crates/domovoi-runner/src/lib.rs", &text, false).is_empty());
}

#[test]
fn shell_invocation_is_rejected() {
    let text = format!("{DOC}let argv = vec![\"sh\", \"-c\", cmd];\n");
    let v = check_file("crates/domovoi-runner/src/exec.rs", &text, false);
    assert!(v.iter().any(|x| x.rule == "R4"));
}

#[test]
fn human_approval_outside_auth_is_rejected() {
    let text = "let a = HumanApproval::new(user);\n";
    let v = check_file("crates/domovoi-engine/src/approvals.rs", text, false);
    assert_eq!(v[0].rule, "INV-1");
    assert!(check_file("crates/domovoi-hub/src/auth/session.rs", text, false).is_empty());
    assert!(check_file("crates/domovoi-engine/tests/gates.rs", text, true).is_empty());
}

#[test]
fn human_approval_restore_outside_store_is_rejected() {
    let text = "let a = HumanApproval::restore(user, channel, at);\n";
    let v = check_file("crates/domovoi-hub/src/auth/session.rs", text, false);
    assert_eq!(v[0].rule, "INV-1");
    assert_eq!(
        check_file("crates/domovoi-engine/src/actions.rs", text, false)[0].rule,
        "INV-1"
    );
    assert!(check_file("crates/domovoi-store/src/rows/action.rs", text, false).is_empty());
    assert!(check_file("crates/domovoi-store/tests/actions.rs", text, true).is_empty());
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
            check_file("crates/domovoi-engine/src/actions.rs", &text, false)
                .iter()
                .any(|x| x.rule == "INV-1"),
            "{call} was accepted in the engine"
        );
        if allowed != "crates/domovoi-store/src/" {
            assert!(
                check_file("crates/domovoi-store/src/repos/actions.rs", &text, false)
                    .iter()
                    .any(|x| x.rule == "INV-1"),
                "{call} was accepted in the store"
            );
        }
        assert!(
            check_file(&format!("{allowed}gate.rs"), &text, false).is_empty(),
            "{call} was rejected in its allowed prefix"
        );
        assert!(
            check_file("crates/domovoi-engine/tests/gates.rs", &text, true).is_empty(),
            "{call} was rejected in a test target"
        );
    }
}

#[test]
fn plan_hash_v2_has_one_implementation() {
    let text = "let text = \"plan:v2\\n\";\n";
    assert_eq!(
        check_file("crates/domovoi-engine/src/actions/executor.rs", text, false)[0].rule,
        "INV-1"
    );
    assert!(check_file("crates/domovoi-core/src/action/plan.rs", text, false).is_empty());
    assert!(check_file("crates/domovoi-core/tests/plan_v2.rs", text, true).is_empty());
}

#[test]
fn authorization_is_not_a_public_enum() {
    let text = "pub enum Authorization { Human, Policy }\n";
    assert_eq!(
        check_file("crates/domovoi-core/src/action/mod.rs", text, false)[0].rule,
        "INV-1"
    );
    assert!(check_file(AUTHORIZATION_MODULE, text, false).is_empty());
}

#[test]
fn pure_crate_io_is_rejected() {
    let text = "let f = std::fs::read(\"x\");\n";
    assert_eq!(
        check_file("crates/domovoi-core/src/incident.rs", text, false)[0].rule,
        "R2"
    );
    assert!(check_file("crates/domovoi-store/src/db.rs", text, false).is_empty());
}

#[test]
fn long_file_is_rejected_but_test_only_files_are_exempt() {
    let text = "fn a() {}\n".repeat(MAX_FILE_LINES + 1);
    assert_eq!(
        check_file("crates/domovoi-engine/src/big.rs", &text, false)[0].rule,
        "R8"
    );
    assert!(check_file("crates/domovoi-engine/tests/big.rs", &text, true).is_empty());
}

#[test]
fn a_test_like_name_does_not_exempt_a_module() {
    let text = "fn a() {}\n".repeat(MAX_FILE_LINES + 1);
    for rel in [
        "crates/domovoi-engine/src/probe_tests.rs",
        "crates/domovoi-engine/src/tests/probe.rs",
        "crates/domovoi-engine/src/tests.rs",
    ] {
        assert!(
            check_file(rel, &text, false).iter().any(|x| x.rule == "R8"),
            "{rel} was exempted by its name"
        );
        assert!(check_file(rel, &text, true).is_empty());
    }
}

#[test]
fn crate_root_without_doc_is_rejected() {
    assert_eq!(
        check_file("crates/domovoi-core/src/lib.rs", "pub mod a;\n", false)[0].rule,
        "R10"
    );
}

#[test]
fn comments_are_ignored() {
    let text = "// Never call std::process::Command here.\n";
    assert!(check_file("crates/domovoi-engine/src/a.rs", text, false).is_empty());
}
