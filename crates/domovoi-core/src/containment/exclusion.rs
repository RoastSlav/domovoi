//! The exclusion algorithm: what a block may never cover, computed the same way by the
//! helper and the hub.

use super::net::{Family, Net};

/// The fixed ranges that are never blocked: loopback, link-local, unspecified, multicast and
/// broadcast, both families.
#[must_use]
pub fn fixed_exclusions() -> Vec<Net> {
    const V4: [(u32, u8); 5] = [
        (0x7f00_0000, 8),
        (0xa9fe_0000, 16),
        (0, 8),
        (0xe000_0000, 4),
        (0xffff_ffff, 32),
    ];
    let mut out = Vec::new();
    for (base, prefix) in V4 {
        let mask = if prefix == 32 { 0 } else { u32::MAX >> prefix };
        out.push(Net::raw(
            Family::V4,
            u128::from(base),
            u128::from(base | mask),
            prefix,
        ));
    }
    out.push(Net::raw(Family::V6, 1, 1, 128));
    out.push(Net::raw(Family::V6, 0, 0, 128));
    let fe80 = 0xfe80u128 << 112;
    out.push(Net::raw(Family::V6, fe80, fe80 | (u128::MAX >> 10), 10));
    let ff00 = 0xff00u128 << 112;
    out.push(Net::raw(Family::V6, ff00, ff00 | (u128::MAX >> 8), 8));
    out
}

/// What the helper knows about the host's own paths, from root-owned state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExclusionInputs {
    /// Management CIDRs from the helper's configuration.
    pub management_cidrs: Vec<Net>,
    /// Addresses of the hub.
    pub hub_addrs: Vec<Net>,
    /// Peers of the agent's control link.
    pub link_peers: Vec<Net>,
    /// Addresses of the host itself.
    pub local_addrs: Vec<Net>,
    /// Default gateways.
    pub gateways: Vec<Net>,
}

/// Why no exclusion set can be computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExclusionRefusal {
    /// An input could not be established, so all containment is refused.
    #[error("an exclusion input could not be established")]
    Incomplete,
    /// No management CIDR is configured.
    #[error("no management CIDR is configured")]
    NoManagementCidrs,
}

/// The fixed ranges plus every input, mapped members written as IPv4, sorted and deduplicated.
///
/// # Errors
/// `Incomplete` when `inputs` is `None`; `NoManagementCidrs` when none is configured.
pub fn effective_exclusions(
    inputs: Option<&ExclusionInputs>,
) -> Result<Vec<Net>, ExclusionRefusal> {
    let inputs = inputs.ok_or(ExclusionRefusal::Incomplete)?;
    if inputs.management_cidrs.is_empty() {
        return Err(ExclusionRefusal::NoManagementCidrs);
    }
    let mut all = fixed_exclusions();
    for list in [
        &inputs.management_cidrs,
        &inputs.hub_addrs,
        &inputs.link_peers,
        &inputs.local_addrs,
        &inputs.gateways,
    ] {
        all.extend(list.iter().map(|net| net.canonicalise_mapped()));
    }
    all.sort();
    all.dedup();
    Ok(all)
}

/// The helper's own prefix floors; never `/0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPolicy {
    /// Shortest IPv4 prefix allowed.
    pub min_prefix_v4: u8,
    /// Shortest IPv6 prefix allowed.
    pub min_prefix_v6: u8,
}

/// Why a target is refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetRefusal {
    /// The prefix is shorter than the policy minimum.
    #[error("the prefix is wider than the helper allows")]
    PrefixTooWide,
    /// The target overlaps an exclusion, in either direction.
    #[error("the target overlaps the excluded range {0}")]
    ExclusionOverlap(Net),
}

/// Checks a target: first the prefix floor, then overlap with every exclusion of its own
/// family. A `/32` inside a management `/24` and a `/16` that contains it are both refused.
///
/// # Errors
/// [`TargetRefusal`] naming the first failure.
pub fn check_target(
    net: &Net,
    exclusions: &[Net],
    policy: &TargetPolicy,
) -> Result<(), TargetRefusal> {
    let floor = match net.family() {
        Family::V4 => policy.min_prefix_v4,
        Family::V6 => policy.min_prefix_v6,
    }
    .max(1);
    if net.prefix_len() < floor {
        return Err(TargetRefusal::PrefixTooWide);
    }
    match exclusions.iter().find(|ex| net.overlaps(ex)) {
        Some(ex) => Err(TargetRefusal::ExclusionOverlap(*ex)),
        None => Ok(()),
    }
}

/// Disjoint ranges in ascending order, overlapping and adjacent ones merged, for rendering an
/// interval set.
#[must_use]
pub fn coalesce(nets: &[Net]) -> Vec<(u128, u128, Family)> {
    let mut ranges: Vec<(Family, u128, u128)> = nets
        .iter()
        .map(|net| (net.family(), net.lo(), net.hi()))
        .collect();
    ranges.sort_unstable();
    let mut out: Vec<(u128, u128, Family)> = Vec::new();
    for (family, lo, hi) in ranges {
        match out.last_mut() {
            Some((_, last_hi, last_family))
                if *last_family == family && lo <= last_hi.saturating_add(1) =>
            {
                *last_hi = (*last_hi).max(hi);
            }
            _ => out.push((lo, hi, family)),
        }
    }
    out
}
