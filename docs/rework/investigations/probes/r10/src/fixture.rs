//! Synthetic, deterministic route snapshots. A fixture is not a live kernel observation.

use std::net::IpAddr;

use crate::encoding::{Cidr, Family};
use crate::lines::Selector;
use crate::routes::{
    IfAddress, Lookup, PolicyRule, ReadError, RouteSource, RuleAction, TableRoute,
};

/// Failures to inject into a fixture read.
#[derive(Debug, Clone, Default)]
pub struct Failures {
    /// Policy rules unreadable.
    pub rules: bool,
    /// Tables that cannot be read.
    pub tables: Vec<u32>,
    /// Addresses unreadable.
    pub addrs: bool,
    /// Endpoints whose lookup fails.
    pub lookups: Vec<IpAddr>,
    /// A truncated table read, reported as partial.
    pub partial_table: Option<u32>,
}

/// A whole routing snapshot.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Policy rules.
    pub rules: Vec<PolicyRule>,
    /// Routes of all tables.
    pub routes: Vec<TableRoute>,
    /// Interface addresses.
    pub addrs: Vec<IfAddress>,
    /// Injected failures.
    pub failures: Failures,
}

/// A [`RouteSource`] over a [`Snapshot`]. Lookup is a small emulation: rules in priority order,
/// longest prefix then lowest metric inside the chosen table. Rules whose selector needs more
/// than `to` (or a default `from`) do not match a bare lookup.
#[derive(Debug, Clone)]
pub struct FixtureSource(pub Snapshot);

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// Parse `a/p` for fixtures (panics are avoided: bad input becomes `0.0.0.0/0`).
#[must_use]
pub fn cidr(s: &str) -> Cidr {
    crate::encoding::parse_cidr_config(s).unwrap_or(Cidr::host(ip("0.0.0.0")))
}

/// A table route.
#[must_use]
pub fn route(table: u32, dst: &str, gw: Option<&str>, dev: &str, metric: u32) -> TableRoute {
    let dst = cidr(dst);
    TableRoute {
        family: dst.family(),
        table,
        dst,
        gateway: gw.map(ip),
        ifname: dev.as_bytes().to_vec(),
        metric,
        prefsrc: None,
    }
}

/// A lookup rule with a selector.
#[must_use]
pub fn rule(fam: Family, prio: u32, selector: Selector, table: u32) -> PolicyRule {
    PolicyRule { family: fam, priority: prio, selector, table, action: RuleAction::Lookup }
}

/// An interface address.
#[must_use]
pub fn addr(a: &str, dev: &str) -> IfAddress {
    IfAddress {
        addr: crate::encoding::parse_ifaddr(a).unwrap_or(Cidr::host(ip("0.0.0.0"))),
        ifname: dev.as_bytes().to_vec(),
    }
}

/// The standard three rules of one family: `local`, `main`, plus the default.
#[must_use]
pub fn base_rules(fam: Family) -> Vec<PolicyRule> {
    vec![
        rule(fam, 0, Selector::default(), 255),
        rule(fam, 32766, Selector::default(), 254),
    ]
}

impl FixtureSource {
    fn selector_matches(sel: &Selector, fam: Family, dst: IpAddr) -> bool {
        let needs_more = sel.iif.is_some()
            || sel.oif.is_some()
            || sel.fwmark.is_some()
            || sel.ipproto.is_some()
            || sel.sport.is_some()
            || sel.dport.is_some()
            || sel.uidrange.is_some()
            || sel.from.is_some_and(|f| f.prefix != 0 || f.family() != fam);
        if needs_more {
            return false;
        }
        let base = sel.to.is_none_or(|t| Cidr::host(dst).subset_of(&t));
        base != sel.not
    }
}

impl RouteSource for FixtureSource {
    fn rules(&self) -> Result<Vec<PolicyRule>, ReadError> {
        if self.0.failures.rules {
            return Err(ReadError::RulesUnreadable);
        }
        Ok(self.0.rules.clone())
    }

    fn table_routes(&self, table: u32) -> Result<Vec<TableRoute>, ReadError> {
        if self.0.failures.tables.contains(&table) {
            return Err(ReadError::TableUnreadable(table));
        }
        if self.0.failures.partial_table == Some(table) {
            return Err(ReadError::Partial(format!("table {table} truncated")));
        }
        Ok(self.0.routes.iter().filter(|r| r.table == table).cloned().collect())
    }

    fn addresses(&self) -> Result<Vec<IfAddress>, ReadError> {
        if self.0.failures.addrs {
            return Err(ReadError::AddressesUnreadable);
        }
        Ok(self.0.addrs.clone())
    }

    fn lookup(&self, endpoint: IpAddr) -> Result<Lookup, ReadError> {
        if self.0.failures.lookups.contains(&endpoint) {
            return Err(ReadError::LookupFailed(endpoint));
        }
        let fam = Family::of(endpoint);
        let mut rules: Vec<&PolicyRule> = self.0.rules.iter().filter(|r| r.family == fam).collect();
        rules.sort_by_key(|r| r.priority);
        for r in rules {
            if r.action != RuleAction::Lookup || !Self::selector_matches(&r.selector, fam, endpoint) {
                continue;
            }
            let best = self
                .0
                .routes
                .iter()
                .filter(|t| t.table == r.table && t.family == fam && Cidr::host(endpoint).subset_of(&t.dst))
                .max_by_key(|t| (t.dst.prefix, std::cmp::Reverse(t.metric)));
            if let Some(t) = best {
                let src = t.prefsrc.or_else(|| {
                    self.0
                        .addrs
                        .iter()
                        .find(|a| a.ifname == t.ifname && Family::of(a.addr.addr) == fam)
                        .map(|a| a.addr.addr)
                });
                return src
                    .map(|src| Lookup { route: t.clone(), src })
                    .ok_or(ReadError::LookupFailed(endpoint));
            }
        }
        Err(ReadError::LookupFailed(endpoint))
    }
}

/// A small snapshot for the child-process tests: one LAN interface, IPv4 only.
#[must_use]
pub fn demo_snapshot() -> Snapshot {
    Snapshot {
        rules: base_rules(Family::Inet),
        routes: vec![
            route(254, "0.0.0.0/0", Some("192.168.1.1"), "eth0", 100),
            route(254, "192.168.1.0/24", None, "eth0", 100),
        ],
        addrs: vec![addr("192.168.1.20/24", "eth0")],
        ..Snapshot::default()
    }
}
