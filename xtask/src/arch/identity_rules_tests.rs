//! Tests for the helper and identity source rules: each rule gets a failing source, its allowed
//! places and the ordinary second path it exists to refuse.

use super::*;

const DOC: &str = "//! Crate doc.\n";

fn rules(found: &[Violation]) -> Vec<&'static str> {
    found.iter().map(|v| v.rule).collect()
}

fn scan(rel: &str, body: &str) -> Vec<Violation> {
    check_file(rel, &format!("{DOC}{body}"), false)
}

#[test]
fn helper_socket_and_request_are_named_only_in_allowed_files() {
    let bodies = [
        "use domovoi_core::helper::HelperRequest;\n",
        "let frame: HelperRequestFrame = read();\n",
        "let c = HelperClient::connect();\n",
        "const S: &str = HELPER_SOCKET;\n",
        "let path = \"/run/domovoi-helper.sock\";\n",
    ];
    let allowed = [
        "crates/domovoi-core/src/helper/request.rs",
        "crates/domovoi-helper/src/serve.rs",
        "crates/domovoi-agent/src/helper_client.rs",
        "crates/domovoi-agent/src/runtime.rs",
        "crates/domovoi/src/cmd/helper.rs",
        "crates/domovoi-testkit/src/helper.rs",
    ];
    let refused = [
        "crates/domovoi-hub/src/api/mod.rs",
        "crates/domovoi-engine/src/lib.rs",
        "crates/domovoi-agent/src/link/mod.rs",
        "crates/domovoi-runner/src/exec/mod.rs",
        "crates/domovoi-core/src/identity/paths.rs",
    ];
    for body in bodies {
        for rel in allowed {
            assert!(
                !rules(&scan(rel, body)).contains(&"HELPER-1"),
                "{rel}: {body}"
            );
        }
        for rel in refused {
            assert!(
                rules(&scan(rel, body)).contains(&"HELPER-1"),
                "{rel}: {body}"
            );
        }
    }
    // Tests, comments and unrelated words are not mentions.
    let test = check_file(
        "crates/domovoi-hub/tests/x.rs",
        "let c = HelperClient::new();\n",
        true,
    );
    assert!(test.is_empty());
    assert!(
        scan(
            "crates/domovoi-hub/src/a.rs",
            "// HelperClient and domovoi-helper.sock\nlet MyHelperClient = 1;\n"
        )
        .is_empty()
    );
}

#[test]
fn helper_state_paths_are_named_only_in_allowed_files() {
    for body in [
        "let d = \"/var/lib/domovoi-helper\";\n",
        "let k = dir.join(\"attest.key\");\n",
        "let l = dir.join(\"state.lock\");\n",
        "let p = dir.join(\"platform.json\");\n",
    ] {
        assert!(rules(&scan("crates/domovoi-agent/src/a.rs", body)).contains(&"HELPER-2"));
        assert!(
            !rules(&scan("crates/domovoi-helper/src/start_check.rs", body)).contains(&"HELPER-2")
        );
        assert!(
            !rules(&scan("crates/domovoi-core/src/identity/paths.rs", body)).contains(&"HELPER-2")
        );
    }
    // A method call named `state.lock()` is code, not a path.
    assert!(
        scan(
            "crates/domovoi-hub/src/a.rs",
            "let g = self.state.lock();\n"
        )
        .is_empty()
    );
}

#[test]
fn skill_account_name_has_one_definition_and_no_numeric_ids_nearby() {
    let skill = "pub const SKILL_ACCOUNT: &str = \"domovoi-skill\";\n";
    let found = scan("crates/domovoi-helper/src/inspect/unix.rs", skill);
    assert_eq!(rules(&found), ["IDENT-1"], "{found:?}");
    assert!(scan("crates/domovoi-core/src/identity/accounts.rs", skill).is_empty());
    // The skill literal is IDENT-1's alone; the agent literal is ID-2's.
    let agent = "let n = \"domovoi-agent\";\n";
    assert_eq!(rules(&scan("crates/domovoi-hub/src/a.rs", agent)), ["ID-2"]);
    for rel in [
        "crates/domovoi-core/src/identity/accounts.rs",
        "crates/domovoi-helper/src/inspect/unix.rs",
        "crates/domovoi-helper/src/selftest/units.rs",
        "crates/domovoi-helper/src/start_check.rs",
    ] {
        assert!(scan(rel, agent).is_empty(), "{rel}");
    }
    // Group-membership calls.
    let call = "let g = groups_of(&user);\nlet h = u.supplementary_groups();\n";
    assert_eq!(
        rules(&scan("crates/domovoi-agent/src/a.rs", call)),
        ["ID-2", "ID-2"]
    );
    assert!(scan("crates/domovoi-core/src/identity/accounts.rs", call).is_empty());
    // Numeric ids within three lines of an account constant, in the file that defines it.
    let near = "pub const AGENT_ACCOUNT: &str = \"domovoi-agent\";\n\nlet entry = UserEntry { uid: 1001, gid: 0 };\n";
    let found = scan("crates/domovoi-core/src/identity/accounts.rs", near);
    assert_eq!(rules(&found), ["ID-2"], "{found:?}");
    assert!(found[0].location.ends_with(":4"), "{found:?}");
    let zero = "pub const AGENT_ACCOUNT: &str = \"domovoi-agent\";\nlet entry = UserEntry { uid: 0, gid: 0 };\n";
    assert!(scan("crates/domovoi-core/src/identity/accounts.rs", zero).is_empty());
    let far = "pub const AGENT_ACCOUNT: &str = \"domovoi-agent\";\n\n\n\n\nlet uid = 1001;\n";
    assert!(scan("crates/domovoi-core/src/identity/accounts.rs", far).is_empty());
    let typed = "pub const AGENT_ACCOUNT: &str = \"domovoi-agent\";\npub struct E { pub uid: u32, pub gid: u32 }\n";
    assert!(scan("crates/domovoi-core/src/identity/accounts.rs", typed).is_empty());
}

#[test]
fn inline_test_modules_and_test_paths_are_exempt() {
    let body = "fn f() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let a = \"domovoi-agent\";\n    }\n}\nlet b = \"domovoi-agent\";\n";
    let found = scan("crates/domovoi-hub/src/a.rs", body);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].location.ends_with(":9"));
    assert!(
        check_file(
            "crates/domovoi-hub/src/a_tests.rs",
            "let a = \"domovoi-agent\";\n",
            false
        )
        .is_empty()
    );
}

#[test]
fn no_code_parses_systemd_instance_names() {
    for body in [
        "let (t, i) = unit.split_once('@')?;\n",
        "let i = unit.rsplit('@').next();\n",
        "let parts = name.split(\"@\");\n",
        "let t = name.rsplit_once( '@' );\n",
        "let name = \"domovoi-helper@selftest.service\";\n",
    ] {
        for rel in [
            "crates/domovoi-helper/src/serve.rs",
            "crates/domovoi-agent/src/helper_client.rs",
            "crates/domovoi-runner/src/exec/mod.rs",
        ] {
            assert!(rules(&scan(rel, body)).contains(&"ID-3"), "{rel}: {body}");
        }
        // Outside the three places the rule does not apply.
        assert!(!rules(&scan("crates/domovoi-hub/src/a.rs", body)).contains(&"ID-3"));
    }
    // The instance literal is allowed in the one file that names the instance.
    let literal = "const T: &str = \"domovoi-helper@\";\n";
    assert!(scan("crates/domovoi-helper/src/selftest/units.rs", literal).is_empty());
    // A split on another character, and an e-mail check in a comment, are fine.
    assert!(
        scan(
            "crates/domovoi-helper/src/serve.rs",
            "let p = s.split(':');\n// split('@')\n"
        )
        .is_empty()
    );
}

#[test]
fn declarations_and_unit_names_are_not_calls_or_socket_paths() {
    let declared = "trait T {\n    fn groups_of(&self) -> u32;\n    fn supplementary_groups(&self);\n}\nlet groups_of = 3;\n";
    assert!(scan("crates/domovoi-hub/src/a.rs", declared).is_empty());
    let socket_unit = "let unit = \"domovoi-helper.socket\";\n";
    assert!(scan("crates/domovoi-hub/src/a.rs", socket_unit).is_empty());
}

#[test]
fn runner_start_checks_may_call_group_membership_but_no_other_runner_file_may() {
    let call = "let groups = inspect.supplementary_groups();\n";
    assert!(scan("crates/domovoi-runner/src/serve/start_checks.rs", call).is_empty());
    for rel in [
        "crates/domovoi-runner/src/serve/connection.rs",
        "crates/domovoi-runner/src/exec/mod.rs",
        "crates/domovoi-agent/src/runtime.rs",
    ] {
        assert_eq!(rules(&scan(rel, call)), ["ID-2"], "{rel}");
    }
}
