//! Loading and validating `helper.toml`.

use domovoi_config::{ConfigIssue, FRESH_INSTALL_GROUPS, HelperConfig, load_helper, parse_helper};
use domovoi_core::helper::HelperGroup;

fn errors(text: &str) -> Vec<ConfigIssue> {
    parse_helper(text).err().unwrap_or_default()
}

const OWNER_EXAMPLE: &str = r#"
groups = ["docker.read", "docker.write", "systemd.read", "systemd.write", "nft", "smart.read", "kernel.read", "net.read", "safety.attest"]

[docker]
write_containers = ["plex"]

[systemd]
restart_units = ["plex.service"]

[nft]
table = "domovoi"
max_elements = 1000

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]

[safety]
management_cidrs = ["192.168.1.0/24", "100.64.0.0/10"]
protected_units = ["sshd.service", "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket", "systemd-networkd.service", "nftables.service"]
protected_containers = []

[safety.containment]
max_ttl = "24h"
max_active = 50

[limits]
timeout = "30s"
mutations_per_hour = 30
"#;

const FRESH_INSTALL_FILE: &str = r#"
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]

[platform]
allow_write_groups_on_untested = false

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]

[safety]
management_cidrs = []
protected_units = ["sshd.service", "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket"]
protected_containers = []

[safety.containment]
max_ttl = "1h"
max_active = 0

[limits]
timeout = "30s"
mutations_per_hour = 0
"#;

#[test]
fn helper_config_default_is_read_groups_only() {
    let from_empty = parse_helper("").expect("an empty file loads").0;
    assert_eq!(from_empty.effective_groups(), FRESH_INSTALL_GROUPS);
    assert!(from_empty.ignored_groups().is_empty());
    assert!(
        from_empty.effective_groups().iter().all(|g| !g.is_write()),
        "no write group is on by default"
    );
    let empty_list = parse_helper("groups = []").expect("loads").0;
    assert_eq!(empty_list.effective_groups(), FRESH_INSTALL_GROUPS);
    assert_eq!(
        HelperConfig::default().effective_groups(),
        FRESH_INSTALL_GROUPS
    );
    // What the installer writes is the default.
    let installed = parse_helper(FRESH_INSTALL_FILE).expect("loads").0;
    assert_eq!(installed, HelperConfig::default());
    assert_eq!(
        from_empty.smart.devices,
        ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]
    );
    assert_eq!(from_empty.safety.containment.max_active, 0);
    assert_eq!(from_empty.limits.mutations_per_hour, 0);
}

#[test]
fn helper_config_rejects_unknown_key_group_and_duplicates() {
    let unknown_key = errors("grops = []");
    assert!(
        unknown_key.iter().any(|i| i.path == "grops"),
        "{unknown_key:?}"
    );
    let section_key = errors("[platform]\nallow_writes = true");
    assert!(!section_key.is_empty());
    let unknown_group = errors(r#"groups = ["docker.read", "docker.admin"]"#);
    assert!(
        unknown_group
            .iter()
            .any(|i| i.path.starts_with("groups") && i.message.contains("docker.admin")),
        "{unknown_group:?}"
    );
    let duplicate = errors(r#"groups = ["docker.read", "net.read", "docker.read"]"#);
    assert!(
        duplicate
            .iter()
            .any(|i| i.path == "groups[2]" && i.message.contains("more than once")),
        "{duplicate:?}"
    );
    assert!(!errors("[nft]\nmax_elements = 0").is_empty());
    assert!(!errors("[nft]\nmax_elements = 4294967296").is_empty());
    assert!(!errors("[safety.containment]\nmax_ttl = \"soon\"").is_empty());
    assert!(!errors("not toml =").is_empty());
}

#[test]
fn helper_config_ignores_write_groups_in_effective_set() {
    let config = parse_helper(OWNER_EXAMPLE).expect("loads").0;
    assert_eq!(
        config.effective_groups(),
        [
            HelperGroup::DockerRead,
            HelperGroup::SystemdRead,
            HelperGroup::SmartRead,
            HelperGroup::KernelRead,
            HelperGroup::NetRead,
            HelperGroup::SafetyAttest,
        ]
    );
    assert_eq!(
        config.ignored_groups(),
        [
            HelperGroup::DockerWrite,
            HelperGroup::SystemdWrite,
            HelperGroup::Nft
        ]
    );
    let only_writes = parse_helper(r#"groups = ["nft"]"#).expect("loads").0;
    assert!(only_writes.effective_groups().is_empty());
    assert_eq!(only_writes.ignored_groups(), [HelperGroup::Nft]);
}

#[test]
fn helper_config_platform_opt_in_defaults_off() {
    assert!(!parse_helper("").expect("loads").0.write_opt_in());
    assert!(!parse_helper("[platform]").expect("loads").0.write_opt_in());
    let on = parse_helper("[platform]\nallow_write_groups_on_untested = true")
        .expect("loads")
        .0;
    assert!(on.write_opt_in());
    assert!(!errors("[platform]\nallow_write_groups_on_untested = \"yes\"").is_empty());
}

#[test]
fn helper_config_accepts_the_documented_example() {
    let (config, warnings) = parse_helper(OWNER_EXAMPLE).expect("the owner example loads");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(config.docker.write_containers, ["plex"]);
    assert_eq!(config.systemd.restart_units, ["plex.service"]);
    assert_eq!(
        (config.nft.table.as_str(), config.nft.max_elements),
        ("domovoi", 1000)
    );
    assert_eq!(config.safety.management_cidrs.len(), 2);
    assert_eq!(config.safety.protected_units.len(), 6);
    assert_eq!(config.safety.containment.max_active, 50);
    assert_eq!(
        config.safety.containment.max_ttl.as_signed().as_secs(),
        24 * 3600
    );
    assert_eq!(config.limits.timeout.as_signed().as_secs(), 30);
    assert_eq!(config.limits.mutations_per_hour, 30);
}

#[test]
fn helper_config_loads_from_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("helper.toml");
    std::fs::write(&path, FRESH_INSTALL_FILE).expect("write");
    assert_eq!(
        load_helper(&path).expect("loads").0,
        HelperConfig::default()
    );
    let missing = load_helper(&dir.path().join("absent.toml")).expect_err("no file");
    assert!(missing[0].message.contains("cannot read"));
}
