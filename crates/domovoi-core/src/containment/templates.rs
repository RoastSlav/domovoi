//! The nft scripts: the only text ever sent to `nft`.
//!
//! Rendering takes validated values only: a table name, set names from a closed enum, a
//! canonical network, whole seconds of at least one, and a bounded element count. Nothing else
//! is interpolated. A TTL is written only as `<n>s` with `n >= 1`, so `timeout 0s` and an
//! element without a timeout cannot be rendered.

use super::net::{Family, Net, addr_text};

/// The chain priority: before conntrack and every manager's number (-300, -200, -140, -100,
/// -90, 0, 10, 100, 110).
pub const CHAIN_PRIORITY: i32 = -310;
/// The kernel timeout of step one, in seconds.
pub const STEP1_TTL_S: u32 = 30;
/// The longest `max_elements`.
pub const MAX_ELEMENTS_LIMIT: u32 = 10_000;
/// The complete environment of every spawned `nft` and `conntrack`.
pub const NFT_ENV: [(&str, &str); 1] = [("LC_ALL", "C")];

/// A value a template refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// The table name does not match `[a-z][a-z0-9_]{0,31}`.
    #[error("invalid table name")]
    TableName,
    /// `max_elements` is outside 1 to 10 000.
    #[error("max_elements must be 1 to 10000")]
    MaxElements,
    /// A TTL of zero seconds.
    #[error("a timeout must be at least one second")]
    ZeroSeconds,
    /// A confirm TTL not above the 30 s of step one.
    #[error("the confirm TTL must be above 30 seconds")]
    TtlTooShort,
}

/// A validated nft table name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableName(String);

impl TableName {
    /// The default table name.
    pub const DEFAULT: &'static str = "domovoi";

    /// Validates `name` against `[a-z][a-z0-9_]{0,31}`.
    ///
    /// # Errors
    /// [`TemplateError::TableName`].
    pub fn parse(name: &str) -> Result<Self, TemplateError> {
        let bytes = name.as_bytes();
        let ok = (1..=32).contains(&bytes.len())
            && bytes[0].is_ascii_lowercase()
            && bytes
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
        if ok {
            Ok(Self(name.to_owned()))
        } else {
            Err(TemplateError::TableName)
        }
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TableName {
    fn default() -> Self {
        Self(Self::DEFAULT.to_owned())
    }
}

/// A validated element capacity, 1 to 10 000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxElements(u32);

impl MaxElements {
    /// Validates `n`.
    ///
    /// # Errors
    /// [`TemplateError::MaxElements`].
    pub const fn new(n: u32) -> Result<Self, TemplateError> {
        if n >= 1 && n <= MAX_ELEMENTS_LIMIT {
            Ok(Self(n))
        } else {
            Err(TemplateError::MaxElements)
        }
    }

    /// The count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Whole seconds, at least one; the only way a timeout is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seconds(u32);

impl Seconds {
    /// Validates `n >= 1`.
    ///
    /// # Errors
    /// [`TemplateError::ZeroSeconds`].
    pub const fn new(n: u32) -> Result<Self, TemplateError> {
        if n >= 1 {
            Ok(Self(n))
        } else {
            Err(TemplateError::ZeroSeconds)
        }
    }

    /// The count.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    fn text(self) -> String {
        format!("{}s", self.0)
    }
}

/// Which sets the table holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    /// Plain hash sets for single hosts only.
    Hosts,
    /// Hash sets plus interval sets for prefixes.
    HostsAndPrefixes,
}

impl Layout {
    /// The wire word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hosts => "hosts",
            Self::HostsAndPrefixes => "hosts_and_prefixes",
        }
    }
}

/// The closed set of set names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SetName {
    /// Single IPv4 hosts.
    Blocked4,
    /// Single IPv6 hosts.
    Blocked6,
    /// IPv4 prefixes.
    BlockedNet4,
    /// IPv6 prefixes.
    BlockedNet6,
    /// IPv4 exclusions.
    Excl4,
    /// IPv6 exclusions.
    Excl6,
}

impl SetName {
    /// The name in the table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked4 => "blocked4",
            Self::Blocked6 => "blocked6",
            Self::BlockedNet4 => "blockednet4",
            Self::BlockedNet6 => "blockednet6",
            Self::Excl4 => "excl4",
            Self::Excl6 => "excl6",
        }
    }

    /// The set that holds a block of `net`: a hash set for a single host, an interval set
    /// for a prefix.
    #[must_use]
    pub fn for_net(net: &Net) -> Self {
        match (net.family(), net.is_single_host()) {
            (Family::V4, true) => Self::Blocked4,
            (Family::V6, true) => Self::Blocked6,
            (Family::V4, false) => Self::BlockedNet4,
            (Family::V6, false) => Self::BlockedNet6,
        }
    }

    /// The exclusion set of a family.
    #[must_use]
    pub const fn exclusion(family: Family) -> Self {
        match family {
            Family::V4 => Self::Excl4,
            Family::V6 => Self::Excl6,
        }
    }
}

/// The bootstrap batch, sent once when the table is absent.
#[must_use]
pub fn bootstrap(table: &TableName, layout: Layout, max: MaxElements) -> String {
    let t = table.as_str();
    let n = max.get();
    let step1 = format!("{STEP1_TTL_S}s");
    let mut s = format!("create table inet {t}\n");
    s.push_str(&format!(
        "add set inet {t} blocked4 {{ type ipv4_addr; flags timeout; timeout {step1}; size {n}; }}\n"
    ));
    s.push_str(&format!(
        "add set inet {t} blocked6 {{ type ipv6_addr; flags timeout; timeout {step1}; size {n}; }}\n"
    ));
    s.push_str(&format!(
        "add set inet {t} excl4 {{ type ipv4_addr; flags interval; }}\n"
    ));
    s.push_str(&format!(
        "add set inet {t} excl6 {{ type ipv6_addr; flags interval; }}\n"
    ));
    if layout == Layout::HostsAndPrefixes {
        s.push_str(&format!(
            "add set inet {t} blockednet4 {{ type ipv4_addr; flags interval,timeout; timeout {step1}; size {n}; }}\n"
        ));
        s.push_str(&format!(
            "add set inet {t} blockednet6 {{ type ipv6_addr; flags interval,timeout; timeout {step1}; size {n}; }}\n"
        ));
    }
    s.push_str(&format!(
        "add chain inet {t} contain {{ type filter hook prerouting priority {CHAIN_PRIORITY}; policy accept; }}\n"
    ));
    s.push_str(&format!("add rule inet {t} contain counter\n"));
    s.push_str(&format!(
        "add rule inet {t} contain ip saddr @excl4 accept\n"
    ));
    s.push_str(&format!(
        "add rule inet {t} contain ip6 saddr @excl6 accept\n"
    ));
    s.push_str(&format!(
        "add rule inet {t} contain ip saddr @blocked4 counter drop\n"
    ));
    s.push_str(&format!(
        "add rule inet {t} contain ip6 saddr @blocked6 counter drop\n"
    ));
    if layout == Layout::HostsAndPrefixes {
        s.push_str(&format!(
            "add rule inet {t} contain ip saddr @blockednet4 counter drop\n"
        ));
        s.push_str(&format!(
            "add rule inet {t} contain ip6 saddr @blockednet6 counter drop\n"
        ));
    }
    s
}

/// Step one: `create` the element with the 30 s kernel timeout. `create` fails with
/// `File exists` on a present element and never touches its timeout.
#[must_use]
pub fn apply(table: &TableName, net: &Net) -> String {
    format!(
        "create element inet {} {} {{ {} timeout {STEP1_TTL_S}s }}\n",
        table.as_str(),
        SetName::for_net(net).as_str(),
        net.element_text(),
    )
}

/// Step two: one batch, delete then add with the full TTL.
///
/// # Errors
/// [`TemplateError::TtlTooShort`] when `ttl` is not above 30 s.
pub fn confirm(table: &TableName, net: &Net, ttl: Seconds) -> Result<String, TemplateError> {
    if ttl.get() <= STEP1_TTL_S {
        return Err(TemplateError::TtlTooShort);
    }
    let (t, set, x) = (
        table.as_str(),
        SetName::for_net(net).as_str(),
        net.element_text(),
    );
    Ok(format!(
        "delete element inet {t} {set} {{ {x} }}\nadd element inet {t} {set} {{ {x} timeout {} }}\n",
        ttl.text()
    ))
}

/// The rollback batch, idempotent for present, absent and expired elements.
#[must_use]
pub fn rollback(table: &TableName, net: &Net) -> String {
    let (t, set, x) = (
        table.as_str(),
        SetName::for_net(net).as_str(),
        net.element_text(),
    );
    let five = Seconds(5).text();
    format!(
        "add element inet {t} {set} {{ {x} timeout {five} }}\ndelete element inet {t} {set} {{ {x} }}\n"
    )
}

/// The rollback fallback, sent only after a membership read says the element is present.
#[must_use]
pub fn rollback_fallback(table: &TableName, net: &Net) -> String {
    format!(
        "delete element inet {} {} {{ {} }}\n",
        table.as_str(),
        SetName::for_net(net).as_str(),
        net.element_text(),
    )
}

/// One family's exclusion rewrite: flush, then add the coalesced ranges. The `add element`
/// line is left out when `ranges` is empty.
#[must_use]
pub fn exclusion_rewrite(table: &TableName, family: Family, ranges: &[(u128, u128)]) -> String {
    let t = table.as_str();
    let set = SetName::exclusion(family).as_str();
    let mut s = format!("flush set inet {t} {set}\n");
    if !ranges.is_empty() {
        let list: Vec<String> = ranges
            .iter()
            .map(|(lo, hi)| format!("{}-{}", addr_text(family, *lo), addr_text(family, *hi)))
            .collect();
        s.push_str(&format!(
            "add element inet {t} {set} {{ {} }}\n",
            list.join(", ")
        ));
    }
    s
}

/// The argv of `nft -j list table inet <table>`.
#[must_use]
pub fn list_table_args(table: &TableName) -> Vec<String> {
    ["-j", "list", "table", "inet", table.as_str()]
        .map(str::to_owned)
        .to_vec()
}

/// The argv of `nft -j list set inet <table> <set>`.
#[must_use]
pub fn list_set_args(table: &TableName, set: SetName) -> Vec<String> {
    ["-j", "list", "set", "inet", table.as_str(), set.as_str()]
        .map(str::to_owned)
        .to_vec()
}

/// The argv of `nft -j list flowtables`.
#[must_use]
pub fn list_flowtables_args() -> Vec<String> {
    ["-j", "list", "flowtables"].map(str::to_owned).to_vec()
}

/// The argv of `nft -f -`; the script goes on stdin.
#[must_use]
pub fn script_args() -> Vec<String> {
    ["-f", "-"].map(str::to_owned).to_vec()
}

/// The argv of `conntrack -D -s <source>`.
#[must_use]
pub fn conntrack_delete_args(net: &Net) -> Vec<String> {
    vec!["-D".to_owned(), "-s".to_owned(), net.element_text()]
}

/// The one table deletion: our own table, when it is empty and its layout must change.
#[must_use]
pub fn delete_own_table(table: &TableName) -> String {
    format!("delete table inet {}\n", table.as_str())
}
