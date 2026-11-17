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

// ---- the lexer behind the line rules ----

fn rules(violations: &[Violation]) -> Vec<&'static str> {
    violations.iter().map(|violation| violation.rule).collect()
}

const ENGINE: &str = "crates/domovoi-engine/src/approvals.rs";

#[test]
fn a_unary_deref_line_is_code() {
    let text = "fn f() {\n    *slot = HumanApproval::new(user);\n}\n";
    let v = check_file(ENGINE, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    assert_eq!(v[0].location, format!("{ENGINE}:2"));
}

#[test]
fn code_after_a_block_comment_on_the_same_line_is_checked() {
    let text = "fn f() {\n    /* explanation */ let approval = HumanApproval::new(user);\n}\n";
    let v = check_file(ENGINE, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    assert_eq!(v[0].location, format!("{ENGINE}:2"));
}

#[test]
fn a_multi_line_block_comment_before_a_call_hides_nothing_after_it() {
    let text = "fn f() {\n    /* first line\n       second line\n    */ let a = HumanApproval::new(user);\n}\n";
    let v = check_file(ENGINE, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    assert_eq!(v[0].location, format!("{ENGINE}:4"));
}

#[test]
fn a_nested_block_comment_ends_at_its_own_close() {
    let text = "fn f() {\n    /* outer /* inner */ HumanApproval::new(user); */\n    let a = HumanApproval::new(user);\n}\n";
    let v = check_file(ENGINE, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    assert_eq!(v[0].location, format!("{ENGINE}:3"));
}

#[test]
fn a_comment_that_names_a_forbidden_call_is_not_flagged() {
    let text = "// HumanApproval::new(user) is called in auth\n/// std::process::Command\n/* use std::fs; \"sh\" \"-c\" plan:v2 */\nfn f() {}\n";
    assert!(check_file("crates/domovoi-engine/src/a.rs", text, false).is_empty());
}

#[test]
fn a_string_naming_a_constructor_is_not_code() {
    let text = "fn f() { let m = \"call HumanApproval::new( here\"; let n = \"use std::fs\"; }\n";
    assert!(check_file("crates/domovoi-engine/src/a.rs", text, false).is_empty());
}

#[test]
fn a_raw_string_with_comment_markers_does_not_hide_the_next_lines() {
    let text = "fn f() {\n    let a = r#\"// not a comment /* nor this\"#;\n    let b = HumanApproval::new(user);\n}\n";
    let v = check_file(ENGINE, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    assert_eq!(v[0].location, format!("{ENGINE}:3"));
}

#[test]
fn a_shell_literal_is_still_recognised() {
    let text =
        "fn f() {\n    let argv = vec![\"sh\", \"-c\", cmd];\n    let path = \"/bin/bash\";\n}\n";
    let v = check_file("crates/domovoi-runner/src/exec.rs", text, false);
    assert_eq!(rules(&v), ["R4", "R4"], "{v:#?}");
    let raw = "fn f() { let s = r#\"/bin/sh\"#; }\n";
    assert_eq!(
        rules(&check_file("crates/domovoi-runner/src/exec.rs", raw, false)),
        ["R4"]
    );
    let in_comment = "// vec![\"sh\", \"-c\", cmd]\nfn f() {}\n";
    assert!(check_file("crates/domovoi-runner/src/exec.rs", in_comment, false).is_empty());
}

#[test]
fn a_real_plan_v2_literal_is_still_recognised() {
    let text = "fn f() {\n    /* c */ let line = \"plan:v2\\n\";\n}\n";
    let rel = "crates/domovoi-engine/src/actions/executor.rs";
    let v = check_file(rel, text, false);
    assert_eq!(rules(&v), ["INV-1"], "{v:#?}");
    let raw = "fn f() { let line = r\"plan:v2\"; }\n";
    assert_eq!(rules(&check_file(rel, raw, false)), ["INV-1"]);
}

#[test]
fn a_process_spawn_after_a_comment_or_deref_is_still_refused() {
    let text = "//! Doc.\nfn f() {\n    /* spawn */ let c = std::process::Command::new(\"x\");\n    *slot = tokio::process::Command::new(\"y\");\n}\n";
    let v = check_file("crates/domovoi-hub/src/lib.rs", text, false);
    assert_eq!(rules(&v), ["R3", "R3"], "{v:#?}");
}

// ---- the whole tree, over a real Cargo workspace ----

/// A one-crate workspace whose library is `lib`, plus extra files, checked by `check_tree`.
fn check_workspace(lib: &str, extra: &[(&str, &str)]) -> Vec<Violation> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let put = |rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    put(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"3\"\n",
    );
    put(
        "crates/demo/Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    );
    put("crates/demo/src/lib.rs", &format!("//! Demo.\n{lib}"));
    for (rel, text) in extra {
        put(rel, text);
    }
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(root.join("Cargo.toml"))
        .no_deps()
        .exec()
        .unwrap();
    check_tree(root, &metadata).unwrap()
}

const RAW: &str = "fn raw(link: &L) {\n    link.run_tool(host, call, deadline);\n}\n";

#[test]
fn a_production_path_module_outside_crates_is_scanned() {
    let v = check_workspace(
        "#[path = \"../../../shared/raw_link.rs\"]\nmod raw_link;\n",
        &[("shared/raw_link.rs", RAW)],
    );
    let found = v.iter().find(|x| x.rule == "EXEC-1").expect("detected");
    assert_eq!(found.location, "shared/raw_link.rs:2");
}

#[test]
fn a_production_include_outside_crates_is_scanned() {
    let v = check_workspace(
        "include!(\"../../../shared/raw_link.rs\");\n",
        &[("shared/raw_link.rs", RAW)],
    );
    let found = v.iter().find(|x| x.rule == "EXEC-1").expect("detected");
    assert_eq!(found.location, "shared/raw_link.rs:2");
}

#[test]
fn a_missing_external_source_names_the_includer() {
    let v = check_workspace("include!(\"../../../shared/gone.rs\");\n", &[]);
    let found = v.iter().find(|x| x.rule == "SRC-1").expect("refused");
    assert_eq!(found.location, "crates/demo/src/lib.rs:2");
    assert!(found.message.contains("shared/gone.rs"), "{found}");
}

#[test]
fn a_path_leaving_the_workspace_is_refused() {
    let v = check_workspace(
        "#[path = \"../../../../outside.rs\"]\nmod outside;\ninclude!(\"../../../../../far.rs\");\n",
        &[],
    );
    let locations: Vec<&str> = v
        .iter()
        .filter(|x| x.rule == "SRC-1")
        .map(|x| x.location.as_str())
        .collect();
    assert_eq!(
        locations,
        ["crates/demo/src/lib.rs:3", "crates/demo/src/lib.rs:4"]
    );
}

#[test]
fn include_str_of_a_data_file_is_not_rust() {
    let v = check_workspace(
        "const DATA: &str = include_str!(\"../../../shared/data.txt\");\n",
        &[("shared/data.txt", "link.run_tool( not rust at all {{{\n")],
    );
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn the_same_inclusion_from_test_only_source_is_test_only() {
    let v = check_workspace(
        "#[cfg(test)]\nmod t {\n    include!(\"../../../../shared/raw_link.rs\");\n}\n",
        &[("shared/raw_link.rs", RAW)],
    );
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn a_dynamic_include_fails_closed() {
    let v = check_workspace(
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
        &[],
    );
    assert!(
        v.iter()
            .any(|x| x.rule == "SRC-1" && x.location == "crates/demo/src/lib.rs:2"),
        "{v:#?}"
    );
}

#[test]
fn a_production_file_a_test_module_also_loads_is_still_checked() {
    let v = check_workspace(
        "#[cfg(test)]\n#[path = \"../../../shared/raw_link.rs\"]\nmod test_shared;\n#[path = \"../../../shared/raw_link.rs\"]\nmod production_shared;\n",
        &[("shared/raw_link.rs", RAW)],
    );
    assert!(v.iter().any(|x| x.rule == "EXEC-1"), "{v:#?}");
}
