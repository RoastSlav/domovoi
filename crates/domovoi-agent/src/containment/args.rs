//! The six arguments of `firewall.block_source`, parsed and checked against each other before
//! anything is journaled.

use domovoi_core::containment::{Family, Net, keys};
use domovoi_proto::messages::RunCapability;
use jiff::SignedDuration;
use uuid::Uuid;

use crate::unattended::FirewallAsk;

/// Which step of a block a request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Step one: the element goes in with the 30 second kernel timeout.
    Apply,
    /// Step two: the hub confirms and the element gets its full lifetime.
    Confirm,
    /// Removal, by the driver or by the owner.
    Rollback,
}

/// A parsed `firewall.block_source` request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockArgs {
    /// The step.
    pub phase: Phase,
    /// The network, re-parsed here; the text on the wire equals its canonical form.
    pub net: Net,
    /// The lifetime the block has after `confirm`, in seconds.
    pub ttl_s: u32,
    /// The execution id of the block's apply.
    pub block: Uuid,
}

/// The wire target of a network: `ipnet:` and the canonical text.
#[must_use]
pub fn wire_target(net: &Net) -> String {
    format!("ipnet:{}", net.canonical())
}

/// The prefix lengths a request must carry for `net`: its own in its family, the family's full
/// length in the other, so a floor on the other family is always met.
#[must_use]
pub fn expected_prefix_lens(net: &Net) -> (u8, u8) {
    match net.family() {
        Family::V4 => (net.prefix_len(), Family::V6.bits()),
        Family::V6 => (Family::V4.bits(), net.prefix_len()),
    }
}

/// What the ceiling is asked about a request, read leniently: an argument that does not parse
/// becomes a value the ceiling refuses, and the strict parse reports it later.
#[must_use]
pub fn ceiling_ask(run: &RunCapability) -> FirewallAsk {
    let int = |key: &str| run.args.get(key).and_then(|text| text.parse::<i64>().ok());
    FirewallAsk {
        ttl: int(keys::ARG_TTL_S).map_or(SignedDuration::MAX, SignedDuration::from_secs),
        prefix_len_v4: int(keys::ARG_PREFIX_LEN_V4),
        prefix_len_v6: int(keys::ARG_PREFIX_LEN_V6),
    }
}

/// The request's arguments, or a sentence for `bad_request`.
pub fn parse(run: &RunCapability) -> Result<BlockArgs, String> {
    let args = &run.args;
    if args.len() != keys::ARG_KEYS.len() || !keys::ARG_KEYS.iter().all(|k| args.contains_key(*k)) {
        return Err(
            "firewall.block_source takes exactly phase, source, ttl_s, block, prefix_len_v4 and prefix_len_v6"
                .to_owned(),
        );
    }
    let phase = match args[keys::ARG_PHASE].as_str() {
        keys::PHASE_APPLY => Phase::Apply,
        keys::PHASE_CONFIRM => Phase::Confirm,
        keys::PHASE_ROLLBACK => Phase::Rollback,
        _ => return Err("phase must be apply, confirm or rollback".to_owned()),
    };
    let text = &args[keys::ARG_SOURCE];
    let net = Net::parse(text).map_err(|error| format!("invalid_target: {error}"))?;
    if net.canonical() != *text {
        return Err("invalid_target: the source must be the canonical network text".to_owned());
    }
    if run.target.as_str() != wire_target(&net) {
        return Err(
            "invalid_target: the target must be ipnet: and the canonical source".to_owned(),
        );
    }
    let (v4, v6) = expected_prefix_lens(&net);
    for (key, want) in [(keys::ARG_PREFIX_LEN_V4, v4), (keys::ARG_PREFIX_LEN_V6, v6)] {
        if args[key] != want.to_string() {
            return Err(format!("{key} must be {want} for this source"));
        }
    }
    let ttl_s: u32 = args[keys::ARG_TTL_S]
        .parse()
        .map_err(|_| "ttl_s must be a whole number of seconds".to_owned())?;
    if ttl_s == 0 {
        return Err("ttl_s must be at least 1".to_owned());
    }
    let block: Uuid = args[keys::ARG_BLOCK]
        .parse()
        .map_err(|_| "block must be a UUID".to_owned())?;
    if phase == Phase::Apply && block != run.execution_id {
        return Err("an apply names itself as the block".to_owned());
    }
    Ok(BlockArgs {
        phase,
        net,
        ttl_s,
        block,
    })
}
