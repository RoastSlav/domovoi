//! Tests for RUNNER-1 and RUNNER-2: each rule gets a failing source, its allowed places and the
//! ordinary second path it exists to refuse.

use super::*;

const DOC: &str = "//! Crate doc.\n";

fn rules(found: &[Violation]) -> Vec<&'static str> {
    found.iter().map(|v| v.rule).collect()
}

fn scan(rel: &str, body: &str) -> Vec<Violation> {
    check_file(rel, &format!("{DOC}{body}"), false)
}

#[test]
fn runner_socket_literal_is_defined_once() {
    let bodies = [
        "const S: &str = \"/run/domovoi-skill-runner.sock\";\n",
        "let r: RunnerRequest = read();\n",
        "use domovoi_core::skill_runner::RunnerRequest;\n",
    ];
    let allowed = [
        "crates/domovoi-core/src/skill_runner/mod.rs",
        "crates/domovoi-agent/src/skill_runner/client.rs",
        "crates/domovoi-runner/src/serve/connection.rs",
        "crates/domovoi/src/cmd/skill_runner.rs",
        "crates/domovoi-testkit/src/runner.rs",
    ];
    let refused = [
        "crates/domovoi-hub/src/api/mod.rs",
        "crates/domovoi-engine/src/lib.rs",
        "crates/domovoi-agent/src/runtime.rs",
        "crates/domovoi-agent/src/link/mod.rs",
        "crates/domovoi-runner/src/exec/mod.rs",
        "crates/domovoi-helper/src/probe.rs",
        "crates/domovoi/src/cmd/agent.rs",
        "crates/domovoi-core/src/isolation/boundary.rs",
    ];
    for body in bodies {
        for rel in allowed {
            assert!(
                !rules(&scan(rel, body)).contains(&"RUNNER-1"),
                "{rel}: {body}"
            );
        }
        for rel in refused {
            assert!(
                rules(&scan(rel, body)).contains(&"RUNNER-1"),
                "{rel}: {body}"
            );
        }
    }
    // Tests, comments, the socket unit's name and unrelated words are not mentions.
    let test = check_file(
        "crates/domovoi-hub/tests/x.rs",
        "let p = \"/run/domovoi-skill-runner.sock\";\n",
        true,
    );
    assert!(test.is_empty());
    let inline_test = scan(
        "crates/domovoi-hub/src/a.rs",
        "#[cfg(test)]\nmod tests {\n    const P: &str = \"/run/domovoi-skill-runner.sock\";\n}\n",
    );
    assert!(inline_test.is_empty(), "{inline_test:?}");
    assert!(
        scan(
            "crates/domovoi-hub/src/a.rs",
            "// domovoi-skill-runner.sock and RunnerRequest\nlet unit = \"domovoi-skill-runner.socket\";\nlet MyRunnerRequestX = 1;\n"
        )
        .is_empty()
    );
}

#[test]
fn runner_socket_path_has_one_definition_in_core() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace");
    let module = root.join("crates/domovoi-core/src/skill_runner/mod.rs");
    let text = std::fs::read_to_string(module).expect("core defines the runner module");
    let literals = text.matches("/run/domovoi-skill-runner.sock").count();
    assert_eq!(literals, 1, "the path is defined once, in core's module");
}

#[test]
fn runner_serve_imports_no_helper_or_agent_state() {
    let serve = "crates/domovoi-runner/src/serve/connection.rs";
    let bodies = [
        "use domovoi_helper::probe::collect;\n",
        "use domovoi_agent::requests::RequestCtx;\n",
        "let c: HelperClient = connect();\n",
        "fn f(status: &AgentStatusSource) {}\n",
        "let r = domovoi_agent::skill_runner::RunnerRoute::new();\n",
    ];
    for body in bodies {
        assert!(rules(&scan(serve, body)).contains(&"RUNNER-2"), "{body}");
    }
    // The runner's own types, core and the injected ports are fine.
    let fine =
        "use domovoi_core::skill_runner::RunnerRequest;\nuse domovoi_core::ports::HostInspect;\n";
    assert!(scan(serve, fine).is_empty());
    // The rule is about the serve tree only: R1 keeps the rest of the crate honest.
    assert!(
        !rules(&scan(
            "crates/domovoi-runner/src/exec/mod.rs",
            "use domovoi_helper::probe::collect;\n"
        ))
        .contains(&"RUNNER-2")
    );
    // Tests of the serve tree may name anything.
    assert!(
        check_file(
            "crates/domovoi-runner/tests/runner/serve.rs",
            "use domovoi_helper::probe::collect;\n",
            true
        )
        .is_empty()
    );
}
