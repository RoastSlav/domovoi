//! Turning the report's action proposals into `Proposed` actions. A proposal is only data:
//! it names a skill from the host's catalogue and arguments that must pass the manifest's
//! parameter specs. Nothing else in the report is ever acted on (INV-9).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use domovoi_core::{
    ActionId, ActionProposal, ActionRequest, Incident, Principal, SkillSummary, TriageReport,
    ValidatedArgs,
};
use sha2::{Digest, Sha256};

use super::{PROPOSAL_TTL, Shared};

/// Who proposed actions that triage created.
pub const PROPOSER: &str = "triage";

/// A fresh single-use chat token: 244 random bits, hex-encoded.
#[must_use]
pub fn mint_token() -> String {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    hex::encode(bytes)
}

/// The hash stored for `token`; the token itself is never stored.
#[must_use]
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// The skill `proposal` names and its validated arguments, or why it cannot become an
/// action on a host with `skills`.
fn check<'a>(
    proposal: &ActionProposal,
    incident: &Incident,
    skills: &'a [SkillSummary],
) -> Result<(&'a SkillSummary, ValidatedArgs), String> {
    let skill = skills
        .iter()
        .find(|s| s.name == proposal.skill)
        .ok_or_else(|| "not a skill on this host".to_owned())?;
    if !skill.applies_to.is_empty() && !skill.applies_to.contains(incident.check()) {
        return Err(format!("does not apply to {}", incident.check()));
    }
    let args = skill
        .validate_args(&proposal.args)
        .map_err(|e| format!("bad arguments: {e}"))?;
    Ok((skill, args))
}

/// Stores a `Proposed` action for each valid proposal in `report` and returns their plain
/// chat tokens. Invalid proposals are removed from the report and listed in its details.
pub(crate) async fn create(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
    report: &mut TriageReport,
) -> BTreeMap<ActionId, String> {
    let now = shared.deps.clock.now();
    let mut tokens = BTreeMap::new();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for proposal in std::mem::take(&mut report.proposals) {
        let (skill, args) = match check(&proposal, incident, skills) {
            Ok(found) => found,
            Err(why) => {
                dropped.push(format!("{}: {why}", proposal.skill));
                continue;
            }
        };
        let token = mint_token();
        let action = ActionRequest::propose(
            incident.id(),
            incident.host(),
            skill,
            &args,
            Principal::System(PROPOSER.into()),
            PROPOSAL_TTL,
            now,
        )
        .with_chat_token_hash(token_hash(&token));
        match shared.deps.actions.insert(&action).await {
            Ok(()) => {
                let entry = crate::actions::audit::proposed(&action, now);
                if let Err(err) = shared.deps.audit.append(&entry).await {
                    tracing::error!(%err, "could not write the audit log");
                }
                tokens.insert(action.id(), token);
                kept.push(proposal);
            }
            Err(err) => {
                tracing::warn!(%err, "could not store a proposed action");
                dropped.push(format!("{}: could not be stored", proposal.skill));
            }
        }
    }
    report.proposals = kept;
    if !dropped.is_empty() {
        let mut details = std::mem::take(&mut report.details);
        if !details.trim().is_empty() {
            details.push_str("\n\n");
        }
        details.push_str("Proposals not offered:");
        for line in dropped {
            let _ = write!(details, "\n- {line}");
        }
        report.details = details;
    }
    tokens
}
