//! What the hub decides before any containment frame leaves: how safe the helper is, which of
//! the host's management paths a verified report does not cover, and whether the target would
//! overlap the verified set.
//!
//! Nothing here reads a value an agent supplied. The verified set comes from the report the hub
//! verified against the owner's pinned key, and the overlap test is the same pure function the
//! helper applies, so the two cannot drift (ADR 0038).

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::action::PlanAssurance;
use domovoi_core::attest::{
    Authority, Cidr, Decision, ExpectedSafety, Hash32, Report, SafetyIdentity, TrustView,
    UnverifiedReason, VerifiedState, decide, gate, parse_report,
};
use domovoi_core::containment::{ExclusionInputs, Net, effective_exclusions, keys};
use domovoi_core::ports::HelperTrustRead;
use jiff::{SignedDuration, Timestamp};

use crate::policy::{ContainmentInput, ManagementPath};

/// The host's discovered management paths, as the candidates the verified report must cover.
#[async_trait]
pub trait PathSource: Send + Sync {
    /// The canonical CIDR of every management path of `host`, or why they cannot be read.
    async fn discovered(&self, host: HostId) -> Result<Vec<Cidr>, String>;
}

/// How a request stands against the helper's verified safety set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    /// What the plan hash binds.
    pub assurance: PlanAssurance,
    /// What every phase sends as `expected_safety`.
    pub expected: ExpectedSafety,
    /// The paths the verified report does not cover; empty for a verified assessment.
    pub unprotected: Vec<Cidr>,
}

impl Assessment {
    /// Whether the helper's safety set was verified and covers every discovered path.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        matches!(self.assurance, PlanAssurance::Verified(_))
    }

    /// The reason coverage could not be verified, when it could not.
    #[must_use]
    pub fn reason(&self) -> Option<UnverifiedReason> {
        match &self.assurance {
            PlanAssurance::Verified(_) => None,
            PlanAssurance::Unverified(reason) => Some(*reason),
        }
    }
}

/// Why no assessment could be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssessError {
    /// A policy run needs a verified identity; this is why there is none.
    #[error("policy containment needs a verified helper: {}", .0.token())]
    PolicyUnverified(UnverifiedReason),
    /// The stored trust or the discovered paths could not be read.
    #[error("the helper's trust state could not be read: {0}")]
    Unreadable(String),
}

/// Assesses `host` for a request under `authority`.
///
/// # Errors
/// [`AssessError::PolicyUnverified`] when a policy request cannot be verified;
/// [`AssessError::Unreadable`] when the store or the path source failed.
pub async fn assess(
    trust: &dyn HelperTrustRead,
    paths: &dyn PathSource,
    host: HostId,
    authority: Authority,
    now: Timestamp,
) -> Result<Assessment, AssessError> {
    let view = trust
        .view(host)
        .await
        .map_err(|err| AssessError::Unreadable(err.to_string()))?;
    let candidates = paths
        .discovered(host)
        .await
        .map_err(AssessError::Unreadable)?;
    match decide(now, &view, authority, &candidates) {
        Decision::Verified(identity) => Ok(Assessment {
            expected: ExpectedSafety::Verified {
                fingerprint: Hash32::from(identity.fingerprint),
                boot_id: identity.boot_id.clone(),
                generation: identity.generation,
                config_hash: identity.config_hash,
                route_state_hash: identity.route_state_hash,
            },
            assurance: PlanAssurance::Verified(identity),
            unprotected: Vec::new(),
        }),
        Decision::UnverifiedHuman {
            reason,
            unprotected,
        } => Ok(Assessment {
            expected: ExpectedSafety::Unverified { reason },
            assurance: PlanAssurance::Unverified(reason),
            unprotected,
        }),
        Decision::Refused(reason) => Err(AssessError::PolicyUnverified(reason)),
    }
}

/// The exclusion inputs the verified report proves: management networks, hub addresses, link
/// peers, local addresses and gateways. `None` when the report cannot be read again.
#[must_use]
pub fn verified_exclusion_inputs(state: &VerifiedState) -> Option<ExclusionInputs> {
    let Ok(Report::Complete(report)) = parse_report(&state.report) else {
        return None;
    };
    let host = |ip: core::net::IpAddr| -> Option<Net> {
        let prefix = if ip.is_ipv4() { 32 } else { 128 };
        Net::parse_exclusion(&format!("{ip}/{prefix}")).ok()
    };
    let network = |cidr: &Cidr| -> Option<Net> {
        let cidr = cidr.network();
        Net::parse_exclusion(&format!("{}/{}", cidr.addr, cidr.prefix)).ok()
    };
    let mut inputs = ExclusionInputs::default();
    for cidr in &state.cidrs {
        inputs.management_cidrs.push(network(cidr)?);
    }
    for ip in &state.hub_addrs {
        inputs.hub_addrs.push(host(*ip)?);
    }
    for (ip, _port) in &state.link_peers {
        inputs.link_peers.push(host(*ip)?);
    }
    for line in &report.routes.local_addr {
        let local = domovoi_core::attest::LocalAddr::parse(line).ok()?;
        inputs.local_addrs.push(host(local.addr.addr)?);
    }
    for line in &report.routes.route {
        let route = domovoi_core::attest::Route::parse(line).ok()?;
        if let Some(gateway) = route.gateway {
            inputs.gateways.push(host(gateway)?);
        }
    }
    Some(inputs)
}

/// Whether `net` overlaps the verified set or a fixed range. Anything that cannot be
/// established counts as an overlap: the safe answer is to refuse.
#[must_use]
pub fn overlaps_verified_set(net: &Net, state: &VerifiedState) -> bool {
    let inputs = verified_exclusion_inputs(state);
    match effective_exclusions(inputs.as_ref()) {
        Ok(all) => all.iter().any(|excluded| excluded.overlaps(net)),
        Err(_) => true,
    }
}

/// The containment posture the policy evaluator reads, built from the stored trust and the
/// host's discovered paths. Nothing is fetched during evaluation: callers build this first.
#[must_use]
pub fn containment_input(
    now: Timestamp,
    view: &TrustView,
    net: &Net,
    discovered: &[Cidr],
    stale_confirmation: bool,
) -> ContainmentInput {
    let checked = gate(now, view);
    let helper_incomplete = matches!(
        view.latest,
        Some(domovoi_core::attest::LatestOutcome::SignedIncomplete)
    );
    let uncovered = domovoi_core::attest::unprotected_paths(checked, discovered)
        .into_iter()
        .filter_map(|cidr| ManagementPath::parse(&format!("{}/{}", cidr.addr, cidr.prefix)).ok())
        .collect();
    ContainmentInput {
        attested: checked.is_ok(),
        uncovered_paths: uncovered,
        stale_confirmation,
        target_overlaps_verified_set: checked
            .map_or(true, |state| overlaps_verified_set(net, state)),
        helper_incomplete,
    }
}

/// What `expected_safety` a phase of a stored block carries: rebuilt from the assurance the block
/// recorded when it was approved, never invented. `None` when the record cannot be read.
#[must_use]
pub fn expected_from_assurance(assurance: &str) -> Option<ExpectedSafety> {
    let value: serde_json::Value = serde_json::from_str(assurance).ok()?;
    match value.get("kind")?.as_str()? {
        "verified" => {
            let identity =
                SafetyIdentity::parse_canonical(value.get("identity")?.as_str()?).ok()?;
            Some(ExpectedSafety::Verified {
                fingerprint: Hash32::from(identity.fingerprint),
                boot_id: identity.boot_id,
                generation: identity.generation,
                config_hash: identity.config_hash,
                route_state_hash: identity.route_state_hash,
            })
        }
        "unverified" => Some(ExpectedSafety::Unverified {
            reason: UnverifiedReason::parse(value.get("reason")?.as_str()?).ok()?,
        }),
        _ => None,
    }
}

/// The longest block a request may ask for: a year, which `nft` accepts.
pub const MAX_TTL_S: i64 = 31_536_000;

/// The network and the TTL a containment step asks for, from its stored arguments.
pub fn parse_request(args: &BTreeMap<String, String>) -> Result<(Net, SignedDuration), String> {
    let source = args
        .get(keys::ARG_SOURCE)
        .ok_or_else(|| "a containment step names its source".to_owned())?;
    let net = Net::parse(source).map_err(|err| format!("invalid_target: {err}"))?;
    if net.canonical() != *source {
        return Err("invalid_target: the source is not in canonical form".to_owned());
    }
    let ttl: i64 = args
        .get(keys::ARG_TTL_S)
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| "a containment step names its ttl_s".to_owned())?;
    if ttl <= super::STEP_ONE_SECS {
        return Err("ttl_too_short".to_owned());
    }
    if ttl > MAX_TTL_S {
        return Err("ttl_above_maximum".to_owned());
    }
    Ok((net, SignedDuration::from_secs(ttl)))
}
