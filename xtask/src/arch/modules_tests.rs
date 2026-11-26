//! Tests for the hub's module-edge rules (MOD-1 to MOD-4). The rules themselves live in
//! `modules.rs`; the zone and forbidden-name data lives in `rules.rs`.

use super::*;

const INTERACTIONS: &str = "crates/domovoi-hub/src/auth/interactions/mod.rs";
const HTTP: &str = "crates/domovoi-hub/src/api/x.rs";
const CHAT: &str = "crates/domovoi-hub/src/auth/interactions/x.rs";
const WEBHOOK: &str = "crates/domovoi-hub/src/ingest/x.rs";
const SERVICE: &str = "crates/domovoi-hub/src/services/x.rs";
const RUNTIME: &str = "crates/domovoi-hub/src/runtime/x.rs";

fn check(files: &[(&str, &str)]) -> Vec<Violation> {
    let owned: Vec<(String, String)> = files
        .iter()
        .map(|(rel, text)| ((*rel).to_string(), (*text).to_string()))
        .collect();
    module_edge_violations(&owned)
}

fn count(violations: &[Violation], rule: &str) -> usize {
    violations
        .iter()
        .filter(|violation| violation.rule == rule)
        .count()
}

fn has(violations: &[Violation], rule: &str) -> bool {
    count(violations, rule) > 0
}

#[test]
fn chat_importing_http_is_refused() {
    let violations = check(&[(INTERACTIONS, "use crate::api::ApiState;\n")]);
    assert_eq!(count(&violations, "MOD-1"), 1, "{violations:#?}");
    let found = violations
        .iter()
        .find(|violation| violation.rule == "MOD-1")
        .expect("one MOD-1");
    assert!(found.location.starts_with(INTERACTIONS), "{found:#?}");
    assert!(found.location.contains(':'), "{found:#?}");
}

#[test]
fn http_importing_chat_is_refused() {
    let violations = check(&[(HTTP, "use crate::auth::interactions::Actor;\n")]);
    assert_eq!(count(&violations, "MOD-1"), 1, "{violations:#?}");
}

#[test]
fn webhook_importing_http_is_refused() {
    let violations = check(&[(
        WEBHOOK,
        "fn f() { let _ = crate::api::error::ErrorBody::default(); }\n",
    )]);
    assert!(has(&violations, "MOD-1"), "{violations:#?}");
}

#[test]
fn a_super_path_is_resolved() {
    let climbs_to_api = check(&[(
        "crates/domovoi-hub/src/auth/interactions/buttons.rs",
        "use super::super::super::api::X;\n",
    )]);
    assert!(has(&climbs_to_api, "MOD-1"), "{climbs_to_api:#?}");

    let sibling = check(&[(
        "crates/domovoi-hub/src/auth/interactions/buttons.rs",
        "use super::Accepted;\n",
    )]);
    assert!(sibling.is_empty(), "{sibling:#?}");
}

#[test]
fn a_renamed_import_is_resolved() {
    let violations = check(&[(
        CHAT,
        "use crate::api as web;\nfn f() { web::hosts::host_dto(1); }\n",
    )]);
    assert!(has(&violations, "MOD-1"), "{violations:#?}");
}

#[test]
fn a_fully_qualified_expression_path_is_counted() {
    let violations = check(&[(
        "crates/domovoi-hub/src/auth/interactions/commands.rs",
        "fn f() { crate::api::incidents::latest_triage(&t); }\n",
    )]);
    assert!(has(&violations, "MOD-1"), "{violations:#?}");
}

#[test]
fn a_macro_body_cannot_hide_an_edge() {
    let violations = check(&[(
        "crates/domovoi-hub/src/auth/interactions/commands.rs",
        "fn f() { let _ = format!(\"{}\", crate::api::version::HUB_VERSION); }\n",
    )]);
    assert!(has(&violations, "MOD-1"), "{violations:#?}");
}

#[test]
fn services_may_not_import_a_transport_or_the_runtime() {
    let violations = check(&[(
        SERVICE,
        "use crate::api::dto::HostDto;\n\
         use crate::auth::interactions::Accepted;\n\
         use crate::ingest::IngestState;\n\
         use crate::runtime::support;\n",
    )]);
    assert_eq!(count(&violations, "MOD-2"), 4, "{violations:#?}");
    assert!(
        violations.iter().all(|violation| violation.rule == "MOD-2"),
        "{violations:#?}"
    );
}

#[test]
fn services_may_import_auth_approval_agent_server_profile_and_digest() {
    let violations = check(&[(
        SERVICE,
        "use crate::auth::approval::approval_for;\n\
         use crate::agent_server::AgentAdmin;\n\
         use crate::profile::ProfileDeps;\n\
         use crate::digest::DigestDeps;\n",
    )]);
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn transports_may_import_services() {
    for rel in [HTTP, CHAT] {
        let violations = check(&[(rel, "use crate::services::Services;\n")]);
        assert!(violations.is_empty(), "{rel}: {violations:#?}");
    }
}

#[test]
fn runtime_router_and_unzoned_modules_may_import_every_transport() {
    for rel in [
        RUNTIME,
        "crates/domovoi-hub/src/router.rs",
        "crates/domovoi-hub/src/settings/x.rs",
    ] {
        let violations = check(&[(
            rel,
            "use crate::api::ApiState;\n\
             use crate::auth::interactions::InteractionAuthorizer;\n\
             use crate::ingest::IngestState;\n",
        )]);
        assert!(violations.is_empty(), "{rel}: {violations:#?}");
    }
}

#[test]
fn a_new_top_level_module_must_be_declared() {
    let undeclared = check(&[("crates/domovoi-hub/src/operator/mod.rs", "pub fn f() {}\n")]);
    assert_eq!(count(&undeclared, "MOD-3"), 1, "{undeclared:#?}");

    let root = check(&[("crates/domovoi-hub/src/lib.rs", "pub fn f() {}\n")]);
    assert!(root.is_empty(), "{root:#?}");
}

#[test]
fn a_transport_naming_a_command_handle_is_refused() {
    let in_type = "struct S {\n    h: PackageHandle,\n}\n";
    let in_path = "fn f() {\n    let _ = domovoi_engine::PackageHandle::new();\n}\n";
    let in_macro = "fn g() {\n    let _ = format!(\"{}\", PackageHandle::NAME);\n}\n";
    for text in [in_type, in_path, in_macro] {
        let violations = check(&[(HTTP, text)]);
        assert!(has(&violations, "MOD-4"), "{text}: {violations:#?}");
    }
    let all = check(&[(HTTP, &format!("{in_type}{in_path}{in_macro}"))]);
    assert!(count(&all, "MOD-4") >= 3, "{all:#?}");

    let operation = check(&[(CHAT, "struct S {\n    h: OperationHandle,\n}\n")]);
    assert!(has(&operation, "MOD-4"), "{operation:#?}");

    let names = format!("{in_type}{in_path}{in_macro}");
    for rel in [SERVICE, RUNTIME] {
        let violations = check(&[(rel, &names)]);
        assert!(!has(&violations, "MOD-4"), "{rel}: {violations:#?}");
    }
}

#[test]
fn a_cfg_test_module_is_not_checked() {
    let violations = check(&[(
        CHAT,
        "#[cfg(test)]\nmod t {\n    use crate::api::ApiState;\n}\n",
    )]);
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn files_outside_the_hub_are_ignored() {
    let violations = check(&[(
        "crates/domovoi-engine/src/x.rs",
        "use crate::api::ApiState;\n",
    )]);
    assert!(violations.is_empty(), "{violations:#?}");
}

#[test]
fn an_unparsable_file_is_a_violation() {
    let violations = check(&[(HTTP, "fn (((\n")]);
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(violations[0].rule, "MOD-1", "{violations:#?}");
    assert_eq!(violations[0].location, HTTP, "{violations:#?}");
}

#[test]
fn zones_do_not_overlap() {
    for (i, a) in ZONES.iter().enumerate() {
        for b in ZONES.iter().skip(i + 1) {
            assert_ne!(
                a.module, b.module,
                "zones {} and {} share the module `{}`",
                a.name, b.name, a.module
            );
        }
    }
}

#[test]
fn an_inline_module_resolves_super_from_its_own_module() {
    let chat = "crates/domovoi-hub/src/auth/interactions/mod.rs";
    let found = check(&[(
        chat,
        "mod nested { use super::super::super::api::ApiState; }",
    )]);
    assert!(found.iter().any(|v| v.rule == "MOD-1"), "{found:#?}");
    let services = "crates/domovoi-hub/src/services/x.rs";
    let found = check(&[(
        services,
        "mod a { mod b { use super::super::super::super::runtime::support; } }",
    )]);
    assert!(found.iter().any(|v| v.rule == "MOD-2"), "{found:#?}");
}

#[test]
fn an_inline_module_may_import_within_its_own_zone() {
    let chat = "crates/domovoi-hub/src/auth/interactions/mod.rs";
    let found = check(&[(
        chat,
        "mod nested { use super::Accepted; mod deeper { use super::super::Actor; } }",
    )]);
    assert!(found.is_empty(), "{found:#?}");
}
