//! Turning the report's action proposals into `Proposed` actions. A proposal is only data:
//! it names a skill from the host's catalogue and arguments that must pass the manifest's
//! parameter specs. Nothing else in the report is ever acted on (INV-9).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use domovoi_core::ports::{ActionFilter, AgentSnapshot, Page};
use domovoi_core::{
    ActionError, ActionId, ActionProposal, ActionRequest, ActionStatus, HostId, Incident,
    KeyFingerprint, PlanFamily, PlanTarget, Principal, SkillSummary, TriageReport, ValidatedArgs,
};
use jiff::Timestamp;
use sha2::{Digest, Sha256};

use crate::ApprovalError;

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

/// Whether the host's current session is still the one this report was built from.
fn session_current(shared: &Shared, host: HostId, snapshot: &AgentSnapshot) -> bool {
    shared.deps.link.snapshot(host).is_some_and(|current| {
        current.session == snapshot.session && current.agent == snapshot.agent
    })
}

/// Whether `existing` is exactly the plan this proposal would create, execution id included.
///
/// The hash comparison covers the contract, implementation (manifest and entry digest),
/// target, canonical arguments and the action's own execution id; risk is not a v2 input, so
/// it is compared separately.
fn same_plan(
    existing: &ActionRequest,
    skill: &SkillSummary,
    args: &ValidatedArgs,
    wanted: &BTreeMap<String, String>,
    target: &PlanTarget,
    now: Timestamp,
) -> bool {
    existing.status() == &ActionStatus::Proposed
        && existing.expires_at() > now
        && existing.skill() == &skill.name
        && existing.args() == wanted
        && existing.risk() == skill.risk
        && existing.approval_plan().family() == PlanFamily::V2
        && ActionRequest::v2_hash_for(existing.id(), skill, args, target)
            .is_ok_and(|expected| existing.plan_hash_v2() == Some(expected))
}

/// Stores a `Proposed` action for each of `proposals` that names a skill on this host,
/// fits that skill's manifest and is not already waiting for a human with an open window,
/// including one this very batch stored. One whose identical, unexpired action is already
/// waiting gets that action's chat token rotated instead of a second action: the replacement
/// is returned keyed to the waiting action, and the token the previous card carried stops
/// working. If the waiting action's window closes before the rotation, a fresh proposal is
/// stored instead. Returns the
/// plain chat token of each stored or rotated action keyed by its id, the proposals that
/// were stored in the order given, and why each of the others was dropped, one line each
/// and already redacted.
pub(crate) async fn store(
    shared: &Shared,
    incident: &Incident,
    snapshot: Option<&AgentSnapshot>,
    skills: &[SkillSummary],
    proposals: Vec<ActionProposal>,
) -> (BTreeMap<ActionId, String>, Vec<ActionProposal>, Vec<String>) {
    let now = shared.deps.clock.now();
    let mut tokens = BTreeMap::new();
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    let mut existing = match shared
        .deps
        .actions
        .list(
            &ActionFilter {
                incident: Some(incident.id()),
                status: Some("proposed".to_owned()),
                ..ActionFilter::default()
            },
            &Page::first(50),
        )
        .await
    {
        Ok(page) => page.items,
        Err(err) => {
            tracing::warn!(%err, "could not list proposed actions");
            Vec::new()
        }
    };
    for proposal in proposals {
        let (skill, args) = match check(&proposal, incident, skills) {
            Ok(found) => found,
            Err(why) => {
                // The reason can quote an argument value.
                let line = format!("{}: {why}", proposal.skill);
                dropped.push(shared.deps.redactor.redact(&line));
                continue;
            }
        };
        // Resolve the one session and target this proposal may bind to before deciding
        // whether an old proposal is the same plan. A stale action must not be reused for a
        // different implementation or target, and a replaced session must not rotate it.
        let Some(snapshot) = snapshot else {
            dropped.push(format!(
                "{}: no authenticated agent session to bind the action to",
                proposal.skill
            ));
            continue;
        };
        if !session_current(shared, incident.host(), snapshot) {
            dropped.push(format!(
                "{}: the agent session changed while triage ran",
                proposal.skill
            ));
            continue;
        }
        let (target, host_key) = match shared.deps.hosts.live_agent(incident.host()).await {
            Ok(Some(agent)) if agent.agent == snapshot.agent => {
                let host_key = KeyFingerprint::of_public_key(&agent.public_key);
                (PlanTarget::of_incident(incident, host_key), host_key)
            }
            Ok(Some(_)) => {
                dropped.push(format!(
                    "{}: the host's enrolled agent changed while triage ran",
                    proposal.skill
                ));
                continue;
            }
            Ok(None) => {
                dropped.push(format!(
                    "{}: the host has no enrolled agent key",
                    proposal.skill
                ));
                continue;
            }
            Err(err) => {
                tracing::warn!(%err, "could not read the host's enrolled agent");
                dropped.push(format!(
                    "{}: the host's enrolled agent could not be read",
                    proposal.skill
                ));
                continue;
            }
        };
        let wanted = args.to_map();
        let waiting = existing
            .iter()
            .position(|action| same_plan(action, skill, &args, &wanted, &target, now));
        if let Some(i) = waiting {
            // An identical action is already waiting for a human; do not store a second one.
            // The card this run publishes is built from this batch's tokens, so without one
            // for the waiting action its approve buttons would fall back to "Open in Web":
            // rotate the waiting action's chat token and hand over the replacement instead.
            let token = mint_token();
            let id = existing[i].id();
            // The session must still be the one this plan was built from, checked as close
            // as practical to the rotation. A stale action keeps its old token.
            if !session_current(shared, incident.host(), snapshot) {
                dropped.push(format!(
                    "{}: the agent session changed while triage ran",
                    proposal.skill
                ));
                continue;
            }
            // Refused if the action was decided or deleted meanwhile; then the old token
            // still stands and nothing is offered for it, as before.
            match shared
                .deps
                .actions_service
                .rotate_chat_token(id, token_hash(&token))
                .await
            {
                Ok(rotated) => {
                    tokens.insert(rotated.id(), token);
                    existing[i] = rotated;
                    continue;
                }
                // The window closed between listing and rotation. Drop it from this batch so
                // a repeated proposal does not try it again, and fall through to store a
                // fresh one; the expired action is left for the sweep to settle.
                Err(ApprovalError::Refused(ActionError::Expired)) => {
                    existing.remove(i);
                }
                Err(err) => {
                    tracing::warn!(
                        %err,
                        "could not rotate the chat token of a waiting action"
                    );
                    continue;
                }
            }
        }
        let token = mint_token();
        // Recheck as close as practical to persisting the proposal; dispatch remains the
        // authority on a session that changes after this.
        if !session_current(shared, incident.host(), snapshot) {
            dropped.push(format!(
                "{}: the agent session changed while triage ran",
                proposal.skill
            ));
            continue;
        }
        let action = match ActionRequest::propose(
            incident,
            skill,
            &args,
            Some(host_key),
            Principal::System(PROPOSER.into()),
            PROPOSAL_TTL,
            now,
        ) {
            Ok(action) => action.with_chat_token_hash(token_hash(&token)),
            Err(err) => {
                dropped.push(format!("{}: {err}", proposal.skill));
                continue;
            }
        };
        match shared.deps.actions_service.propose(action).await {
            Ok(stored) => {
                tokens.insert(stored.id(), token);
                kept.push(proposal);
                // It is on record now: an identical action again in this batch must not
                // become a second one.
                existing.push(stored);
            }
            Err(err) => {
                tracing::warn!(%err, "could not store a proposed action");
                dropped.push(format!("{}: could not be stored", proposal.skill));
            }
        }
    }
    (tokens, kept, dropped)
}

/// Stores the report's proposals as `Proposed` actions and keeps in the report only the
/// ones that were stored, so the card and the thread never name a skill that is not on
/// record. Why the others were dropped is appended to its details.
pub(crate) async fn create(
    shared: &Shared,
    incident: &Incident,
    snapshot: Option<&AgentSnapshot>,
    skills: &[SkillSummary],
    report: &mut TriageReport,
) -> BTreeMap<ActionId, String> {
    let (tokens, kept, dropped) = store(
        shared,
        incident,
        snapshot,
        skills,
        std::mem::take(&mut report.proposals),
    )
    .await;
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
