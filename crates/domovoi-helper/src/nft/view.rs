//! The table as `nft -j list table` reports it, reduced to what the helper compares.
//!
//! Everything here is read from JSON by number and by name, never from listing text: `nft` 1.0.9
//! prints priority -310 as `raw - 10`, but the JSON carries the number.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use domovoi_core::containment::{Family, Net};
use serde_json::Value;

/// Why JSON is not a table listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the nft listing is not the expected JSON")]
pub struct ViewError;

/// One element of a set: a range of addresses and the time it has left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    /// The address family.
    pub family: Family,
    /// First address as a number.
    pub lo: u128,
    /// Last address as a number.
    pub hi: u128,
    /// The timeout the element was given, in seconds.
    pub timeout: Option<i64>,
    /// The seconds left before the kernel removes it.
    pub expires: Option<i64>,
}

impl Member {
    /// Whether the element is exactly `net`.
    #[must_use]
    pub fn is(&self, net: &Net) -> bool {
        self.family == net.family() && self.lo == net.lo() && self.hi == net.hi()
    }

    /// Whether the element holds all of `net` without being it.
    #[must_use]
    pub fn covers(&self, net: &Net) -> bool {
        self.family == net.family() && self.lo <= net.lo() && net.hi() <= self.hi && !self.is(net)
    }

    /// The element as `lo-hi` text.
    #[must_use]
    pub fn text(&self) -> String {
        format!(
            "{}-{}",
            domovoi_core::containment::addr_text(self.family, self.lo),
            domovoi_core::containment::addr_text(self.family, self.hi)
        )
    }
}

/// A set as listed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetView {
    /// The element type, such as `ipv4_addr`.
    pub typ: String,
    /// The flags.
    pub flags: BTreeSet<String>,
    /// The `size`, when listed.
    pub size: Option<u64>,
    /// The default timeout in seconds, when listed.
    pub timeout: Option<u64>,
    /// The members.
    pub members: Vec<Member>,
}

/// The chain as listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainView {
    /// `filter`.
    pub typ: String,
    /// The hook name.
    pub hook: String,
    /// The priority as a number.
    pub prio: i64,
    /// The default policy.
    pub policy: String,
}

/// The table: its sets, its chain and the rules of the chain in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TableView {
    /// Sets by name.
    pub sets: BTreeMap<String, SetView>,
    /// The `contain` chain, when present.
    pub chain: Option<ChainView>,
    /// A signature of each rule in order, see [`rule_signature`].
    pub rules: Vec<String>,
    /// The packet counter of each rule that has one, in rule order.
    pub counters: Vec<u64>,
}

fn family_of(typ: &str) -> Option<Family> {
    match typ {
        "ipv4_addr" => Some(Family::V4),
        "ipv6_addr" => Some(Family::V6),
        _ => None,
    }
}

fn addr(text: &str) -> Option<(Family, u128)> {
    match text.parse::<IpAddr>().ok()? {
        IpAddr::V4(v4) => Some((Family::V4, u128::from(u32::from(v4)))),
        IpAddr::V6(v6) => Some((Family::V6, u128::from(v6))),
    }
}

fn span(value: &Value) -> Option<(Family, u128, u128)> {
    if let Some(text) = value.as_str() {
        let (family, a) = addr(text)?;
        return Some((family, a, a));
    }
    let object = value.as_object()?;
    if let Some(prefix) = object.get("prefix") {
        let (family, base) = addr(prefix.get("addr")?.as_str()?)?;
        let len = u32::try_from(prefix.get("len")?.as_u64()?).ok()?;
        let bits = match family {
            Family::V4 => 32,
            Family::V6 => 128,
        };
        let host_bits = bits.checked_sub(len)?;
        let mask = if host_bits >= 128 {
            u128::MAX
        } else {
            (1u128 << host_bits) - 1
        };
        return Some((family, base & !mask, (base & !mask) | mask));
    }
    let range = object.get("range")?.as_array()?;
    let (family, lo) = addr(range.first()?.as_str()?)?;
    let (_, hi) = addr(range.get(1)?.as_str()?)?;
    Some((family, lo, hi))
}

fn member(value: &Value) -> Option<Member> {
    let (inner, timeout, expires) = match value.get("elem") {
        Some(elem) => (
            elem.get("val")?,
            elem.get("timeout").and_then(Value::as_i64),
            elem.get("expires").and_then(Value::as_i64),
        ),
        None => (value, None, None),
    };
    let (family, lo, hi) = span(inner)?;
    Some(Member {
        family,
        lo,
        hi,
        timeout,
        expires,
    })
}

/// A short text that names what a rule matches and does, such as `ip.saddr@blocked4:counter,drop`.
#[must_use]
pub fn rule_signature(exprs: &[Value]) -> String {
    let mut parts = Vec::new();
    let mut matched = String::new();
    for expr in exprs {
        let Some(object) = expr.as_object() else {
            continue;
        };
        if let Some(m) = object.get("match") {
            let payload = m.pointer("/left/payload");
            let proto = payload
                .and_then(|p| p.get("protocol"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let field = payload
                .and_then(|p| p.get("field"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let right = m.get("right").and_then(Value::as_str).unwrap_or("?");
            matched = format!("{proto}.{field}{right}");
        } else if object.contains_key("counter") {
            parts.push("counter".to_owned());
        } else if object.contains_key("accept") {
            parts.push("accept".to_owned());
        } else if object.contains_key("drop") {
            parts.push("drop".to_owned());
        } else {
            parts.push("other".to_owned());
        }
    }
    if matched.is_empty() {
        parts.join(",")
    } else {
        format!("{matched}:{}", parts.join(","))
    }
}

fn counter_packets(exprs: &[Value]) -> Option<u64> {
    exprs
        .iter()
        .find_map(|e| e.get("counter"))
        .and_then(|c| c.get("packets"))
        .and_then(Value::as_u64)
}

/// Reads `nft -j list table` or `nft -j list set` output.
pub fn parse(json: &str) -> Result<TableView, ViewError> {
    let root: Value = serde_json::from_str(json).map_err(|_| ViewError)?;
    let items = root
        .get("nftables")
        .and_then(Value::as_array)
        .ok_or(ViewError)?;
    let mut view = TableView::default();
    for item in items {
        if let Some(set) = item.get("set") {
            let name = set.get("name").and_then(Value::as_str).ok_or(ViewError)?;
            let typ = set.get("type").and_then(Value::as_str).unwrap_or_default();
            let flags = set
                .get("flags")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            let members = set
                .get("elem")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(member).collect())
                .unwrap_or_default();
            let _ = family_of(typ);
            view.sets.insert(
                name.to_owned(),
                SetView {
                    typ: typ.to_owned(),
                    flags,
                    size: set.get("size").and_then(Value::as_u64),
                    timeout: set.get("timeout").and_then(Value::as_u64),
                    members,
                },
            );
        } else if let Some(chain) = item.get("chain") {
            view.chain = Some(ChainView {
                typ: chain
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                hook: chain
                    .get("hook")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                prio: chain.get("prio").and_then(Value::as_i64).unwrap_or(i64::MAX),
                policy: chain
                    .get("policy")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            });
        } else if let Some(rule) = item.get("rule") {
            let exprs = rule
                .get("expr")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice);
            view.rules.push(rule_signature(exprs));
            if let Some(packets) = counter_packets(exprs) {
                view.counters.push(packets);
            }
        }
    }
    Ok(view)
}

/// Whether `nft` said the table (or an object it names) does not exist: exit 1 and the bare
/// `No such file or directory` of a read.
#[must_use]
pub fn is_absent(exit: Option<i32>, stderr: &str) -> bool {
    exit == Some(1) && stderr.contains("No such file or directory")
}

impl TableView {
    /// The members of the sets named in `names`, with the set each came from.
    pub fn members_of<'a>(
        &'a self,
        names: &'a [&'a str],
    ) -> impl Iterator<Item = (&'a str, &'a Member)> + 'a {
        names.iter().flat_map(move |name| {
            self.sets
                .get(*name)
                .into_iter()
                .flat_map(move |set| set.members.iter().map(move |m| (*name, m)))
        })
    }
}
