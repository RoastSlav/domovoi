//! Tests for the helper read rules: UNIT-3 over the widened helper unit, HELPER-1 over the
//! agent's `helper_client/` directory, READ-1 and the helper's no-spawn rule (R3).

use super::Violation;
use super::identity_rules::{check_file as identity_check, check_read_file};
use super::scan::check_file as scan_check;
use super::units::{UnitFile, check_units};

const DOC: &str = "//! Crate doc.\n";
const HELPER_UNIT_TEXT: &str = include_str!("../../../deploy/systemd/domovoi-helper@.service");

fn rules(found: &[Violation]) -> Vec<&'static str> {
    found.iter().map(|v| v.rule).collect()
}

fn unit3(text: &str) -> Vec<Violation> {
    let units = [UnitFile::parse("domovoi-helper@.service", text)];
    check_units(&units, None, false)
        .into_iter()
        .filter(|v| v.rule == "UNIT-3")
        .collect()
}

fn replace(from: &str, to: &str) -> String {
    assert!(HELPER_UNIT_TEXT.contains(from), "unit lacks `{from}`");
    HELPER_UNIT_TEXT.replacen(from, to, 1)
}

#[test]
fn the_shipped_helper_unit_passes_unit_3() {
    assert!(unit3(HELPER_UNIT_TEXT).is_empty());
}

#[test]
fn unit_parser_flags_helper_unit_with_an_extra_capability() {
    let wide = replace("CAP_DAC_READ_SEARCH", "CAP_DAC_READ_SEARCH CAP_SYS_MODULE");
    let found = unit3(&wide);
    assert!(
        found
            .iter()
            .any(|v| v.message.contains("CapabilityBoundingSet")),
        "{found:?}"
    );
    let missing = replace(" CAP_SYSLOG", "");
    assert!(!unit3(&missing).is_empty());
    let ambient = replace(
        "AmbientCapabilities=\n",
        "AmbientCapabilities=CAP_SYS_ADMIN\n",
    );
    assert!(!unit3(&ambient).is_empty());
}

#[test]
fn unit_parser_flags_helper_unit_with_private_devices_yes_or_without_device_policy_closed() {
    let hidden = replace("PrivateDevices=no", "PrivateDevices=yes");
    assert!(
        unit3(&hidden)
            .iter()
            .any(|v| v.message.contains("PrivateDevices")),
        "private devices must be off"
    );
    let open = replace("DevicePolicy=closed\n", "");
    assert!(
        unit3(&open)
            .iter()
            .any(|v| v.message.contains("DevicePolicy")),
        "device policy must be closed"
    );
    let loose = replace("DevicePolicy=closed", "DevicePolicy=auto");
    assert!(!unit3(&loose).is_empty());
    // The device list is exact too: no extra device, no write access.
    let extra = replace(
        "DeviceAllow=char-sg r\n",
        "DeviceAllow=char-sg r\nDeviceAllow=/dev/mem r\n",
    );
    assert!(!unit3(&extra).is_empty());
    let writable = replace("DeviceAllow=block-sd r\n", "DeviceAllow=block-sd rw\n");
    assert!(!unit3(&writable).is_empty());
}

#[test]
fn helper_client_directory_is_the_only_agent_code_naming_helper_requests() {
    let body = "use domovoi_core::helper::HelperRequest;\nlet c = HelperClient::connect();\n";
    let scan = |rel: &str| rules(&identity_check(rel, &format!("{DOC}{body}"), false));
    for rel in [
        "crates/domovoi-agent/src/helper_client/mod.rs",
        "crates/domovoi-agent/src/helper_client/reads.rs",
        "crates/domovoi-agent/src/helper_client/container_source.rs",
        "crates/domovoi-agent/src/runtime.rs",
    ] {
        assert!(!scan(rel).contains(&"HELPER-1"), "{rel}");
    }
    for rel in [
        "crates/domovoi-agent/src/docker.rs",
        "crates/domovoi-agent/src/production.rs",
        "crates/domovoi-agent/src/requests/tools.rs",
        "crates/domovoi-agent/src/helper_clients.rs",
        "crates/domovoi-checks/src/smart/mod.rs",
    ] {
        assert!(scan(rel).contains(&"HELPER-1"), "{rel}");
    }
}

#[test]
fn bollard_is_used_only_in_checks_and_the_helper_read_module() {
    let body = "use bollard::Docker;\n";
    let scan = |rel: &str| rules(&check_read_file(rel, &format!("{DOC}{body}"), false));
    for rel in [
        "crates/domovoi-checks/src/containers/docker.rs",
        "crates/domovoi-helper/src/reads/docker/client.rs",
        "crates/domovoi-helper/src/reads/docker/mod.rs",
    ] {
        assert!(scan(rel).is_empty(), "{rel}");
    }
    for rel in [
        "crates/domovoi-agent/src/docker.rs",
        "crates/domovoi-checks/src/containers/mod.rs",
        "crates/domovoi-helper/src/reads/mod.rs",
        "crates/domovoi-helper/src/reads/tools/smart.rs",
        "crates/domovoi-helper/src/serve/dispatch.rs",
        "crates/domovoi-hub/src/services/mod.rs",
    ] {
        assert_eq!(scan(rel), ["READ-1"], "{rel}");
    }
    // Test sources and comments are not uses.
    assert!(check_read_file("crates/domovoi-helper/tests/reads_docker.rs", body, true).is_empty());
    assert!(
        scan_check(
            "crates/domovoi-agent/src/docker.rs",
            &format!("{DOC}// bollard\n"),
            false
        )
        .iter()
        .all(|v| v.rule != "READ-1")
    );
}

#[test]
fn helper_crate_spawns_nothing_directly() {
    for body in [
        "use std::process::Command;\n",
        "let c = tokio::process::Command::new(\"ss\");\n",
        "let c = std::process::Command::new(\"smartctl\");\n",
    ] {
        for rel in [
            "crates/domovoi-helper/src/reads/tools/smart.rs",
            "crates/domovoi-helper/src/serve/dispatch.rs",
        ] {
            let found = scan_check(rel, &format!("{DOC}{body}"), false);
            assert!(rules(&found).contains(&"R3"), "{rel}: {body}: {found:?}");
        }
    }
}
