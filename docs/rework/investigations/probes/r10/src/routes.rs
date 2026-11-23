//! Route-state observation through a `RouteSource` and the route lines it yields (RW-37 route
//! coverage). The source is an interface a live netlink reader could also implement; the
//! selection logic in [`observe`] is the code under test, the kernel is not reimplemented.

use std::collections::BTreeSet;
use std::net::IpAddr;

use crate::encoding::{Cidr, Family};
use crate::hash::{ROUTES_PREFIX, domain_hash};
use crate::lines::{EndpointRoute, LinkPeer, LocalAddr, Route, Rule, Selector};

/// Table ids the observer always reads: `main` and `local`.
pub const MAIN_TABLE: u32 = 254;
/// The `local` table id.
pub const LOCAL_TABLE: u32 = 255;

/// What a policy rule does. Only `Lookup` has a representation in the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleAction {
    /// `lookup <table>`
    Lookup,
    /// `unreachable`, `blackhole`, `prohibit`, `goto`, `nop`: no `table` field can express it.
    Other(String),
}

/// One policy rule as read from the kernel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// Family.
    pub family: Family,
    /// Priority.
    pub priority: u32,
    /// Selector.
    pub selector: Selector,
    /// Table (meaningful for `Lookup`).
    pub table: u32,
    /// Action.
    pub action: RuleAction,
}

/// One route in a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRoute {
    /// Family.
    pub family: Family,
    /// Table.
    pub table: u32,
    /// Destination.
    pub dst: Cidr,
    /// Gateway.
    pub gateway: Option<IpAddr>,
    /// Interface.
    pub ifname: Vec<u8>,
    /// Metric.
    pub metric: u32,
    /// Preferred source, if the route sets one.
    pub prefsrc: Option<IpAddr>,
}

impl TableRoute {
    fn to_line(&self) -> Route {
        Route {
            family: self.family,
            table: self.table,
            dst: self.dst,
            gateway: self.gateway,
            ifname: self.ifname.clone(),
            metric: self.metric,
        }
    }
}

/// An address on an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfAddress {
    /// Address with prefix, host bits kept.
    pub addr: Cidr,
    /// Interface.
    pub ifname: Vec<u8>,
}

/// A `ip route get` style answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The route that was selected.
    pub route: TableRoute,
    /// Source address the kernel would use.
    pub src: IpAddr,
}

/// Why route state could not be read completely. Each maps to an `incomplete_reason` token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// Policy rules could not be read.
    #[error("policy rules unreadable")]
    RulesUnreadable,
    /// A referenced table could not be read.
    #[error("table {0} unreadable")]
    TableUnreadable(u32),
    /// Interface addresses could not be read.
    #[error("interface addresses unreadable")]
    AddressesUnreadable,
    /// A route lookup failed.
    #[error("lookup for {0} failed")]
    LookupFailed(IpAddr),
    /// A read was cut short.
    #[error("partial read: {0}")]
    Partial(String),
    /// A rule exists that the report format cannot express.
    #[error("rule action {0} cannot be represented")]
    RuleUnrepresentable(String),
}

impl ReadError {
    /// The `incomplete_reason` token. The contract only says "with a reason"; these tokens are
    /// the harness's choice.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::RulesUnreadable => "rules_unreadable",
            Self::TableUnreadable(_) => "table_unreadable",
            Self::AddressesUnreadable => "addresses_unreadable",
            Self::LookupFailed(_) => "lookup_failed",
            Self::Partial(_) => "route_read_partial",
            Self::RuleUnrepresentable(_) => "rule_unrepresentable",
        }
    }
}

/// A reader of kernel routing state. A live netlink implementation would implement the same trait.
pub trait RouteSource {
    /// All IPv4 and IPv6 policy rules.
    ///
    /// # Errors
    /// [`ReadError`] when unreadable.
    fn rules(&self) -> Result<Vec<PolicyRule>, ReadError>;
    /// All routes of one table, both families.
    ///
    /// # Errors
    /// [`ReadError`] when unreadable.
    fn table_routes(&self, table: u32) -> Result<Vec<TableRoute>, ReadError>;
    /// All interface addresses.
    ///
    /// # Errors
    /// [`ReadError`] when unreadable.
    fn addresses(&self) -> Result<Vec<IfAddress>, ReadError>;
    /// Kernel route lookup for one endpoint, honouring policy rules.
    ///
    /// # Errors
    /// [`ReadError`] when the lookup fails.
    fn lookup(&self, endpoint: IpAddr) -> Result<Lookup, ReadError>;
}

/// Management endpoints: where the helper must not cut the host off.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    /// Hub addresses from root-owned configuration.
    pub hub_addrs: Vec<IpAddr>,
    /// Kernel-observed link peers (address, port).
    pub link_peers: Vec<(IpAddr, u16)>,
    /// Configured management CIDRs (network addresses are looked up).
    pub cidrs: Vec<Cidr>,
}

/// The route half of the report: encoded values per key, sorted and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RouteLines {
    /// `hub_addr` values.
    pub hub_addr: Vec<String>,
    /// `link_peer` values.
    pub link_peer: Vec<String>,
    /// `local_addr` values.
    pub local_addr: Vec<String>,
    /// `route` values.
    pub route: Vec<String>,
    /// `rule` values.
    pub rule: Vec<String>,
    /// `endpoint_route` values.
    pub endpoint_route: Vec<String>,
}

impl RouteLines {
    /// Full `key=value` lines in report order.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        for (k, vals) in [
            ("hub_addr", &self.hub_addr),
            ("link_peer", &self.link_peer),
            ("local_addr", &self.local_addr),
            ("route", &self.route),
            ("rule", &self.rule),
            ("endpoint_route", &self.endpoint_route),
        ] {
            v.extend(vals.iter().map(|x| format!("{k}={x}")));
        }
        v
    }

    /// `route_state_hash`.
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        domain_hash(ROUTES_PREFIX, &self.lines())
    }

    /// Sort bytewise and drop exact duplicates in every group.
    pub fn normalise(&mut self) {
        for g in [
            &mut self.hub_addr,
            &mut self.link_peer,
            &mut self.local_addr,
            &mut self.route,
            &mut self.rule,
            &mut self.endpoint_route,
        ] {
            let s: BTreeSet<String> = g.drain(..).collect();
            g.extend(s);
        }
    }
}

/// Assemble the attested route state for the given endpoints.
///
/// Reading of the contract used here (see the report): `rule` lines are every policy rule with a
/// `lookup` action (each selects a referenced table by definition); `route` lines are the routes
/// the endpoint lookups selected plus every default route in a referenced table (rule tables,
/// `main`, `local`); `local_addr` lines are the addresses on interfaces named by an
/// `endpoint_route`.
///
/// # Errors
/// The first [`ReadError`]; the caller must then report `complete=false` and no hash.
pub fn observe(src: &dyn RouteSource, ep: &Endpoints) -> Result<RouteLines, ReadError> {
    let rules = src.rules()?;
    let mut tables: BTreeSet<u32> = BTreeSet::from([MAIN_TABLE, LOCAL_TABLE]);
    let mut out = RouteLines::default();
    for r in &rules {
        match &r.action {
            RuleAction::Lookup => {
                tables.insert(r.table);
                out.rule.push(
                    Rule {
                        family: r.family,
                        priority: r.priority,
                        selector: r.selector.clone(),
                        table: r.table,
                    }
                    .render(),
                );
            }
            RuleAction::Other(a) => return Err(ReadError::RuleUnrepresentable(a.clone())),
        }
    }
    let mut selected: Vec<TableRoute> = Vec::new();
    for t in &tables {
        for route in src.table_routes(*t)? {
            if route.dst.prefix == 0 {
                selected.push(route);
            }
        }
    }
    let mut endpoints: BTreeSet<IpAddr> = BTreeSet::new();
    endpoints.extend(ep.hub_addrs.iter().copied());
    endpoints.extend(ep.link_peers.iter().map(|(ip, _)| *ip));
    endpoints.extend(ep.cidrs.iter().map(|c| c.network().addr));
    let mut ifaces: BTreeSet<Vec<u8>> = BTreeSet::new();
    for e in endpoints {
        let l = src.lookup(e)?;
        ifaces.insert(l.route.ifname.clone());
        out.endpoint_route.push(
            EndpointRoute {
                endpoint: e,
                table: l.route.table,
                gateway: l.route.gateway,
                ifname: l.route.ifname.clone(),
                src: l.src,
            }
            .render(),
        );
        selected.push(l.route);
    }
    out.route = selected.iter().map(|r| r.to_line().render()).collect();
    for a in src.addresses()? {
        if ifaces.contains(&a.ifname) {
            out.local_addr.push(LocalAddr { addr: a.addr, ifname: a.ifname }.render());
        }
    }
    out.hub_addr = ep.hub_addrs.iter().map(ToString::to_string).collect();
    out.link_peer = ep
        .link_peers
        .iter()
        .map(|(ip, port)| LinkPeer { ip: *ip, port: *port }.render())
        .collect();
    out.normalise();
    Ok(out)
}
