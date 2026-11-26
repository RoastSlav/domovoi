//! What the helper enforces for every caller: the prefix floors, the TTL window, `max_active`
//! counted from the sets, and the exclusion set from root-owned state.

use domovoi_core::attest::{Cidr, LinkPeer, LocalAddr, RouteLines};
use domovoi_core::containment::templates::STEP1_TTL_S;
use domovoi_core::containment::{ExclusionInputs, Net, TargetPolicy, Token};

use super::Refusal;
use super::view::TableView;

/// The shortest IPv4 prefix the helper blocks, for every caller.
pub const MIN_PREFIX_V4: u8 = 24;
/// The shortest IPv6 prefix the helper blocks, for every caller.
pub const MIN_PREFIX_V6: u8 = 64;

/// The helper's prefix floors.
#[must_use]
pub const fn policy() -> TargetPolicy {
    TargetPolicy {
        min_prefix_v4: MIN_PREFIX_V4,
        min_prefix_v6: MIN_PREFIX_V6,
    }
}

/// The sets whose members count toward `max_active`.
#[must_use]
pub fn counted_sets(interval_path: bool) -> &'static [&'static str] {
    if interval_path {
        &["blocked4", "blocked6", "blockednet4", "blockednet6"]
    } else {
        &["blocked4", "blocked6"]
    }
}

/// How many elements the block sets hold, foreign ones included, read from the JSON.
#[must_use]
pub fn active_count(view: &TableView, interval_path: bool) -> usize {
    view.members_of(counted_sets(interval_path)).count()
}

/// Refuses a TTL at or below the first step's 30 seconds, or above the helper's maximum. A TTL
/// is refused, never clamped.
pub fn check_ttl(ttl_s: u32, max_ttl_s: u64) -> Result<(), Refusal> {
    if ttl_s <= STEP1_TTL_S {
        return Err(Refusal::new(
            Token::TtlTooShort,
            format!("the TTL must be above {STEP1_TTL_S}s"),
        ));
    }
    if u64::from(ttl_s) > max_ttl_s {
        return Err(Refusal::new(
            Token::TtlAboveMaximum,
            format!("the TTL is above the maximum of {max_ttl_s}s"),
        ));
    }
    Ok(())
}

/// Refuses one more block when the sets already hold `max_active`; `0` refuses everything.
pub fn check_active(view: &TableView, interval_path: bool, max_active: u32) -> Result<(), Refusal> {
    let active = active_count(view, interval_path);
    if active >= max_active as usize {
        return Err(Refusal::new(
            Token::MaxActiveReached,
            format!("{active} elements are active; the maximum is {max_active}"),
        ));
    }
    Ok(())
}

fn cidr_net(cidr: &Cidr) -> Option<Net> {
    Net::parse_exclusion(&cidr.network().render()).ok()
}

fn host_net(ip: std::net::IpAddr) -> Option<Net> {
    let cidr = Cidr::host(ip);
    Net::parse_exclusion(&cidr.render()).ok()
}

/// The exclusion inputs from the helper's observation of itself: the management CIDRs of the
/// configuration, the hub's addresses, the link peers, the host's own addresses and the
/// gateways of its routes. `None` when any line cannot be read, so all containment is refused.
#[must_use]
pub fn inputs_from(cidrs: &[String], routes: &RouteLines) -> Option<ExclusionInputs> {
    let mut inputs = ExclusionInputs::default();
    for text in cidrs {
        inputs
            .management_cidrs
            .push(cidr_net(&Cidr::parse_report(text).ok()?)?);
    }
    for text in &routes.hub_addr {
        inputs.hub_addrs.push(host_net(text.parse().ok()?)?);
    }
    for text in &routes.link_peer {
        inputs
            .link_peers
            .push(host_net(LinkPeer::parse(text).ok()?.ip)?);
    }
    for text in &routes.local_addr {
        inputs
            .local_addrs
            .push(host_net(LocalAddr::parse(text).ok()?.addr.addr)?);
    }
    for text in &routes.route {
        let route = domovoi_core::attest::Route::parse(text).ok()?;
        if let Some(gateway) = route.gateway {
            inputs.gateways.push(host_net(gateway)?);
        }
    }
    Some(inputs)
}
