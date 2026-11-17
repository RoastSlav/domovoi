//! Tests for the syntax-aware EXEC checks: formatting, aliases, macros, comments, derefs and
//! the test-only wiring that decides which files are production.

use super::*;

fn files(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(rel, text)| ((*rel).to_string(), (*text).to_string()))
        .collect()
}

fn has(violations: &[Violation], rule: &str) -> bool {
    violations.iter().any(|violation| violation.rule == rule)
}

const EXECUTOR: &str = "fn run(a: Action) -> R {\n    let authorized = AuthorizedExecution::of(a)?;\n    execute::execute(authorized)\n}\n";
const EXECUTE: &str = "fn build() -> WriteCommand { WriteCommand::Dispatch { id: 1 } }\n";
const WRITER: &str =
    "fn write(command: WriteCommand) { match command { WriteCommand::Dispatch { .. } => {} } }\n";

/// The engine's three card sites, each holding its exact count.
fn engine_sites() -> Vec<(String, String)> {
    files(&[
        ("crates/domovoi-engine/src/actions/executor.rs", EXECUTOR),
        ("crates/domovoi-engine/src/actions/execute.rs", EXECUTE),
        ("crates/domovoi-engine/src/actions/writer.rs", WRITER),
    ])
}

const POLICY: &str = "fn f() {\n    AgentLink::run_tool(a, b, c);\n    AgentLink::start_skill(a, b, c, d, e);\n    AgentLink::skill_result(a, b, c);\n    AgentLink::run_check(a, b, c, d);\n}\n";

/// The hub's two card sites, with the calls split across lines as ordinary formatting allows.
const CAPS: &str = "fn f() {\n    self.link\n        .run_tool(a, b, c);\n    self.link\n        .start_skill(a, b, c, d, e);\n    self.link\n        .skill_result(a, b, c);\n    self.link\n        .run_check(a, b, c, d);\n}\n";

fn hub_sites() -> Vec<(String, String)> {
    files(&[
        (
            "crates/domovoi-hub/src/agent_server/skill_policy.rs",
            POLICY,
        ),
        ("crates/domovoi-hub/src/runtime/caps.rs", CAPS),
    ])
}

/// The complete engine card set plus one extra production file.
fn engine_with(rel: &str, text: &str) -> Vec<(String, String)> {
    let mut tree = engine_sites();
    tree.push((rel.to_string(), text.to_string()));
    tree
}

#[test]
fn the_card_sites_pass_in_their_exact_counts() {
    assert!(exec_violations(&engine_sites()).is_empty());
    assert!(exec_violations(&hub_sites()).is_empty());
}

#[test]
fn a_second_call_in_a_card_site_is_refused() {
    let mut hub = hub_sites();
    hub[1].1 = format!("{CAPS}fn again() {{ self.link.run_tool(a, b, c); }}\n");
    let violations = exec_violations(&hub);
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn a_call_split_across_lines_is_counted() {
    let text = "deps.link.run_tool\n    (host, call, deadline)\n    .await?;\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn an_accessor_split_from_its_parentheses_is_counted() {
    let text = "let link = server.link\n    ();\n";
    let violations = exec_violations(&files(&[("crates/domovoi-hub/src/api/hosts.rs", text)]));
    assert!(has(&violations, "EXEC-4"), "{violations:#?}");
}

#[test]
fn a_renamed_import_is_resolved() {
    let text = "use domovoi_core::ports::AgentLink as Proxy;\nfn proxy(link: std::sync::Arc<dyn Proxy>) {}\n";
    let violations = exec_violations(&files(&[("crates/domovoi-hub/src/api/hosts.rs", text)]));
    assert!(has(&violations, "EXEC-4"), "{violations:#?}");
}

#[test]
fn renamed_boundary_constructors_are_resolved() {
    let text = "use super::execute::{execute as dispatch, AuthorizedExecution as Authorized};\n\
                fn run(a: Action) {\n    let authorized = Authorized::of(a);\n    dispatch(authorized);\n}\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-2"), "{violations:#?}");
}

#[test]
fn a_macro_body_cannot_hide_an_operation() {
    let text = "macro_rules! sneak { () => { deps.link.run_tool(a, b, c) } }\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn an_operation_inside_a_macro_invocation_is_counted() {
    let text = "fn f() {\n    assert!(check { run_tool(a, b, c) });\n}\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn code_after_a_block_comment_on_the_same_line_is_counted() {
    let text = "fn f() {\n    /* checked by hand */ let out = link.run_tool(a, b, c);\n}\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-hub/src/profile/collect.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn a_line_starting_with_a_deref_is_counted() {
    let text = "fn f() {\n    *slot.borrow_mut() = run_tool(a, b, c);\n}\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn comments_and_strings_are_ignored() {
    let text = "// link.run_tool(a, b, c) in prose\nlet message = \"link.run_tool(a, b, c)\";\n";
    let violations = exec_violations(&engine_with(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    ));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn inline_test_modules_are_pruned() {
    let text = "#[cfg(test)]\nmod tests { fn f() { run_tool(a, b, c); } }\n";
    let violations = exec_violations(&engine_with(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    ));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn a_negated_cfg_test_module_is_production() {
    let text = "#[cfg(not(test))]\nmod tests { fn f() { run_tool(a, b, c); } }\n";
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    )]));
    assert!(has(&violations, "EXEC-1"), "{violations:#?}");
}

#[test]
fn a_conjunction_with_cfg_test_is_pruned() {
    let text = "#[cfg(all(test, unix))]\nmod tests { fn f() { run_tool(a, b, c); } }\n";
    let violations = exec_violations(&engine_with(
        "crates/domovoi-engine/src/actions/other.rs",
        text,
    ));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn a_missing_card_site_is_refused() {
    let mut engine = engine_sites();
    engine.retain(|(rel, _)| !rel.ends_with("writer.rs"));
    let violations = exec_violations(&engine);
    assert!(
        violations
            .iter()
            .any(|violation| violation.location.ends_with("writer.rs")),
        "{violations:#?}"
    );
}

#[test]
fn filename_test_modules_are_production() {
    for (rel, text) in [
        (
            "crates/domovoi-engine/src/tests/probe.rs",
            "fn f() { run_tool(a, b, c); }\n",
        ),
        (
            "crates/domovoi-engine/src/probe_tests.rs",
            "fn f() { run_tool(a, b, c); }\n",
        ),
        (
            "crates/domovoi-hub/src/tests/probe.rs",
            "fn f(link: std::sync::Arc<dyn AgentLink>) {}\n",
        ),
    ] {
        let violations = exec_violations(&files(&[(rel, text)]));
        assert!(
            has(&violations, "EXEC-1") || has(&violations, "EXEC-4"),
            "{rel} was treated as test-only: {violations:#?}"
        );
    }
}

#[test]
fn an_unparseable_file_is_refused() {
    let violations = exec_violations(&files(&[(
        "crates/domovoi-engine/src/actions/other.rs",
        "fn broken( {\n",
    )]));
    assert!(has(&violations, "EXEC"), "{violations:#?}");
}

#[test]
fn test_only_files_follow_wiring_not_names() {
    let tree = files(&[
        (
            "crates/domovoi-engine/src/actions/execute.rs",
            "#[cfg(test)]\n#[path = \"execute_tests.rs\"]\nmod tests;\n",
        ),
        (
            "crates/domovoi-engine/src/actions/execute_tests.rs",
            "fn f() { run_tool(a, b, c); }\n",
        ),
        ("crates/domovoi-engine/src/actions/mod.rs", "mod tests;\n"),
        (
            "crates/domovoi-engine/src/actions/tests.rs",
            "fn f() { run_tool(a, b, c); }\n",
        ),
    ]);
    let test_only = cfg_test_closure(&tree);
    assert!(test_only.contains("crates/domovoi-engine/src/actions/execute_tests.rs"));
    assert!(
        !test_only.contains("crates/domovoi-engine/src/actions/tests.rs"),
        "a tests.rs module wired without cfg(test) is production"
    );
    let production: Vec<(String, String)> = tree
        .iter()
        .filter(|(rel, _)| !test_only.contains(rel))
        .cloned()
        .collect();
    let violations = exec_violations(&production);
    assert!(
        violations
            .iter()
            .any(|violation| violation.location.contains("actions/tests.rs")),
        "{violations:#?}"
    );
}

#[test]
fn test_target_roots_reach_their_modules() {
    let root = "crates/domovoi-engine/tests/services/main.rs";
    let tree = files(&[
        (root, "mod gates;\nmod harness;\n"),
        (
            "crates/domovoi-engine/tests/services/gates.rs",
            "#[path = \"gates_inner.rs\"]\nmod gates_inner;\n",
        ),
        (
            "crates/domovoi-engine/tests/services/harness.rs",
            "fn h() {}\n",
        ),
        (
            "crates/domovoi-engine/tests/services/gates_inner.rs",
            "fn g() {}\n",
        ),
    ]);
    let roots: BTreeSet<String> = [root.to_string()].into_iter().collect();
    let test_only = test_only_files(&tree, &roots);
    for expected in [
        root,
        "crates/domovoi-engine/tests/services/gates.rs",
        "crates/domovoi-engine/tests/services/harness.rs",
        "crates/domovoi-engine/tests/services/gates_inner.rs",
    ] {
        assert!(
            test_only.contains(expected),
            "{expected} was left production"
        );
    }
}
