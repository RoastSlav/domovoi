//! What a policy permit for a block is issued against: the plan recomputed now, the helper's
//! verified identity, the evaluator's containment input and the arguments the ceiling reads.
//!
//! The permit issuer (in the hub) calls [`block_facts`] and nothing else here. A block is only
//! ever permitted against a verified helper identity; an unverified one is refused with the
//! reason the person would have been shown.

use std::collections::BTreeMap;

use domovoi_core::action::ContainmentPlan;
use domovoi_core::attest::{Authority, UnverifiedReason};
use domovoi_core::capability::TargetQuery;
use domovoi_core::containment::{Family, Net, keys};
use domovoi_core::{ExecutionRecord, PlanHash};
use jiff::{SignedDuration, Timestamp};

use super::ContainmentDriver;
use super::input::{AssessError, assess, containment_input, parse_request};
use crate::policy::ContainmentInput;

/// Why no permit can be issued for a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockFactsError {
    /// The helper is not verified, for this reason; only a person may approve the block.
    Unverified(UnverifiedReason),
    /// The step's arguments are not a valid block.
    BadRequest(String),
    /// The host, its trust state or its paths could not be read.
    Unreadable(String),
}

/// What a permit for one block is issued against.
#[derive(Debug, Clone)]
pub struct BlockFacts {
    /// The plan hash recomputed now; the issuer compares it with the stored one.
    pub plan: PlanHash,
    /// The verified safety identity, as the permit carries it.
    pub safety_identity: String,
    /// What the evaluator reads about the helper's coverage and the target.
    pub containment: ContainmentInput,
    /// The arguments the ceiling reads: the step's own plus both prefix floors.
    pub args: BTreeMap<String, String>,
    /// The TTL the block asks for.
    pub ttl: SignedDuration,
}

/// The two prefix lengths a ceiling bounds: the target's own for its family, and the widest
/// value for the other family so that floor is always met.
#[must_use]
pub fn prefix_args(net: &Net) -> [(String, String); 2] {
    let (v4, v6) = match net.family() {
        Family::V4 => (net.prefix_len(), 128),
        Family::V6 => (32, net.prefix_len()),
    };
    [
        (keys::ARG_PREFIX_LEN_V4.to_owned(), v4.to_string()),
        (keys::ARG_PREFIX_LEN_V6.to_owned(), v6.to_string()),
    ]
}

/// Reads the facts a permit for `row` is issued against, under policy authority.
///
/// # Errors
/// [`BlockFactsError::Unverified`] when the helper is not verified, fresh and covering every
/// discovered path; [`BlockFactsError::BadRequest`] for arguments that are not a block;
/// [`BlockFactsError::Unreadable`] when the host, trust state or paths could not be read.
pub async fn block_facts(
    driver: &ContainmentDriver,
    row: &ExecutionRecord,
    now: Timestamp,
) -> Result<BlockFacts, BlockFactsError> {
    let (net, ttl) = parse_request(&row.args).map_err(BlockFactsError::BadRequest)?;
    let deps = driver.deps();
    let resolved = deps
        .resolver
        .resolve(TargetQuery::NetworkSource {
            host: row.host,
            source: net,
        })
        .await
        .map_err(|err| BlockFactsError::Unreadable(err.to_string()))?;
    let assessment = assess(&*deps.trust, &*deps.paths, row.host, Authority::Policy, now)
        .await
        .map_err(|err| match err {
            AssessError::PolicyUnverified(reason) => BlockFactsError::Unverified(reason),
            AssessError::Unreadable(text) => BlockFactsError::Unreadable(text),
        })?;
    let domovoi_core::action::PlanAssurance::Verified(identity) = &assessment.assurance else {
        // `assess` under policy authority answers `Verified` or an error; anything else is not
        // a basis for a permit.
        return Err(BlockFactsError::Unverified(
            assessment
                .reason()
                .unwrap_or(UnverifiedReason::AttestationMissing),
        ));
    };
    let plan = ContainmentPlan::new(
        &net,
        ttl,
        row.host,
        &resolved.host_key,
        &assessment.assurance,
    )
    .hash();
    let view = deps
        .trust
        .view(row.host)
        .await
        .map_err(|err| BlockFactsError::Unreadable(err.to_string()))?;
    let discovered = deps
        .paths
        .discovered(row.host)
        .await
        .map_err(BlockFactsError::Unreadable)?;
    let containment = containment_input(now, &view, &net, &discovered, false);
    let mut args = row.args.clone();
    args.extend(prefix_args(&net));
    Ok(BlockFacts {
        plan,
        safety_identity: identity.canonical(),
        containment,
        args,
        ttl,
    })
}
