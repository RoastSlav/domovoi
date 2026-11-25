//! Route observation: what the host's routing state means for the management endpoints.
//!
//! [`observe`] turns a [`RouteSource`] (policy rules, routing tables, interface addresses and
//! kernel lookups) into the sorted route lines of a report. It never hashes a partial picture:
//! the first read that fails, or any rule or route the report cannot express, is a
//! [`ReadError`] and the helper then signs the minimal incomplete report instead.

use core::net::IpAddr;
use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use domovoi_core::attest::{
    Cidr, EndpointRoute, Family, IncompleteReason, LinkPeer, LocalAddr, Route, RouteLines, Rule,
    Selector,
};

/// The `main` routing table.
pub const MAIN_TABLE: u32 = 254;
/// The `local` routing table.
pub const LOCAL_TABLE: u32 = 255;

/// Why the route state could not be observed completely. Each variant is one signed
/// `incomplete_reason`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// The configuration needed to name the endpoints cannot be read.
    #[error("the endpoint configuration cannot be read")]
    ConfigUnreadable,
    /// The policy rules could not be read.
    #[error("the policy rules could not be read")]
    RulesUnreadable,
    /// A referenced routing table could not be read.
    #[error("routing table {0} could not be read")]
    TableUnreadable(u32),
    /// The interface addresses could not be read.
    #[error("the interface addresses could not be read")]
    AddressesUnreadable,
    /// A route lookup failed.
    #[error("a route lookup failed")]
    LookupFailed,
    /// A read returned output the parser could not complete.
    #[error("a read returned incomplete output: {0}")]
    Partial(String),
    /// A rule or route the report cannot express.
    #[error("the route state cannot be expressed: {0}")]
    Unrepresentable(String),
}

impl ReadError {
    /// The `incomplete_reason` token the helper signs.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::ConfigUnreadable => IncompleteReason::CONFIG_UNREADABLE,
            Self::RulesUnreadable => IncompleteReason::RULES_UNREADABLE,
            Self::TableUnreadable(_) => IncompleteReason::TABLE_UNREADABLE,
            Self::AddressesUnreadable => IncompleteReason::ADDRESSES_UNREADABLE,
            Self::LookupFailed => IncompleteReason::LOOKUP_FAILED,
            Self::Partial(_) => IncompleteReason::ROUTE_READ_PARTIAL,
            Self::Unrepresentable(_) => IncompleteReason::ROUTE_UNREPRESENTABLE,
        }
    }
}

/// What a policy rule does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleAction {
    /// Look up a routing table.
    Lookup,
    /// Anything else (blackhole, prohibit, goto, suppression ...): the report cannot express it.
    Other(String),
}

/// One policy rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    /// The address family the rule belongs to.
    pub family: Family,
    /// The rule's priority.
    pub priority: u32,
    /// What the rule matches.
    pub selector: Selector,
    /// What the rule does.
    pub action: RuleAction,
    /// The table a lookup rule consults; 0 for other actions.
    pub table: u32,
}

/// What kind of route an entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteKind {
    /// An ordinary unicast route.
    Unicast,
    /// Local, broadcast, blackhole, unreachable, prohibit, multicast ... with the raw type text.
    Other(String),
}

/// One route of a routing table, or the route a lookup selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRoute {
    /// The address family.
    pub family: Family,
    /// The table the route lives in.
    pub table: u32,
    /// The destination prefix.
    pub dst: Cidr,
    /// The next hop, if any.
    pub gateway: Option<IpAddr>,
    /// The output interface as raw bytes.
    pub ifname: Vec<u8>,
    /// The route's metric; 0 when the kernel prints none.
    pub metric: u32,
    /// The route's kind.
    pub kind: RouteKind,
    /// Whether the route has several next hops.
    pub multipath: bool,
}

impl TableRoute {
    /// The report line of this route.
    #[must_use]
    pub fn to_line(&self) -> Route {
        Route {
            family: self.family,
            table: self.table,
            dst: self.dst,
            gateway: self.gateway,
            ifname: self.ifname.clone(),
            metric: self.metric,
        }
    }

    fn check_expressible(&self) -> Result<(), ReadError> {
        if let RouteKind::Other(kind) = &self.kind {
            return Err(ReadError::Unrepresentable(format!("route type {kind}")));
        }
        if self.multipath {
            return Err(ReadError::Unrepresentable("a multipath route".to_owned()));
        }
        Ok(())
    }
}

/// One address on an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfAddress {
    /// The address with its prefix length, host bits kept.
    pub addr: Cidr,
    /// The interface name as raw bytes.
    pub ifname: Vec<u8>,
}

/// What the kernel answered for one destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    /// The destination that was asked about.
    pub endpoint: IpAddr,
    /// The table the lookup selected.
    pub table: u32,
    /// The next hop, if any.
    pub gateway: Option<IpAddr>,
    /// The output interface as raw bytes.
    pub ifname: Vec<u8>,
    /// The source address the kernel chose.
    pub src: IpAddr,
    /// The metric the kernel printed; 0 when it prints none.
    pub metric: u32,
    /// The kind of the selected route.
    pub kind: RouteKind,
    /// Whether the selected route has several next hops.
    pub multipath: bool,
}

/// The routing facts of the host.
#[async_trait]
pub trait RouteSource: Send + Sync {
    /// The policy rules of both families.
    async fn rules(&self) -> Result<Vec<PolicyRule>, ReadError>;
    /// Every route of `table`, both families. A table that does not exist has no routes.
    async fn table_routes(&self, table: u32) -> Result<Vec<TableRoute>, ReadError>;
    /// Every interface address.
    async fn addresses(&self) -> Result<Vec<IfAddress>, ReadError>;
    /// The kernel's own answer for `endpoint`, honouring the policy rules.
    async fn lookup(&self, endpoint: IpAddr) -> Result<Lookup, ReadError>;
}

/// The management endpoints the route state is observed for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Endpoints {
    /// The hub's literal addresses.
    pub hub_addrs: Vec<IpAddr>,
    /// The agent's link peers.
    pub link_peers: Vec<(IpAddr, u16)>,
    /// The configured management networks.
    pub cidrs: Vec<Cidr>,
}

fn longest_match(routes: &[TableRoute], lookup: &Lookup) -> Option<TableRoute> {
    routes
        .iter()
        .filter(|route| {
            route.kind == RouteKind::Unicast
                && route.table == lookup.table
                && Family::of(lookup.endpoint) == route.family
                && Cidr::host(lookup.endpoint).subset_of(&route.dst)
                && route.gateway == lookup.gateway
                && route.ifname == lookup.ifname
        })
        .max_by_key(|route| (route.dst.prefix, std::cmp::Reverse(route.metric)))
        .cloned()
}

/// Observes the route state for `endpoints` and returns the sorted route lines.
///
/// `rule` lines are every lookup rule; the referenced tables are the rule tables plus `main` and
/// `local`; `route` lines are every default route of a referenced table plus, for each endpoint,
/// the route the kernel selected (found as the longest prefix of the selected table that matches
/// the kernel's answer); `local_addr` lines are the addresses on the interfaces the endpoint
/// lookups used. The endpoints are every hub address, every link peer and the network address of
/// every configured prefix.
///
/// # Errors
///
/// The first [`ReadError`]: a read that failed, a rule whose action is not a lookup, a selected
/// route that is not a plain unicast route, or a route the kernel selected that the table dump
/// does not show. Nothing is ever left out of the lines instead.
pub async fn observe(
    src: &dyn RouteSource,
    endpoints: &Endpoints,
) -> Result<RouteLines, ReadError> {
    let rules = src.rules().await?;
    let mut tables: BTreeSet<u32> = BTreeSet::from([MAIN_TABLE, LOCAL_TABLE]);
    let mut out = RouteLines::default();
    for rule in &rules {
        match &rule.action {
            RuleAction::Lookup => {
                tables.insert(rule.table);
                out.rule.push(
                    Rule {
                        family: rule.family,
                        priority: rule.priority,
                        selector: rule.selector.clone(),
                        table: rule.table,
                    }
                    .render(),
                );
            }
            RuleAction::Other(action) => {
                return Err(ReadError::Unrepresentable(format!("rule action {action}")));
            }
        }
    }
    let mut dumps: BTreeMap<u32, Vec<TableRoute>> = BTreeMap::new();
    let mut selected: Vec<TableRoute> = Vec::new();
    for table in &tables {
        let routes = src.table_routes(*table).await?;
        for route in routes.iter().filter(|route| route.dst.prefix == 0) {
            route.check_expressible()?;
            selected.push(route.clone());
        }
        dumps.insert(*table, routes);
    }
    let mut addrs: BTreeSet<IpAddr> = BTreeSet::new();
    addrs.extend(endpoints.hub_addrs.iter().copied());
    addrs.extend(endpoints.link_peers.iter().map(|(ip, _)| *ip));
    addrs.extend(endpoints.cidrs.iter().map(|cidr| cidr.network().addr));
    let mut ifaces: BTreeSet<Vec<u8>> = BTreeSet::new();
    for endpoint in addrs {
        let lookup = src.lookup(endpoint).await?;
        let probe = TableRoute {
            family: Family::of(endpoint),
            table: lookup.table,
            dst: Cidr::host(endpoint),
            gateway: lookup.gateway,
            ifname: lookup.ifname.clone(),
            metric: lookup.metric,
            kind: lookup.kind.clone(),
            multipath: lookup.multipath,
        };
        probe.check_expressible()?;
        if let std::collections::btree_map::Entry::Vacant(entry) = dumps.entry(lookup.table) {
            entry.insert(src.table_routes(lookup.table).await?);
        }
        let route = dumps
            .get(&lookup.table)
            .and_then(|routes| longest_match(routes, &lookup))
            .ok_or_else(|| {
                ReadError::Partial("the selected route is not in the table dump".to_owned())
            })?;
        ifaces.insert(lookup.ifname.clone());
        out.endpoint_route.push(
            EndpointRoute {
                endpoint,
                table: lookup.table,
                gateway: lookup.gateway,
                ifname: lookup.ifname,
                src: lookup.src,
            }
            .render(),
        );
        selected.push(route);
    }
    out.route = selected
        .iter()
        .map(|route| route.to_line().render())
        .collect();
    for address in src.addresses().await? {
        if ifaces.contains(&address.ifname) {
            out.local_addr.push(
                LocalAddr {
                    addr: address.addr,
                    ifname: address.ifname,
                }
                .render(),
            );
        }
    }
    out.hub_addr = endpoints
        .hub_addrs
        .iter()
        .map(ToString::to_string)
        .collect();
    out.link_peer = endpoints
        .link_peers
        .iter()
        .map(|(ip, port)| {
            LinkPeer {
                ip: *ip,
                port: *port,
            }
            .render()
        })
        .collect();
    out.normalise();
    Ok(out)
}
