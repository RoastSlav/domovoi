//! Whether a finished run fixed what raised the incident: how long to wait before the
//! originating check is re-run. The re-check itself is a durable `wake` effect, so a hub
//! restart between the run and the re-check does not skip it.

use domovoi_core::{ActionRequest, CheckKey, Risk, SkillSummary};
use jiff::SignedDuration;

use super::Shared;

/// How long a verification re-check may take.
pub const RECHECK_DEADLINE: SignedDuration = SignedDuration::from_secs(60);

/// How long to wait before re-checking, or `None` when the action is not verified. A
/// skill that says how to verify itself wins; otherwise a write skill that applies to
/// the check that raised the incident waits the service's default.
fn verify_delay(shared: &Shared, skill: &SkillSummary, check: &CheckKey) -> Option<SignedDuration> {
    match &skill.verify {
        Some(spec) if spec.recheck => Some(spec.delay),
        Some(_) => None,
        None => (skill.risk == Risk::Write && skill.applies_to.contains(check))
            .then_some(shared.settings.verify_default_delay),
    }
}

/// The delay the finished `action` owes its re-check, if any: the skill's own `verify`, or
/// the service's default for a write skill that applies to the incident's check. A store
/// that cannot say leaves the action unverified rather than guessing.
pub(crate) async fn verify_after(
    shared: &Shared,
    action: &ActionRequest,
    skill: &SkillSummary,
) -> Option<SignedDuration> {
    let incident = shared.deps.incidents.get(action.incident()).await.ok()??;
    verify_delay(shared, skill, incident.check())
}
