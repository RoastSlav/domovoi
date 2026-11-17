//! Unit tests for the line-based architecture scans: every rule, its allowed areas and the
//! ordinary second path a rule exists to refuse.

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
    let text = "let a = HumanApproval::restore(user, channel, at);\n";
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
    assert!(check_file("crates/domovoi-engine/src/evidence/evidence_tests.rs", text).is_empty());
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

/// The trait file is the one `ports/` exception; another port cannot read a host or start a
/// skill just because it lives next to it.
#[test]
fn the_link_definition_exception_is_the_trait_file_alone() {
    let cases = [
        (
            "let out = link.run_tool(host, call, deadline).await;\n",
            "EXEC-1",
        ),
        (
            "link.start_skill(host, req, session, epoch, deadline).await;\n",
            "EXEC-2",
        ),
        (
            "link.run_check(host, check, None, deadline).await;\n",
            "EXEC-3",
        ),
    ];
    for (text, rule) in cases {
        assert!(
            check_file("crates/domovoi-core/src/ports/agent_link.rs", text).is_empty(),
            "the trait declaration is allowed through LINK_DEFINITION_PREFIXES"
        );
        assert_eq!(
            check_file("crates/domovoi-core/src/ports/reconcile.rs", text)[0].rule,
            rule,
            "another port is not the link's declaration: {text}"
        );
    }
    // The implementations and the fakes stay allowed, each for its own file.
    assert!(check_file("crates/domovoi-hub/src/agent_server/link.rs", cases[0].0).is_empty());
    assert!(
        check_file(
            "crates/domovoi-hub/src/agent_server/skill_policy.rs",
            cases[1].0
        )
        .is_empty()
    );
    assert!(check_file("crates/domovoi-testkit/src/link/mod.rs", cases[0].0).is_empty());
}

/// EXEC-4's core `ports/` exceptions are exact: the trait declaration file and the module
/// that re-exports it. Another port beside them may not name the link or take it raw.
#[test]
fn the_core_ports_exceptions_are_the_trait_file_and_its_reexport() {
    assert!(
        check_file(
            "crates/domovoi-core/src/ports/agent_link.rs",
            "pub trait AgentLink: Send + Sync {\n"
        )
        .is_empty(),
        "the trait declaration may not name its own trait"
    );
    assert!(
        check_file(
            "crates/domovoi-core/src/ports/mod.rs",
            "    ActionAnswers, AgentLink, AgentSnapshot,\n"
        )
        .is_empty(),
        "the re-export of the trait may not be refused"
    );
    let naming = "fn proxy(link: Arc<dyn AgentLink>) -> Host {\n";
    let refused = check_file("crates/domovoi-core/src/ports/reconcile.rs", naming);
    assert!(
        refused.iter().any(|v| v.rule == "EXEC-4"),
        "`{naming}` was accepted in another core port"
    );
    let accessor = "let presence = self.link().presence(host);\n";
    let refused = check_file("crates/domovoi-core/src/ports/agent_proxy.rs", accessor);
    assert!(
        refused.iter().any(|v| v.rule == "EXEC-4"),
        "`{accessor}` was accepted in another core port"
    );
    // The trait file keeps its own EXEC-1..3 exception; a neighbour does not.
    assert!(
        check_file(
            "crates/domovoi-core/src/ports/agent_link.rs",
            "link.start_skill(host, req, session, epoch, deadline).await;\n"
        )
        .is_empty(),
        "the trait file is the EXEC-2 exception"
    );
    assert_eq!(
        check_file(
            "crates/domovoi-core/src/ports/mod.rs",
            "link.start_skill(host, req, s, e, d).await;\n"
        )[0]
        .rule,
        "EXEC-2",
        "the re-export module is not a place to start a skill"
    );
}

#[test]
fn a_transport_naming_the_link_is_rejected() {
    for text in [
        "let link: Arc<dyn AgentLink> = server.link();\n",
        "let link = WsAgentLink::new(registry, clock, hosts);\n",
        "let link = PolicyLink::wrap(inner, policy);\n",
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

/// The returned type need not be written down: taking the link from the server is refused
/// wherever a transport lives, including a transport directory added later and the binary's
/// own source.
#[test]
fn a_transport_taking_the_raw_link_is_rejected() {
    for text in [
        "let link = server.link();\n",
        "let presence = self.link().presence(host);\n",
        "let link = hub.server.link_for_tests();\n",
    ] {
        assert_eq!(
            check_file("crates/domovoi-hub/src/api/hosts.rs", text)[0].rule,
            "EXEC-4",
            "{text} was accepted in a transport"
        );
        assert!(
            check_file("crates/domovoi/src/main.rs", text)
                .iter()
                .any(|x| x.rule == "EXEC-4"),
            "{text} was accepted in the binary's own source"
        );
    }
    assert_eq!(
        check_file(
            "crates/domovoi-hub/src/sse/events.rs",
            "let link = server.link();\n"
        )[0]
        .rule,
        "EXEC-4",
        "a transport directory added later is covered without an edit"
    );
    // The composition root and the link's own module may still take it.
    assert!(
        check_file(
            "crates/domovoi-hub/src/runtime/mod.rs",
            "let link = server.link();\n"
        )
        .is_empty()
    );
    assert!(
        check_file(
            "crates/domovoi-hub/src/agent_server/mod.rs",
            "let link = self.link();\n"
        )
        .is_empty()
    );
    // The suites that drive an agent's wire directly are test paths.
    assert!(
        check_file(
            "crates/domovoi-hub/tests/hub/sessions.rs",
            "let link = server.link_for_tests();\n"
        )
        .is_empty()
    );
    assert!(
        check_file(
            "crates/domovoi/tests/agent_hub.rs",
            "let link = server.link_for_tests();\n"
        )
        .is_empty()
    );
    // The rule is scoped to the scanned crates: a method named `link` elsewhere is not this.
    assert!(
        check_file(
            "crates/domovoi-store/src/urls.rs",
            "let parts = url.link();\n"
        )
        .is_empty()
    );
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
