//! Tests for the unit parser and the unit rules: each rule gets a failing input, and the real
//! `deploy/` tree is the gate.

use std::path::Path;

use super::*;

const HELPER_SERVICE: &str = "\
[Unit]
Description=Domovoi helper (connection %i)

[Service]
Type=simple
# Root: no User= line on purpose.
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi helper serve
Restart=no
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=no
DevicePolicy=closed
DeviceAllow=block-sd r
DeviceAllow=block-nvme r
DeviceAllow=char-nvme r
DeviceAllow=char-sg r
CapabilityBoundingSet=CAP_NET_ADMIN CAP_SYS_RAWIO CAP_SYS_ADMIN CAP_SYSLOG CAP_SYS_PTRACE CAP_DAC_READ_SEARCH
AmbientCapabilities=
RuntimeMaxSec=60s
KillMode=control-group
TimeoutStopSec=5s
";

const HELPER_SOCKET: &str = "\
[Unit]
Description=Domovoi helper socket

[Socket]
ListenStream=/run/domovoi-helper.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=8
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
";

const AGENT_SERVICE: &str = "\
[Unit]
Description=Domovoi agent

[Service]
Type=simple
User=domovoi-agent
Group=domovoi-agent
ExecStart=/usr/local/bin/domovoi agent run
ReadWritePaths=/var/lib/domovoi-agent /var/lib/domovoi-packages
";

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

fn created() -> BTreeSet<String> {
    set(&[
        "/etc/domovoi",
        "/var/lib/domovoi-agent",
        "/var/lib/domovoi-packages",
        "/var/lib/domovoi-helper",
    ])
}

fn baseline() -> Vec<UnitFile> {
    vec![
        UnitFile::parse("domovoi-helper@.service", HELPER_SERVICE),
        UnitFile::parse("domovoi-helper.socket", HELPER_SOCKET),
        UnitFile::parse("domovoi-agent.service", AGENT_SERVICE),
    ]
}

fn replace(unit: &str, from: &str, to: &str) -> String {
    assert!(unit.contains(from), "fixture lacks `{from}`");
    unit.replacen(from, to, 1)
}

fn rules_of(found: &[Violation]) -> Vec<&'static str> {
    found.iter().map(|v| v.rule).collect()
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
}

#[test]
fn unit_parser_keeps_order_repeats_continuations_and_comments() {
    let text = "\
; leading comment
[Service]
ExecStart=/bin/a \\
    --flag \\
# comment inside a continuation
    --other
ReadWritePaths=/one
ReadWritePaths=/two
[Install]
WantedBy=multi-user.target
";
    let entries = parse_unit(text);
    let triples: Vec<(&str, &str, &str)> = entries
        .iter()
        .map(|e| (e.section.as_str(), e.key.as_str(), e.value.as_str()))
        .collect();
    assert_eq!(
        triples,
        [
            ("Service", "ExecStart", "/bin/a  --flag  --other"),
            ("Service", "ReadWritePaths", "/one"),
            ("Service", "ReadWritePaths", "/two"),
            ("Install", "WantedBy", "multi-user.target"),
        ]
    );
    assert_eq!(entries[1].line, 7);
}

#[test]
fn a_correct_set_of_units_passes() {
    let found = check_units(&baseline(), Some(&created()), true);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn unit_parser_flags_runtime_max_with_oneshot() {
    let unit = replace(HELPER_SERVICE, "Type=simple", "Type=oneshot");
    let units = [UnitFile::parse("x.service", &unit)];
    let found = check_units(&units, Some(&created()), true);
    assert!(rules_of(&found).contains(&"UNIT-4"), "{found:?}");
    let no_type = replace(HELPER_SERVICE, "Type=simple\n", "");
    let units = [UnitFile::parse("x.service", &no_type)];
    assert!(rules_of(&check_units(&units, Some(&created()), true)).contains(&"UNIT-4"));
}

#[test]
fn unit_parser_flags_wide_socket_mode() {
    let wide = replace(HELPER_SOCKET, "SocketMode=0660", "SocketMode=0666");
    let wrong_group = replace(
        HELPER_SOCKET,
        "SocketGroup=domovoi-agent",
        "SocketGroup=users",
    );
    let elsewhere = replace(HELPER_SOCKET, "/run/domovoi-helper.sock", "/run/other.sock");
    for text in [wide, wrong_group, elsewhere] {
        let units = [UnitFile::parse("domovoi-helper.socket", &text)];
        let found = check_units(&units, Some(&created()), true);
        assert!(rules_of(&found).contains(&"UNIT-5"), "{text}\n{found:?}");
    }
}

#[test]
fn unit_parser_flags_nonempty_supplementary_groups_and_docker() {
    let docker = replace(
        AGENT_SERVICE,
        "Group=domovoi-agent",
        "SupplementaryGroups=systemd-journal docker",
    );
    let units = [UnitFile::parse("domovoi-agent.service", &docker)];
    let found = check_units(&units, Some(&created()), true);
    assert!(rules_of(&found).contains(&"UNIT-2"), "{found:?}");
    let group = replace(AGENT_SERVICE, "Group=domovoi-agent", "Group=docker");
    let units = [UnitFile::parse("domovoi-agent.service", &group)];
    assert!(rules_of(&check_units(&units, Some(&created()), true)).contains(&"UNIT-2"));
    // The helper unit takes no supplementary groups at all.
    let helper = format!("{HELPER_SERVICE}SupplementaryGroups=systemd-journal\n");
    let units = [UnitFile::parse(HELPER_UNIT, &helper)];
    let found = check_units(&units, Some(&created()), true);
    assert!(
        found
            .iter()
            .any(|v| v.rule == "UNIT-3" && v.message.contains("SupplementaryGroups")),
        "{found:?}"
    );
}

#[test]
fn unit_parser_flags_dash_prefixed_mask_of_installer_path() {
    let dashed = replace(
        HELPER_SERVICE,
        "ReadWritePaths=/var/lib/domovoi-helper",
        "ReadWritePaths=-/var/lib/domovoi-helper",
    );
    let units = [UnitFile::parse("x.service", &dashed)];
    let found = check_units(&units, Some(&created()), true);
    assert!(
        found
            .iter()
            .any(|v| v.rule == "UNIT-7" && v.message.contains("no `-` prefix")),
        "{found:?}"
    );
    let uncreated = format!(
        "{HELPER_SERVICE}InaccessiblePaths=/var/lib/other\nBindReadOnlyPaths=/etc/domovoi:/etc/domovoi:ro\nTemporaryFileSystem=/run:ro\nReadWritePaths=-/optional /var/lib\n"
    );
    let units = [UnitFile::parse("x.service", &uncreated)];
    let found = check_units(&units, Some(&created()), true);
    let unit7: Vec<_> = found.iter().filter(|v| v.rule == "UNIT-7").collect();
    assert_eq!(unit7.len(), 1, "{found:?}");
    assert!(unit7[0].message.contains("/var/lib/other"));
}

#[test]
fn unit_with_path_directives_needs_a_created_set_only_when_required() {
    let units = baseline();
    assert!(check_units(&units, None, false).is_empty());
    let found = check_units(&units, None, true);
    assert_eq!(rules_of(&found), ["UNIT-7"], "reported once: {found:?}");
}

#[test]
fn unit_parser_flags_agent_unit_without_user() {
    let no_user = replace(AGENT_SERVICE, "User=domovoi-agent\n", "");
    let mut units = baseline();
    units[2] = UnitFile::parse("domovoi-agent.service", &no_user);
    let found = check_units(&units, Some(&created()), true);
    assert_eq!(rules_of(&found), ["UNIT-8"], "{found:?}");
    units.pop();
    assert!(rules_of(&check_units(&units, Some(&created()), true)).contains(&"UNIT-8"));
}

#[test]
fn unit_parser_flags_writable_docker_socket_in_compose() {
    let compose = |entry: &str| {
        vec![(
            "deploy/compose/agent.yml".to_string(),
            format!(
                "services:\n  proxy:\n    image: proxy\n    volumes:\n      - {entry}\n      - ./data:/data\n    ports:\n      - \"80:80\"\n"
            ),
        )]
    };
    for writable in [
        "/var/run/docker.sock:/var/run/docker.sock",
        "\"/var/run/docker.sock:/var/run/docker.sock:rw\"",
    ] {
        let found = check_compose(&compose(writable));
        assert_eq!(rules_of(&found), ["UNIT-8"], "{writable}: {found:?}");
        assert!(found[0].location.ends_with(":5"), "{found:?}");
    }
    for read_only in [
        "/var/run/docker.sock:/var/run/docker.sock:ro",
        "'/var/run/docker.sock:/var/run/docker.sock:ro,z'",
    ] {
        assert!(check_compose(&compose(read_only)).is_empty(), "{read_only}");
    }
    // A mention outside `volumes:` (an environment line, a port list) is not a mount.
    let other = vec![(
        "c.yml".to_string(),
        "services:\n  a:\n    environment:\n      - DOCKER_HOST=unix:///var/run/docker.sock\n"
            .to_string(),
    )];
    assert!(check_compose(&other).is_empty());
}

#[test]
fn unit_parser_flags_exec_under_run() {
    for exec in [
        "ExecStart=/run/domovoi/helper serve",
        "ExecStart=-/tmp/helper",
        "ExecStartPre=/dev/shm/prep",
        "ExecStopPost=@/run/x y",
    ] {
        let key = exec.split('=').next().unwrap_or_default();
        let text = format!("[Service]\nType=simple\n{exec}\n");
        let units = [UnitFile::parse("x.service", &text)];
        let found = check_units(&units, Some(&created()), true);
        assert!(
            found
                .iter()
                .any(|v| v.rule == "UNIT-9" && v.message.contains(key)),
            "{exec}: {found:?}"
        );
    }
    // An argument under /run is not an exec path.
    let fine = "[Service]\nExecStart=/usr/local/bin/domovoi serve --socket /run/x.sock\n";
    let units = [UnitFile::parse("x.service", fine)];
    assert!(!rules_of(&check_units(&units, Some(&created()), true)).contains(&"UNIT-9"));
}

#[test]
fn unit_parser_flags_missing_or_changed_helper_directive() {
    let changed = [
        replace(HELPER_SERVICE, "RuntimeMaxSec=60s", "RuntimeMaxSec=120s"),
        replace(HELPER_SERVICE, "ProtectHome=yes\n", ""),
        replace(
            HELPER_SERVICE,
            "AmbientCapabilities=\n",
            "AmbientCapabilities=CAP_NET_ADMIN\n",
        ),
        format!("{HELPER_SERVICE}User=root\n"),
        format!("{HELPER_SERVICE}PrivateTmp=no\n"),
    ];
    for text in changed {
        let units = [UnitFile::parse(HELPER_UNIT, &text)];
        let found = check_units(&units, Some(&created()), true);
        assert!(rules_of(&found).contains(&"UNIT-3"), "{text}\n{found:?}");
    }
    assert!(rules_of(&check_units(&[], Some(&created()), true)).contains(&"UNIT-3"));
}

#[test]
fn shipped_units_pass_every_unit_rule() {
    let found = check_deploy(root(), REQUIRE_CREATED_SET).expect("the deploy tree reads");
    let unit_rules: Vec<_> = found
        .iter()
        .filter(|v| v.rule.starts_with("UNIT-"))
        .collect();
    assert!(unit_rules.is_empty(), "{unit_rules:#?}");
}

fn shipped(rule: &str) -> Vec<Violation> {
    check_deploy(root(), false)
        .expect("the deploy tree reads")
        .into_iter()
        .filter(|v| v.rule == rule)
        .collect()
}

#[test]
fn no_unit_execs_from_run() {
    assert_eq!(shipped("UNIT-9"), []);
}

#[test]
fn socket_units_are_0660_root_owned_group_domovoi_agent() {
    assert_eq!(shipped("UNIT-5"), []);
    let units = load(root()).expect("the deploy tree reads").units;
    let sockets: Vec<_> = units
        .iter()
        .filter(|u| u.name.ends_with(".socket"))
        .collect();
    assert!(!sockets.is_empty(), "the helper socket ships");
}

#[test]
fn agent_unit_runs_as_domovoi_agent() {
    assert_eq!(shipped("UNIT-8"), []);
}

#[test]
fn no_unit_or_compose_file_binds_docker_socket_read_write_into_the_agent() {
    assert_eq!(shipped("UNIT-8"), []);
    assert_eq!(shipped("UNIT-2"), []);
}
