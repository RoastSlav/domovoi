//! Tests for the syntax-aware EXEC checks: formatting, aliases, macros, comments, derefs, the
//! link accessor in every spelling, and the dispatch construction and match cards. Which files
//! are production is tested in `reach_tests.rs`.

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
const EXECUTE: &str = "fn build() -> WriteCommand { WriteCommand::Dispatch { id: 1 } }\nfn send() { handoff.hand_off(a); handoff.await_handoff_result(b); }\n";
const OP_EXECUTE: &str = "fn build() -> OperationCommand { OperationCommand::Dispatch { id: 1 } }\nfn send() { handoff.hand_off(a); handoff.await_handoff_result(b); }\n";
const OP_DISPATCH: &str = "fn run(r: Row) -> R {\n    let authorized = AuthorizedStep::of(r)?;\n    execute::execute_step(authorized)\n}\n";
const OP_WRITER: &str =
    "fn write(c: OperationCommand) { match c { OperationCommand::Dispatch { .. } => {} } }\n";
const WRITER: &str =
    "fn write(command: WriteCommand) { match command { WriteCommand::Dispatch { .. } => {} } }\n";

/// The engine's card sites, each holding its exact count: three for the action service and
/// four for the operation service.
fn engine_sites() -> Vec<(String, String)> {
    files(&[
        ("crates/domovoi-engine/src/actions/executor.rs", EXECUTOR),
        ("crates/domovoi-engine/src/actions/execute.rs", EXECUTE),
        ("crates/domovoi-engine/src/actions/writer.rs", WRITER),
        (OP_EXECUTE_PATH, OP_EXECUTE),
        (OP_DISPATCH_PATH, OP_DISPATCH),
        (OP_WRITER_PATH, OP_WRITER),
    ])
}

const OP_EXECUTE_PATH: &str = "crates/domovoi-engine/src/operations/execute.rs";
const OP_DISPATCH_PATH: &str = "crates/domovoi-engine/src/operations/effects/dispatch.rs";
const OP_WRITER_PATH: &str = "crates/domovoi-engine/src/operations/writer/mod.rs";

/// The hub's one card site, with the calls split across lines as ordinary formatting allows.
const CAPS: &str = "fn f() {\n    self.link\n        .run_tool(a, b, c);\n    self.link\n        .start_skill(a, b, c, d, e);\n    self.link\n        .skill_result(a, b, c);\n    self.link\n        .run_check(a, b, c, d);\n}\n";

fn hub_sites() -> Vec<(String, String)> {
    files(&[
        ("crates/domovoi-hub/src/runtime/caps.rs", CAPS),
        (
            "crates/domovoi-hub/src/api/mod.rs",
            "fn f() { Caller::web(a, b); }
",
        ),
        (
            "crates/domovoi-hub/src/auth/interactions/mod.rs",
            "fn f() { Caller::chat(a, b, c); }
",
        ),
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
    hub[0].1 = format!("{CAPS}fn again() {{ self.link.run_tool(a, b, c); }}\n");
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

// ---- the link accessor as a path (EXEC-4) ----

const HOSTS: &str = "crates/domovoi-hub/src/api/hosts.rs";

fn accessor_violations(text: &str) -> Vec<Violation> {
    exec_violations(&files(&[(HOSTS, text)]))
}

#[test]
fn the_accessor_is_refused_in_every_path_spelling() {
    for text in [
        "fn f(server: &S) { let raw = AgentServer::link(&server); }\n",
        "fn f(server: &S) { let get_link = AgentServer::link; let raw = get_link(&server); }\n",
        "use crate::agent_server::AgentServer as Server;\nfn f(s: &S) { let raw = Server::link(&s); }\n",
        "use crate::agent_server::AgentServer::link as obtain;\nfn f(s: &S) { let raw = obtain(&s); }\n",
        "use crate::agent_server::AgentServer::link;\nfn f(s: &S) { let raw = link(&s); }\n",
        "use crate::agent_server::AgentServer;\ntype Server = AgentServer;\nfn f(s: &S) { let raw = Server::link(&s); }\n",
        "fn f(s: &S) { let raw = crate::agent_server::AgentServer::link(&s); }\n",
        "fn f(s: &S) { let raw = <AgentServer>::link(&s); }\n",
        "fn f(s: &S) { let raw = <crate::agent_server::AgentServer as Any>::link(&s); }\n",
        "impl AgentServer { fn g(&self) { let raw = Self::link(self); } }\n",
        "fn f(s: &S) { let raw = AgentServer::link_for_tests(&s); }\n",
        "fn f(s: &S) { let get = AgentServer::link_for_tests; let raw = get(&s); }\n",
        "use crate::agent_server::AgentServer as Server;\nfn f(s: &S) { let raw = Server::link_for_tests(&s); }\n",
        "use crate::agent_server::AgentServer::link_for_tests as test_link;\nfn f(s: &S) { let raw = test_link(&s); }\n",
        "fn f(s: &S) { wrap!(AgentServer::link(&s)); }\n",
    ] {
        let violations = accessor_violations(text);
        let found = violations
            .iter()
            .find(|violation| violation.rule == "EXEC-4");
        assert!(found.is_some(), "{text}\n{violations:#?}");
        assert!(
            found.is_some_and(|violation| violation.location.starts_with(HOSTS)),
            "{text}: {violations:#?}"
        );
    }
}

#[test]
fn the_accessor_check_names_the_line() {
    let text = "fn f() {}\n\nfn g(s: &S) {\n    let raw = AgentServer::link(&s);\n}\n";
    let violations = accessor_violations(text);
    let found = violations.iter().find(|v| v.rule == "EXEC-4").unwrap();
    assert_eq!(found.location, format!("{HOSTS}:4"));
}

#[test]
fn an_unrelated_link_is_not_an_accessor() {
    for text in [
        "fn f() { let a = Other::link(&x); }\n",
        "fn f() { let get = Other::link; get(&x); }\n",
        "use crate::elsewhere::Other as Server;\nfn f() { Server::link(&x); }\n",
        "use crate::elsewhere::Other::link as obtain;\nfn f() { obtain(&x); }\n",
        "type Server = Other;\nfn f() { Server::link(&x); }\n",
        "impl Other { fn g(&self) { Self::link(self); } }\n",
        "fn link() {}\nfn f() { link(); }\n",
        "// AgentServer::link(&server)\nfn f() {}\n",
        "fn f() { let m = \"AgentServer::link(&server)\"; }\n",
        "fn f() { let m = \"AgentServer::link_for_tests\"; /* AgentServer::link */ }\n",
    ] {
        let violations = accessor_violations(text);
        assert!(
            !violations
                .iter()
                .any(|violation| violation.rule == "EXEC-4"),
            "{text}\n{violations:#?}"
        );
    }
}

#[test]
fn the_accessor_paths_are_allowed_where_the_link_is_owned() {
    for rel in [
        "crates/domovoi-hub/src/agent_server/mod.rs",
        "crates/domovoi-hub/src/runtime/mod.rs",
    ] {
        let violations = exec_violations(&files(&[(
            rel,
            "fn f() { let raw = AgentServer::link(&s); }\n",
        )]));
        assert!(!has(&violations, "EXEC-4"), "{rel}: {violations:#?}");
    }
}

// ---- Dispatch: construction and match are separate cards (EXEC-2) ----

const EXECUTE_PATH: &str = "crates/domovoi-engine/src/actions/execute.rs";
const WRITER_PATH: &str = "crates/domovoi-engine/src/actions/writer.rs";
const OTHER_PATH: &str = "crates/domovoi-engine/src/actions/other.rs";

/// The engine's card sites with the file at `rel` replaced by `text`.
fn engine_with_site(rel: &str, text: &str) -> Vec<(String, String)> {
    let mut tree: Vec<(String, String)> = engine_sites()
        .into_iter()
        .filter(|(path, _)| path != rel)
        .collect();
    tree.push((rel.to_string(), text.to_string()));
    tree
}

fn messages(violations: &[Violation]) -> String {
    violations
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn one_construction_in_execute_and_one_pattern_in_writer_pass() {
    assert!(exec_violations(&engine_sites()).is_empty());
}

#[test]
fn a_second_construction_in_execute_is_refused() {
    let text =
        format!("{EXECUTE}fn again() -> WriteCommand {{ WriteCommand::Dispatch {{ id: 2 }} }}\n");
    let violations = exec_violations(&engine_with_site(EXECUTE_PATH, &text));
    assert!(
        messages(&violations).contains("struct expression"),
        "{violations:#?}"
    );
    assert!(
        violations
            .iter()
            .any(|v| v.location.starts_with(EXECUTE_PATH))
    );
}

#[test]
fn a_second_pattern_in_writer_is_refused() {
    let text = format!(
        "{WRITER}fn again(c: WriteCommand) {{ if let WriteCommand::Dispatch {{ .. }} = c {{}} }}\n"
    );
    let violations = exec_violations(&engine_with_site(WRITER_PATH, &text));
    assert!(
        messages(&violations).contains("struct pattern"),
        "{violations:#?}"
    );
}

#[test]
fn a_construction_in_writer_is_refused_even_beside_its_pattern() {
    let text =
        format!("{WRITER}fn build() -> WriteCommand {{ WriteCommand::Dispatch {{ id: 1 }} }}\n");
    let violations = exec_violations(&engine_with_site(WRITER_PATH, &text));
    assert!(
        violations
            .iter()
            .any(|v| v.location.starts_with(WRITER_PATH) && v.message.contains("struct expression")),
        "{violations:#?}"
    );
}

#[test]
fn a_pattern_in_execute_is_refused_even_beside_its_construction() {
    let text = format!(
        "{EXECUTE}fn m(c: WriteCommand) {{ match c {{ WriteCommand::Dispatch {{ .. }} => {{}} }} }}\n"
    );
    let violations = exec_violations(&engine_with_site(EXECUTE_PATH, &text));
    assert!(
        violations
            .iter()
            .any(|v| v.location.starts_with(EXECUTE_PATH) && v.message.contains("struct pattern")),
        "{violations:#?}"
    );
}

#[test]
fn the_wrong_node_kind_at_each_site_is_refused() {
    let swapped = vec![
        (
            EXECUTE_PATH.to_string(),
            "fn m(c: WriteCommand) { match c { WriteCommand::Dispatch { .. } => {} } }\n"
                .to_string(),
        ),
        (
            WRITER_PATH.to_string(),
            "fn build() -> WriteCommand { WriteCommand::Dispatch { id: 1 } }\n".to_string(),
        ),
        (
            "crates/domovoi-engine/src/actions/executor.rs".to_string(),
            EXECUTOR.to_string(),
        ),
    ];
    let violations = exec_violations(&swapped);
    for path in [EXECUTE_PATH, WRITER_PATH] {
        assert!(
            violations
                .iter()
                .filter(|v| v.location.starts_with(path))
                .count()
                >= 2,
            "{path}: {violations:#?}"
        );
    }
}

#[test]
fn a_dispatch_in_an_unrelated_production_module_is_refused() {
    for text in [
        "fn b() -> WriteCommand { WriteCommand::Dispatch { id: 1 } }\n",
        "fn m(c: WriteCommand) { match c { WriteCommand::Dispatch { .. } => {} } }\n",
    ] {
        let violations = exec_violations(&engine_with(OTHER_PATH, text));
        assert!(
            violations.iter().any(|v| v.location.contains("other.rs")),
            "{text}\n{violations:#?}"
        );
    }
}

#[test]
fn other_mentions_of_dispatch_are_refused() {
    for text in [
        "fn m(c: WriteCommand) { if matches!(c, WriteCommand::Dispatch { .. }) {} }\n",
        "fn m() { let f = WriteCommand::Dispatch; }\n",
        "fn m(c: WriteCommand) { match c { WriteCommand::Dispatch(..) => {} } }\n",
    ] {
        let violations = exec_violations(&engine_with(OTHER_PATH, text));
        assert!(
            violations.iter().any(|v| v.location.contains("other.rs")),
            "{text}\n{violations:#?}"
        );
    }
}

#[test]
fn comments_and_strings_naming_dispatch_do_not_count() {
    let text = "// WriteCommand::Dispatch { id }\nfn f() { let s = \"WriteCommand::Dispatch { id: 1 }\"; }\n/* WriteCommand::Dispatch { .. } */\n";
    let violations = exec_violations(&engine_with(OTHER_PATH, text));
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn renamed_dispatch_imports_are_resolved() {
    let text = "use crate::actions::WriteCommand as Cmd;\nfn b() -> Cmd { Cmd::Dispatch { id: 1 } }\nfn m(c: Cmd) { match c { Cmd::Dispatch { .. } => {} } }\n";
    let violations = exec_violations(&engine_with(OTHER_PATH, text));
    assert!(
        violations
            .iter()
            .filter(|v| v.location.contains("other.rs"))
            .count()
            >= 2,
        "{violations:#?}"
    );
}

#[test]
fn a_missing_construction_or_match_site_is_refused() {
    for (site, kind) in [
        (EXECUTE_PATH, "struct expression"),
        (WRITER_PATH, "struct pattern"),
    ] {
        let violations = exec_violations(&engine_with_site(site, "fn nothing() {}\n"));
        assert!(
            violations
                .iter()
                .any(|v| v.location.starts_with(site) && v.message.contains(kind)),
            "{site}: {violations:#?}"
        );
    }
}

// ---- The operation service's dispatch boundary (EXEC-2) ----

#[test]
fn the_operation_sites_pass_in_their_exact_counts() {
    assert!(exec_violations(&engine_sites()).is_empty());
}

#[test]
fn a_second_hand_off_in_a_card_site_is_refused() {
    for site in [OP_EXECUTE_PATH, EXECUTE_PATH] {
        let current = if site == OP_EXECUTE_PATH {
            OP_EXECUTE
        } else {
            EXECUTE
        };
        let text = format!(
            "{current}fn again() {{ handoff.hand_off(a); }}
"
        );
        let violations = exec_violations(&engine_with_site(site, &text));
        assert!(
            violations
                .iter()
                .any(|v| v.rule == "EXEC-2" && v.location.starts_with(site)),
            "{site}: {violations:#?}"
        );
    }
}

#[test]
fn a_second_await_handoff_result_in_a_card_site_is_refused() {
    let text = format!(
        "{OP_EXECUTE}fn again() {{ handoff.await_handoff_result(b); }}
"
    );
    let violations = exec_violations(&engine_with_site(OP_EXECUTE_PATH, &text));
    assert!(
        violations
            .iter()
            .any(|v| v.rule == "EXEC-2" && v.location.starts_with(OP_EXECUTE_PATH)),
        "{violations:#?}"
    );
}

#[test]
fn a_hand_off_or_await_outside_the_two_execute_files_is_refused() {
    for text in [
        "fn f() { handoff.hand_off(a, b); }
",
        "fn f() { handoff.await_handoff_result(a, b); }
",
    ] {
        let violations = exec_violations(&engine_with(OTHER_PATH, text));
        assert!(
            violations
                .iter()
                .any(|v| v.rule == "EXEC-2" && v.location.contains("other.rs")),
            "{text}
{violations:#?}"
        );
    }
}

#[test]
fn a_missing_hand_off_in_an_operation_site_is_refused() {
    let text = "fn build() -> OperationCommand { OperationCommand::Dispatch { id: 1 } }
";
    let violations = exec_violations(&engine_with_site(OP_EXECUTE_PATH, text));
    assert!(
        violations
            .iter()
            .any(|v| v.rule == "EXEC-2" && v.location.starts_with(OP_EXECUTE_PATH)),
        "{violations:#?}"
    );
}

#[test]
fn a_second_authorized_step_constructor_is_refused() {
    let again = "fn again(r: Row) { let _ = AuthorizedStep::of(r); }
";
    let in_site = format!("{OP_DISPATCH}{again}");
    for violations in [
        exec_violations(&engine_with_site(OP_DISPATCH_PATH, &in_site)),
        exec_violations(&engine_with(OTHER_PATH, again)),
    ] {
        assert!(
            violations
                .iter()
                .any(|v| v.rule == "EXEC-2" && v.message.contains("AuthorizedStep::of")),
            "{violations:#?}"
        );
    }
}

#[test]
fn a_second_execute_step_call_is_refused() {
    let again = "fn again(a: Step) { execute::execute_step(a); }
";
    let in_site = format!("{OP_DISPATCH}{again}");
    for violations in [
        exec_violations(&engine_with_site(OP_DISPATCH_PATH, &in_site)),
        exec_violations(&engine_with(OTHER_PATH, again)),
    ] {
        assert!(
            violations
                .iter()
                .any(|v| v.rule == "EXEC-2" && v.message.contains("execute::execute_step")),
            "{violations:#?}"
        );
    }
}

#[test]
fn a_second_operation_dispatch_construction_is_refused() {
    let again = "fn again() -> OperationCommand { OperationCommand::Dispatch { id: 2 } }
";
    let in_site = format!("{OP_EXECUTE}{again}");
    for violations in [
        exec_violations(&engine_with_site(OP_EXECUTE_PATH, &in_site)),
        exec_violations(&engine_with(OTHER_PATH, again)),
    ] {
        assert!(
            messages(&violations).contains("struct expression"),
            "{violations:#?}"
        );
    }
}

#[test]
fn a_second_operation_dispatch_pattern_is_refused() {
    let again = "fn m(c: OperationCommand) { match c { OperationCommand::Dispatch { .. } => {} } }
";
    let in_site = format!("{OP_WRITER}{again}");
    for violations in [
        exec_violations(&engine_with_site(OP_WRITER_PATH, &in_site)),
        exec_violations(&engine_with(OTHER_PATH, again)),
    ] {
        assert!(
            messages(&violations).contains("struct pattern"),
            "{violations:#?}"
        );
    }
}

#[test]
fn other_mentions_of_the_operation_dispatch_are_refused() {
    for text in [
        "fn m() { let f = OperationCommand::Dispatch; }
",
        "fn m(c: OperationCommand) { match c { OperationCommand::Dispatch(..) => {} } }
",
        "fn m(c: OperationCommand) { if matches!(c, OperationCommand::Dispatch { .. }) {} }
",
    ] {
        let violations = exec_violations(&engine_with(OTHER_PATH, text));
        assert!(
            violations.iter().any(|v| v.location.contains("other.rs")),
            "{text}
{violations:#?}"
        );
    }
}

#[test]
fn the_wrong_node_kind_at_an_operation_site_is_refused() {
    let swapped = engine_with_site(
        OP_EXECUTE_PATH,
        "fn m(c: OperationCommand) { match c { OperationCommand::Dispatch { .. } => {} } }
fn send() { handoff.hand_off(a); handoff.await_handoff_result(b); }
",
    );
    let violations = exec_violations(&swapped);
    assert!(
        violations
            .iter()
            .any(|v| v.location.starts_with(OP_EXECUTE_PATH)),
        "{violations:#?}"
    );
}

// ---- Who builds a Caller (CALLER-1) ----

const WEB_SITE: &str = "crates/domovoi-hub/src/api/mod.rs";
const CHAT_SITE: &str = "crates/domovoi-hub/src/auth/interactions/mod.rs";

#[test]
fn a_second_caller_web_call_is_refused() {
    let chat = (CHAT_SITE, "fn c() { Caller::chat(a, b, c); }\n");

    let twice = files(&[
        (
            WEB_SITE,
            "fn w() { Caller::web(a); }\nfn again() { Caller::web(b); }\n",
        ),
        chat,
    ]);
    let violations = exec_violations(&twice);
    assert!(has(&violations, "CALLER-1"), "{violations:#?}");

    let once = files(&[(WEB_SITE, "fn w() { Caller::web(a); }\n"), chat]);
    let violations = exec_violations(&once);
    assert!(!has(&violations, "CALLER-1"), "{violations:#?}");
}

#[test]
fn a_caller_chat_call_in_another_file_is_refused() {
    let web = (WEB_SITE, "fn w() { Caller::web(a); }\n");

    let elsewhere = files(&[
        (
            "crates/domovoi-hub/src/api/x.rs",
            "fn f() { Caller::chat(a, b, c); }\n",
        ),
        web,
        (CHAT_SITE, "fn c() { Caller::chat(a, b, c); }\n"),
    ]);
    let violations = exec_violations(&elsewhere);
    assert!(
        violations
            .iter()
            .any(|v| v.rule == "CALLER-1"
                && v.location.starts_with("crates/domovoi-hub/src/api/x.rs")),
        "{violations:#?}"
    );

    let owner = files(&[web, (CHAT_SITE, "fn c() { Caller::chat(a, b, c); }\n")]);
    let violations = exec_violations(&owner);
    assert!(!has(&violations, "CALLER-1"), "{violations:#?}");
}
