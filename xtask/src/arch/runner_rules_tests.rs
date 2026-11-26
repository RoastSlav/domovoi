//! Tests for the runner unit rules and the installer's runner order: each rule gets a failing
//! input, and the shipped files and every rendering of them are the gate.

use std::collections::BTreeSet;
use std::path::Path;

use super::super::rules::IDENTITY_CARDS;
use super::super::units::{REQUIRE_CREATED_SET, check_deploy, check_units};
use super::*;

const RUNNER_SERVICE_TEXT: &str =
    include_str!("../../../deploy/systemd/domovoi-skill-runner@.service");
const RUNNER_SOCKET_TEXT: &str =
    include_str!("../../../deploy/systemd/domovoi-skill-runner.socket");
const HELPER_SERVICE_TEXT: &str = include_str!("../../../deploy/systemd/domovoi-helper@.service");
const HELPER_SOCKET_TEXT: &str = include_str!("../../../deploy/systemd/domovoi-helper.socket");
const AGENT_SERVICE_TEXT: &str = include_str!("../../../deploy/systemd/domovoi-agent.service");
const INSTALLER: &str = include_str!("../../../install.sh");

const DEFAULT_SKILLS: &str = "/etc/domovoi/skills";
const DEFAULT_PACKAGES: &str = "/var/lib/domovoi-packages";

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits in the workspace")
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

/// The created-set of the installer, with extra configured directories.
fn created(extra: &[&str]) -> BTreeSet<String> {
    let mut all = set(&[
        "/etc/domovoi",
        "/etc/domovoi/skills",
        "/var/lib/domovoi",
        "/var/lib/domovoi-agent",
        "/var/lib/domovoi-packages",
        "/var/lib/domovoi-helper",
    ]);
    all.extend(extra.iter().map(|p| p.to_string()));
    all
}

/// The set of shipped units with the runner pair replaced by the given texts.
fn units_with(service: &str, socket: &str) -> Vec<UnitFile> {
    vec![
        UnitFile::parse(RUNNER_SERVICE, service),
        UnitFile::parse(RUNNER_SOCKET, socket),
        UnitFile::parse("domovoi-helper@.service", HELPER_SERVICE_TEXT),
        UnitFile::parse("domovoi-helper.socket", HELPER_SOCKET_TEXT),
        UnitFile::parse(
            "domovoi-helper-selftest.service",
            include_str!("../../../deploy/systemd/domovoi-helper-selftest.service"),
        ),
        UnitFile::parse("domovoi-agent.service", AGENT_SERVICE_TEXT),
    ]
}

fn shipped_units() -> Vec<UnitFile> {
    units_with(RUNNER_SERVICE_TEXT, RUNNER_SOCKET_TEXT)
}

fn replace(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "fixture lacks `{from}`");
    text.replacen(from, to, 1)
}

fn rules_of(found: &[Violation]) -> Vec<&'static str> {
    found.iter().map(|v| v.rule).collect()
}

/// What `render_runner_service` in `install.sh` does: the two directories in place of the
/// defaults, through placeholders.
fn render(text: &str, skills: &str, packages: &str) -> String {
    text.replace(DEFAULT_SKILLS, "@SKILLS_DIR@")
        .replace(DEFAULT_PACKAGES, "@PACKAGES_DIR@")
        .replace("@SKILLS_DIR@", skills)
        .replace("@PACKAGES_DIR@", packages)
}

#[test]
fn unit_parser_flags_rw39_inaccessible_child_under_masked_parent() {
    // The planning text listed these children next to the parent masks.
    let rw39 = format!(
        "{RUNNER_SERVICE_TEXT}InaccessiblePaths=/var/lib/domovoi-helper /run/domovoi-helper.sock /etc/domovoi/helper.toml\n"
    );
    let found = check_runner_units(&units_with(&rw39, RUNNER_SOCKET_TEXT));
    let unit1: Vec<_> = found.iter().filter(|v| v.rule == "UNIT-1").collect();
    assert_eq!(unit1.len(), 3, "{found:?}");
    assert!(unit1[0].message.contains("/var/lib/domovoi-helper"));
    // A path outside every masked parent is not a child of one.
    let fine = format!("{RUNNER_SERVICE_TEXT}InaccessiblePaths=/proc/kcore\n");
    let found = check_runner_units(&units_with(&fine, RUNNER_SOCKET_TEXT));
    assert!(!rules_of(&found).contains(&"UNIT-1"), "{found:?}");
    // The parent itself listed again is also refused.
    let twice = format!("{RUNNER_SERVICE_TEXT}InaccessiblePaths=/run\n");
    let found = check_runner_units(&units_with(&twice, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-1"), "{found:?}");
}

#[test]
fn unit_parser_flags_nonempty_supplementary_groups() {
    let group = format!("{RUNNER_SERVICE_TEXT}SupplementaryGroups=docker\n");
    let found = check_runner_units(&units_with(&group, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-2"), "{found:?}");
    assert!(rules_of(&found).contains(&"UNIT-3"), "{found:?}");
    // An empty value does not drop the account's groups and suggests it does: UNIT-3 refuses
    // the line, UNIT-2 has nothing to say about it.
    let empty = format!("{RUNNER_SERVICE_TEXT}SupplementaryGroups=\n");
    let found = check_runner_units(&units_with(&empty, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-3"), "{found:?}");
    assert!(!rules_of(&found).contains(&"UNIT-2"), "{found:?}");
    let wrong = replace(
        RUNNER_SERVICE_TEXT,
        "Group=domovoi-skill",
        "Group=domovoi-agent",
    );
    let found = check_runner_units(&units_with(&wrong, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-2"), "{found:?}");
}

#[test]
fn unit_parser_flags_missing_protect_home() {
    let found = check_runner_units(&units_with(
        &replace(RUNNER_SERVICE_TEXT, "ProtectHome=yes\n", ""),
        RUNNER_SOCKET_TEXT,
    ));
    assert!(
        found
            .iter()
            .any(|v| v.rule == "UNIT-3" && v.message.contains("ProtectHome")),
        "{found:?}"
    );
    // Every directive of the baseline is required with its value.
    for (key, want) in RUNNER_UNIT_DIRECTIVES {
        let line = format!("{key}={want}\n");
        let removed = replace(RUNNER_SERVICE_TEXT, &line, "");
        let found = check_runner_units(&units_with(&removed, RUNNER_SOCKET_TEXT));
        assert!(
            found
                .iter()
                .any(|v| v.rule == "UNIT-3" && v.message.contains(key)),
            "removing {key}: {found:?}"
        );
    }
    let missing = check_runner_present(&[]);
    assert_eq!(rules_of(&missing), ["UNIT-3", "UNIT-5"]);
}

#[test]
fn unit_parser_flags_runner_binds_that_differ_from_exec_start() {
    let drift = replace(
        RUNNER_SERVICE_TEXT,
        "BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages",
        "BindReadOnlyPaths=/etc/domovoi/skills /srv/other",
    );
    let found = check_runner_units(&units_with(&drift, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-3"), "{found:?}");
    let shape = replace(
        RUNNER_SERVICE_TEXT,
        "skill-runner serve --skills-dir",
        "skill-runner serve --other-flag x --skills-dir",
    );
    let found = check_runner_units(&units_with(&shape, RUNNER_SOCKET_TEXT));
    assert!(rules_of(&found).contains(&"UNIT-3"), "{found:?}");
}

#[test]
fn unit_parser_flags_wide_runner_socket_mode() {
    let wide = replace(RUNNER_SOCKET_TEXT, "SocketMode=0660", "SocketMode=0666");
    let found = check_units(
        &units_with(RUNNER_SERVICE_TEXT, &wide),
        Some(&created(&[])),
        true,
    );
    assert!(rules_of(&found).contains(&"UNIT-5"), "{found:?}");
    // Listening on the helper's path, or on another one, is refused too.
    for path in ["/run/domovoi-helper.sock", "/run/other.sock"] {
        let moved = replace(RUNNER_SOCKET_TEXT, "/run/domovoi-skill-runner.sock", path);
        let found = check_units(
            &units_with(RUNNER_SERVICE_TEXT, &moved),
            Some(&created(&[])),
            true,
        );
        assert!(
            found
                .iter()
                .any(|v| v.rule == "UNIT-5" && v.message.contains("runner socket")),
            "{path}: {found:?}"
        );
    }
    let group = replace(
        RUNNER_SOCKET_TEXT,
        "SocketGroup=domovoi-agent",
        "SocketGroup=users",
    );
    let found = check_units(
        &units_with(RUNNER_SERVICE_TEXT, &group),
        Some(&created(&[])),
        true,
    );
    assert!(rules_of(&found).contains(&"UNIT-5"), "{found:?}");
    let gone: Vec<UnitFile> = units_with(RUNNER_SERVICE_TEXT, RUNNER_SOCKET_TEXT)
        .into_iter()
        .filter(|u| u.name != RUNNER_SOCKET)
        .collect();
    assert!(rules_of(&check_runner_present(&gone)).contains(&"UNIT-5"));
}

#[test]
fn unit_parser_flags_socket_bind_into_runner() {
    for bind in [
        "BindReadOnlyPaths=/run/domovoi-helper.sock\n",
        "BindReadOnlyPaths=/run/domovoi-skill-runner.sock:/x.sock\n",
        "BindPaths=/srv/thing.sock\n",
        "BindPaths=/run/other\n",
        "BindPaths=-/run\n",
    ] {
        let text = format!("{RUNNER_SERVICE_TEXT}{bind}");
        let found = check_runner_units(&units_with(&text, RUNNER_SOCKET_TEXT));
        assert!(rules_of(&found).contains(&"UNIT-6"), "{bind}: {found:?}");
    }
    // The two shipped binds are plain directories outside /run.
    assert!(!rules_of(&check_runner_units(&shipped_units())).contains(&"UNIT-6"));
}

#[test]
fn unit_parser_flags_dash_prefixed_mask_of_created_path() {
    let dashed_mask = replace(
        RUNNER_SERVICE_TEXT,
        "TemporaryFileSystem=/etc/domovoi:ro",
        "TemporaryFileSystem=-/etc/domovoi:ro",
    );
    let dashed_bind = replace(
        RUNNER_SERVICE_TEXT,
        "BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages",
        "BindReadOnlyPaths=-/etc/domovoi/skills -/var/lib/domovoi-packages",
    );
    for text in [dashed_mask, dashed_bind] {
        let found = check_units(
            &units_with(&text, RUNNER_SOCKET_TEXT),
            Some(&created(&[])),
            true,
        );
        assert!(
            found
                .iter()
                .any(|v| v.rule == "UNIT-7" && v.message.contains("no `-` prefix")),
            "{text}\n{found:?}"
        );
    }
}

#[test]
fn unit_parser_flags_path_not_in_created_set() {
    // A rendering for other directories passes only when the installer created them.
    let text = render(RUNNER_SERVICE_TEXT, "/srv/skills", "/srv/packages");
    let found = check_units(
        &units_with(&text, RUNNER_SOCKET_TEXT),
        Some(&created(&[])),
        true,
    );
    let unit7: Vec<_> = found.iter().filter(|v| v.rule == "UNIT-7").collect();
    assert_eq!(unit7.len(), 2, "both binds are flagged: {found:?}");
    assert!(unit7.iter().any(|v| v.message.contains("/srv/skills")));
    assert!(unit7.iter().any(|v| v.message.contains("/srv/packages")));
    let mask = replace(RUNNER_SERVICE_TEXT, "/run:ro", "/srv/mask:ro");
    let found = check_units(
        &units_with(&mask, RUNNER_SOCKET_TEXT),
        Some(&created(&[])),
        true,
    );
    assert!(
        found
            .iter()
            .any(|v| v.rule == "UNIT-7" && v.message.contains("TemporaryFileSystem")),
        "{found:?}"
    );
}

#[test]
fn unit_parser_flags_exec_start_under_run() {
    let text = replace(
        RUNNER_SERVICE_TEXT,
        "ExecStart=/usr/local/bin/domovoi",
        "ExecStart=/run/domovoi/domovoi",
    );
    let found = check_units(
        &units_with(&text, RUNNER_SOCKET_TEXT),
        Some(&created(&[])),
        true,
    );
    assert!(rules_of(&found).contains(&"UNIT-9"), "{found:?}");
}

#[test]
fn shipped_runner_units_pass_every_unit_rule() {
    let units = shipped_units();
    let found = check_units(&units, Some(&created(&[])), REQUIRE_CREATED_SET);
    assert!(found.is_empty(), "{found:#?}");
    // The shipped file holds the default directories.
    let exec = UnitFile::parse(RUNNER_SERVICE, RUNNER_SERVICE_TEXT).values("Service", "ExecStart")
        [0]
    .to_string();
    assert_eq!(
        exec_dirs(&exec),
        Some((DEFAULT_SKILLS.to_string(), DEFAULT_PACKAGES.to_string()))
    );
    // And the real tree, with the installer, passes the unit and installer rules.
    let found = check_deploy(root(), REQUIRE_CREATED_SET).expect("the deploy tree reads");
    let hits: Vec<_> = found
        .iter()
        .filter(|v| v.rule.starts_with("UNIT-") || v.rule == "INSTALL-1")
        .collect();
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn rendered_runner_units_for_custom_dirs_pass_every_unit_rule() {
    let pairs = [
        ("/srv/skills", "/srv/packages"),
        ("/etc/domovoi/skills/team", "/var/lib/domovoi-packages-b"),
        ("/opt/domovoi/s", "/var/lib/domovoi-packages"),
        ("/etc/domovoi/skills", "/data/pkgs/domovoi"),
    ];
    for (skills, packages) in pairs {
        let text = render(RUNNER_SERVICE_TEXT, skills, packages);
        assert!(text.contains(&format!("--skills-dir {skills} --packages-dir {packages}")));
        assert!(text.contains(&format!("BindReadOnlyPaths={skills} {packages}")));
        let found = check_units(
            &units_with(&text, RUNNER_SOCKET_TEXT),
            Some(&created(&[skills, packages])),
            true,
        );
        assert!(found.is_empty(), "{skills} {packages}: {found:#?}");
    }
    // A rendering that rewrote only the arguments is caught.
    let half = replace(
        RUNNER_SERVICE_TEXT,
        "--skills-dir /etc/domovoi/skills",
        "--skills-dir /srv/skills",
    );
    let found = check_units(
        &units_with(&half, RUNNER_SOCKET_TEXT),
        Some(&created(&["/srv/skills"])),
        true,
    );
    assert!(rules_of(&found).contains(&"UNIT-3"), "{found:?}");
}

#[test]
fn runner_rules_have_negative_tests() {
    use super::super::runner_scan::check_file as scan;
    let unit_inputs: [(&str, Vec<Violation>); 4] = [
        (
            "UNIT-1",
            check_runner_units(&units_with(
                &format!("{RUNNER_SERVICE_TEXT}InaccessiblePaths=/run/x\n"),
                RUNNER_SOCKET_TEXT,
            )),
        ),
        (
            "UNIT-6",
            check_runner_units(&units_with(
                &format!("{RUNNER_SERVICE_TEXT}BindPaths=/a.sock\n"),
                RUNNER_SOCKET_TEXT,
            )),
        ),
        (
            "RUNNER-1",
            scan(
                "crates/domovoi-hub/src/a.rs",
                "const S: &str = \"/run/domovoi-skill-runner.sock\";\n",
                false,
            ),
        ),
        (
            "RUNNER-2",
            scan(
                "crates/domovoi-runner/src/serve/a.rs",
                "use domovoi_helper::probe::collect;\n",
                false,
            ),
        ),
    ];
    for (rule, found) in unit_inputs {
        assert!(found.iter().any(|v| v.rule == rule), "{rule}: {found:?}");
        let card = IDENTITY_CARDS
            .iter()
            .find(|c| c.id == rule)
            .unwrap_or_else(|| panic!("{rule} has no card"));
        assert!(found[0].message.contains(card.message));
    }
    for id in ["UNIT-2", "UNIT-3", "UNIT-5", "INSTALL-1"] {
        assert!(IDENTITY_CARDS.iter().any(|c| c.id == id), "{id}");
    }
}

#[test]
fn installer_creates_runner_paths_before_enabling_the_socket() {
    let found = check_install_order(INSTALLER);
    assert!(found.is_empty(), "{found:#?}");
    let unit_mutations = [
        replace(INSTALLER, "    runner_dirs\n", "    # runner_dirs\n"),
        replace(
            INSTALLER,
            "    make_dir \"$runner_skills_dir\" 0755 root:root\n",
            "",
        ),
        replace(
            INSTALLER,
            "    make_dir \"$runner_packages_dir\" 0755 domovoi-agent:domovoi-agent\n",
            "",
        ),
        replace(
            INSTALLER,
            "    place_unit domovoi-skill-runner.socket runner_socket_unit\n",
            "",
        ),
        replace(
            INSTALLER,
            "    place_unit domovoi-skill-runner@.service render_runner_service\n",
            "",
        ),
        INSTALLER.replace("agent selftest --installer", "agent selftest --other"),
        // The socket enabled at the top of the script, before anything exists.
        replace(
            INSTALLER,
            "set -eu\n",
            "set -eu\nsystemctl enable --now domovoi-skill-runner.socket\n",
        ),
    ];
    for (n, text) in unit_mutations.iter().enumerate() {
        let found = check_install_order(text);
        assert!(
            found.iter().all(|v| v.rule == "INSTALL-1") && !found.is_empty(),
            "mutation {n}: {found:?}"
        );
    }
    // No runner enable at all is a violation too.
    let found = check_install_order("#!/bin/sh\nsystemctl enable --now domovoi-helper.socket\n");
    assert_eq!(rules_of(&found), ["INSTALL-1"]);
}

/// The text between `NAME() {`, its `cat <<'EOF'` line and the closing `EOF`.
fn embedded(script: &str, name: &str) -> String {
    let head = format!("{name}() {{\n    cat <<'EOF'\n");
    let start = script
        .find(&head)
        .unwrap_or_else(|| panic!("no function {name}"))
        + head.len();
    let body = &script[start..];
    let end = body.find("\nEOF\n").expect("the heredoc ends");
    format!("{}\n", &body[..end])
}

#[test]
fn installer_embedded_runner_units_match_deploy_systemd() {
    assert_eq!(
        embedded(INSTALLER, "runner_socket_unit"),
        RUNNER_SOCKET_TEXT
    );
    assert_eq!(
        embedded(INSTALLER, "runner_service_unit"),
        RUNNER_SERVICE_TEXT
    );
}

#[test]
fn installer_renders_custom_skills_dir_into_exec_start_and_bind() {
    // The sed of `render_runner_service` rewrites both defaults everywhere they occur, and the
    // installer places the rendered text, not the raw function.
    for expr in [
        "-e 's|/etc/domovoi/skills|@SKILLS_DIR@|g'",
        "-e 's|/var/lib/domovoi-packages|@PACKAGES_DIR@|g'",
        "-e \"s|@SKILLS_DIR@|$runner_skills_dir|g\"",
        "-e \"s|@PACKAGES_DIR@|$runner_packages_dir|g\"",
    ] {
        assert!(INSTALLER.contains(expr), "install.sh lacks {expr}");
    }
    assert!(INSTALLER.contains("place_unit domovoi-skill-runner@.service render_runner_service"));
    assert!(INSTALLER.contains("\"$bin\" agent dirs"));
}
