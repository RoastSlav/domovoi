//! `helper.toml`: configuration for the root helper.
//!
//! Read only by the helper, never overridden from the environment (the helper runs from a
//! socket-activated unit with no configuration of its own) and never carrying a secret.

use std::path::Path;

use domovoi_core::helper::HelperGroup;
use serde::{Deserialize, Deserializer};

use crate::duration::Duration;
use crate::env::MapEnv;
use crate::issue::ConfigIssue;
use crate::secret::{Secret, Secrets};
use crate::validate::Validate;
use crate::{LoadResult, finish};

/// The groups of a fresh install: reads only, no write group.
pub const FRESH_INSTALL_GROUPS: [HelperGroup; 6] = [
    HelperGroup::DockerRead,
    HelperGroup::SystemdRead,
    HelperGroup::SmartRead,
    HelperGroup::KernelRead,
    HelperGroup::NetRead,
    HelperGroup::SafetyAttest,
];

/// The whole of `helper.toml`. Every section is optional.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperConfig {
    /// Operation groups to enable. Empty or absent means the fresh-install set. Write groups
    /// are accepted and ignored until their milestones ship.
    #[serde(default, deserialize_with = "parse_groups")]
    pub groups: Vec<HelperGroup>,
    /// Per-host platform decisions.
    #[serde(default)]
    pub platform: PlatformSection,
    /// Which disks `smart.read` may query.
    #[serde(default)]
    pub smart: SmartSection,
    /// Exclusions and containment limits; parsed here, interpreted by later milestones.
    #[serde(default)]
    pub safety: SafetySection,
    /// Rate and time limits.
    #[serde(default)]
    pub limits: LimitsSection,
    /// `[docker]`: the read endpoint; `write_containers` is accepted, not interpreted yet.
    #[serde(default)]
    pub docker: DockerSection,
    /// `[systemd]`: accepted, not interpreted yet.
    #[serde(default)]
    pub systemd: SystemdSection,
    /// `[nft]`: accepted, not interpreted yet.
    #[serde(default)]
    pub nft: NftSection,
}

/// `[platform]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSection {
    /// The owner's per-host opt-in to helper write groups on a platform that has not been
    /// release-tested. Default `false`.
    #[serde(default)]
    pub allow_write_groups_on_untested: bool,
}

/// `[smart]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmartSection {
    /// Device path patterns `smart.read` may query.
    #[serde(default = "default_devices")]
    pub devices: Vec<String>,
}

impl Default for SmartSection {
    fn default() -> Self {
        Self {
            devices: default_devices(),
        }
    }
}

/// `[safety]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetySection {
    /// Networks that containment must never cut off. Empty refuses containment.
    #[serde(default)]
    pub management_cidrs: Vec<String>,
    /// Units no write may target.
    #[serde(default = "default_protected_units")]
    pub protected_units: Vec<String>,
    /// Containers no write may target.
    #[serde(default)]
    pub protected_containers: Vec<String>,
    /// Host hard maxima for containment.
    #[serde(default)]
    pub containment: ContainmentSection,
}

impl Default for SafetySection {
    fn default() -> Self {
        Self {
            management_cidrs: Vec::new(),
            protected_units: default_protected_units(),
            protected_containers: Vec::new(),
            containment: ContainmentSection::default(),
        }
    }
}

/// `[safety.containment]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainmentSection {
    /// The longest a containment may last. Default `"1h"`.
    #[serde(default = "default_max_ttl")]
    pub max_ttl: Duration,
    /// How many containments may be active; `0` allows none. Default `0`.
    #[serde(default)]
    pub max_active: u32,
}

impl Default for ContainmentSection {
    fn default() -> Self {
        Self {
            max_ttl: default_max_ttl(),
            max_active: 0,
        }
    }
}

/// `[limits]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsSection {
    /// The longest one helper operation may run. Default `"30s"`.
    #[serde(default = "default_timeout")]
    pub timeout: Duration,
    /// Mutating operations per hour; `0` allows none. Default `0`.
    #[serde(default)]
    pub mutations_per_hour: u32,
}

impl Default for LimitsSection {
    fn default() -> Self {
        Self {
            timeout: default_timeout(),
            mutations_per_hour: 0,
        }
    }
}

/// `[docker]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerSection {
    /// The engine socket `docker.read` opens, as `unix:///absolute/path`. Absent: the helper tries
    /// the two system sockets.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Containers `docker.write` may restart, by name.
    #[serde(default)]
    pub write_containers: Vec<String>,
}

/// `[systemd]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemdSection {
    /// Units `systemd.write` may restart.
    #[serde(default)]
    pub restart_units: Vec<String>,
}

/// `[nft]`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NftSection {
    /// The nftables table the helper owns. Default `"domovoi"`.
    #[serde(default = "default_nft_table")]
    pub table: String,
    /// The most elements the set may hold, 1 to 10 000. Default 1 000.
    #[serde(default = "default_max_elements")]
    pub max_elements: u32,
}

impl Default for NftSection {
    fn default() -> Self {
        Self {
            table: default_nft_table(),
            max_elements: default_max_elements(),
        }
    }
}

impl Default for HelperConfig {
    /// A fresh install: the read groups and every default.
    fn default() -> Self {
        Self {
            groups: FRESH_INSTALL_GROUPS.to_vec(),
            platform: PlatformSection::default(),
            smart: SmartSection::default(),
            safety: SafetySection::default(),
            limits: LimitsSection::default(),
            docker: DockerSection::default(),
            systemd: SystemdSection::default(),
            nft: NftSection::default(),
        }
    }
}

impl HelperConfig {
    fn configured(&self) -> Vec<HelperGroup> {
        if self.groups.is_empty() {
            FRESH_INSTALL_GROUPS.to_vec()
        } else {
            self.groups.clone()
        }
    }

    /// The groups the helper serves: the configured groups (the fresh-install set when none are
    /// named) minus every write group, in file order.
    #[must_use]
    pub fn effective_groups(&self) -> Vec<HelperGroup> {
        self.configured()
            .into_iter()
            .filter(|g| !g.is_write())
            .collect()
    }

    /// The configured write groups, which are ignored and reported as such.
    #[must_use]
    pub fn ignored_groups(&self) -> Vec<HelperGroup> {
        self.configured()
            .into_iter()
            .filter(|g| g.is_write())
            .collect()
    }

    /// The socket path of `[docker] endpoint`, `None` when the key is absent. A loaded config
    /// has validated it, so a value that does not parse also gives `None`.
    #[must_use]
    pub fn docker_socket(&self) -> Option<std::path::PathBuf> {
        self.docker
            .endpoint
            .as_deref()
            .and_then(docker_socket_path)
            .map(std::path::PathBuf::from)
    }

    /// The `[smart] devices` patterns, each in the grammar of [`valid_device_pattern`].
    #[must_use]
    pub fn smart_device_patterns(&self) -> &[String] {
        &self.smart.devices
    }

    /// Every group named in `helper.toml` (the fresh-install set when none), write groups
    /// included. A write operation is served only when its group is here **and** the platform
    /// tier allows it.
    #[must_use]
    pub fn configured_groups(&self) -> Vec<HelperGroup> {
        self.configured()
    }

    /// The units no write may target: the file's `protected_units` unioned with the installer's
    /// defaults, which an edit of the file cannot remove.
    #[must_use]
    pub fn protected_unit_set(&self) -> std::collections::BTreeSet<String> {
        self.safety
            .protected_units
            .iter()
            .cloned()
            .chain(default_protected_units())
            .collect()
    }

    /// Whether the owner switched write groups on for untested platforms.
    #[must_use]
    pub fn write_opt_in(&self) -> bool {
        self.platform.allow_write_groups_on_untested
    }
}

fn parse_groups<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<HelperGroup>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    names
        .iter()
        .map(|name| {
            HelperGroup::parse(name).ok_or_else(|| {
                let known: Vec<&str> = HelperGroup::ALL.iter().map(|g| g.as_str()).collect();
                serde::de::Error::custom(format!(
                    "unknown helper group `{name}`; known groups are {}",
                    known.join(", ")
                ))
            })
        })
        .collect()
}

fn default_devices() -> Vec<String> {
    vec!["/dev/sd[a-z]".to_owned(), "/dev/nvme[0-9]n1".to_owned()]
}

fn default_protected_units() -> Vec<String> {
    [
        "sshd.service",
        "domovoi-agent.service",
        "domovoi-helper.socket",
        "domovoi-skill-runner.socket",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

fn default_max_ttl() -> Duration {
    Duration::secs(3600)
}

fn default_timeout() -> Duration {
    Duration::secs(30)
}

fn default_nft_table() -> String {
    "domovoi".to_owned()
}

fn default_max_elements() -> u32 {
    1000
}

/// The socket path inside `unix:///path`, or `None` when the text is not that form: an empty path,
/// a relative path or a `..` segment is refused, as is any other scheme.
fn docker_socket_path(endpoint: &str) -> Option<&str> {
    let path = endpoint.strip_prefix("unix://")?;
    let ok = path.len() > 1
        && path.starts_with('/')
        && !path.contains('\0')
        && !path.split('/').any(|segment| segment == "..");
    ok.then_some(path)
}

/// Whether `pattern` is in the device pattern grammar of `[smart] devices`: literal `a-z`, `0-9`
/// and `/` characters, and the single-character classes `[a-z]` and `[0-9]`. Nothing else is
/// accepted (no `*`, `?`, `{`, other classes or `.`), and the pattern must not be empty.
#[must_use]
pub fn valid_device_pattern(pattern: &str) -> bool {
    let mut rest = pattern;
    if rest.is_empty() {
        return false;
    }
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest
            .strip_prefix("[a-z]")
            .or_else(|| rest.strip_prefix("[0-9]"))
        {
            rest = after;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' {
            rest = &rest[1..];
        } else {
            return false;
        }
    }
    true
}

impl Validate for HelperConfig {
    fn validate(&self, issues: &mut Vec<ConfigIssue>) {
        for (index, group) in self.groups.iter().enumerate() {
            if self.groups[..index].contains(group) {
                issues.push(ConfigIssue::error(
                    format!("groups[{index}]"),
                    format!("`{group}` is listed more than once; list each group once"),
                ));
            }
        }
        if let Some(endpoint) = &self.docker.endpoint
            && docker_socket_path(endpoint).is_none()
        {
            issues.push(ConfigIssue::error(
                "docker.endpoint",
                "must be `unix://` followed by an absolute socket path without `..`, \
                 such as `unix:///var/run/docker.sock`",
            ));
        }
        for (index, pattern) in self.smart.devices.iter().enumerate() {
            if !valid_device_pattern(pattern) {
                issues.push(ConfigIssue::error(
                    format!("smart.devices[{index}]"),
                    "may hold only the characters a-z, 0-9 and `/`, and the classes `[a-z]` and \
                     `[0-9]`",
                ));
            }
        }
        if !(1..=10_000).contains(&self.nft.max_elements) {
            issues.push(ConfigIssue::error(
                "nft.max_elements",
                "must be between 1 and 10000",
            ));
        }
        if self.nft.table.is_empty() {
            issues.push(ConfigIssue::error("nft.table", "must not be empty"));
        }
    }
}

impl Secrets for HelperConfig {
    fn secrets_mut(&mut self, _visit: &mut dyn FnMut(&str, &mut Secret)) {}
}

/// Parses and validates `helper.toml` text. The environment is not consulted.
pub fn parse_helper(text: &str) -> LoadResult<HelperConfig> {
    let table: toml::Table = text.parse().map_err(|err: toml::de::Error| {
        vec![ConfigIssue::error("", format!("invalid TOML: {err}"))]
    })?;
    finish(table, &MapEnv::new(), None, Vec::new())
}

/// Loads and validates `helper.toml` from `path`.
pub fn load_helper(path: &Path) -> LoadResult<HelperConfig> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        vec![ConfigIssue::error(
            "",
            format!("cannot read {}: {err}", path.display()),
        )]
    })?;
    parse_helper(&text)
}
