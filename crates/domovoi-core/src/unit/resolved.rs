//! What the hub learns from `unit.identity`: the resolved unit, and the card built from the
//! helper's string outputs. Pure parsing; nothing here reads a host.

use serde::Deserialize;

use super::call::UnitOutputs;
use super::card::{CardFacts, render_facts};
use super::keys;
use super::name::{IdentityHash, UnitId};
use super::refusal::UnitRefusal;
use crate::capability::ResolvedTarget;

/// The card of a unit as the hub shows it: the facts, plus where the platform stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitCard {
    /// What the card says about the unit.
    pub facts: CardFacts,
    /// The unit's `ActiveState` when it was read.
    pub active_state: String,
    /// The host's platform tier as the helper reported it, such as `untested`.
    pub platform_tier: String,
    /// Whether the owner switched write groups on for this host.
    pub write_opt_in: bool,
}

impl UnitCard {
    /// The approval card text for this unit on `host`.
    #[must_use]
    pub fn render(&self, host: &str) -> String {
        render_facts(host, &self.facts, self.platform_tier == "untested")
    }
}

/// A unit the helper resolved: the canonical identity to pin, the hash to bind, and the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedUnit {
    /// The canonical target identity (stable across configuration changes).
    pub target: ResolvedTarget,
    /// The canonical `Id`, never an alias.
    pub id: UnitId,
    /// The hash of the configuration and files a restart is bound to.
    pub identity_hash: IdentityHash,
    /// The card the approval shows.
    pub card: UnitCard,
}

#[derive(Deserialize)]
struct Pin {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Loaded {
    exec_start: Vec<String>,
    user: String,
    working_directory: String,
    followers: Vec<String>,
}

/// The id and hash a `unit.identity` answer names.
///
/// # Errors
/// `read_failed` when either is missing or malformed: nothing is known, so nothing is bound.
pub fn id_and_hash(outputs: &UnitOutputs) -> Result<(UnitId, IdentityHash), UnitRefusal> {
    let id = outputs
        .get(keys::OUT_ID)
        .and_then(|text| UnitId::parse(text).ok())
        .ok_or(UnitRefusal::ReadFailed)?;
    let hash = outputs
        .get(keys::OUT_IDENTITY_HASH)
        .filter(|text| text.len() == 64)
        .and_then(|text| text.parse().ok())
        .ok_or(UnitRefusal::ReadFailed)?;
    Ok((id, hash))
}

/// The card a `unit.identity` answer describes.
///
/// # Errors
/// `read_failed` for any field that is missing or does not parse.
pub fn card_of(outputs: &UnitOutputs) -> Result<UnitCard, UnitRefusal> {
    let (id, _) = id_and_hash(outputs)?;
    let bad = || UnitRefusal::ReadFailed;
    let fragment: Pin = serde_json::from_str(outputs.get(keys::OUT_FRAGMENT).ok_or_else(bad)?)
        .map_err(|_| bad())?;
    let drop_ins: Vec<Pin> = serde_json::from_str(outputs.get(keys::OUT_DROP_INS).ok_or_else(bad)?)
        .map_err(|_| bad())?;
    let loaded: Loaded =
        serde_json::from_str(outputs.get(keys::OUT_LOADED).ok_or_else(bad)?).map_err(|_| bad())?;
    Ok(UnitCard {
        facts: CardFacts {
            id: id.to_string(),
            fragment_path: fragment.path,
            fragment_sha256: fragment.sha256,
            drop_ins: drop_ins.into_iter().map(|p| (p.path, p.sha256)).collect(),
            exec_start: loaded.exec_start.join(" "),
            user: loaded.user,
            working_directory: loaded.working_directory,
            followers: loaded.followers,
        },
        active_state: outputs
            .get(keys::OUT_ACTIVE_STATE)
            .unwrap_or_default()
            .to_owned(),
        platform_tier: outputs
            .get(keys::OUT_PLATFORM_TIER)
            .unwrap_or("blocked")
            .to_owned(),
        write_opt_in: outputs.get(keys::OUT_WRITE_OPT_IN) == Some("true"),
    })
}
