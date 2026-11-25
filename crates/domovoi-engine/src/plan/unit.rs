//! The checks of a `unit.restart` step: the name, the helper's identity, the pinned subject,
//! availability, the budget and the history. A refusal names its token and sentence so the model
//! can say why.

use domovoi_core::capability::{ContractVersion, TargetName};
use domovoi_core::meta::{PlanInput, PlanStepInput};
use domovoi_core::unit::{IdentityHash, UnitCard, UnitId, UnitName};
use domovoi_core::{ContractId, HostId, PinnedSubject};

use super::refusal::PlanRefusalCode as Code;
use super::resolve::map_error;
use super::scope::PlanScope;
use super::validate::{
    Fail, PlanValidator, PlannedStep, REASON_MAX, RunKey, check_history, fail, plain,
};

/// What a `unit.restart` step is bound to: the canonical id, the configuration hash and the card
/// the approval shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitBinding {
    /// The canonical `Id`, never an alias.
    pub id: UnitId,
    /// The hash of the configuration and files the restart is bound to.
    pub identity_hash: IdentityHash,
    /// The card the approval shows.
    pub card: UnitCard,
}

impl PlanValidator<'_> {
    /// The checks of a `unit.restart` step: the name, the helper's identity, the pinned subject,
    /// availability, the budget and the history. A refusal names its token and sentence so the
    /// model can say why.
    pub(super) async fn check_unit(
        &self,
        scope: &PlanScope,
        staged: &[PlannedStep],
        accepted: &[PlannedStep],
        step: &PlanStepInput,
        (host, label): (HostId, &str),
        id: &ContractId,
    ) -> Result<PlannedStep, Fail> {
        let args = PlanInput::canonical_args(step);
        let typed = match (args.len(), args.get("unit")) {
            (1, Some(typed)) => typed.clone(),
            _ => {
                return Err(fail(
                    Code::BadArguments,
                    "bad arguments: unit.restart takes exactly one argument, `unit`",
                ));
            }
        };
        UnitName::parse(&typed).map_err(|error| {
            fail(
                Code::TargetInvalid,
                format!("bad_unit_name: {}", error.reason),
            )
        })?;
        let name = TargetName::parse(&typed).map_err(|error| {
            fail(
                Code::TargetInvalid,
                format!("bad_unit_name: {}", error.reason),
            )
        })?;
        let unit = self
            .resolver
            .resolve_unit(host, &name)
            .await
            .map_err(|error| plain(map_error(&error, &typed)))?;
        let subject = pick_unit_subject(scope, host, step, &typed, unit.id.as_str())?;
        let note = self.availability(host, id).await?;
        if let Some(left) = scope.executions_left
            && staged.len() + accepted.len() >= left as usize
        {
            return Err(fail(
                Code::BudgetSpent,
                "this operation has no execution budget left",
            ));
        }
        let key = RunKey {
            contract: id,
            host,
            check: &subject.check,
            target: subject.target.as_ref(),
            args: &args,
        };
        let retry_of = check_history(scope, staged, accepted, &key, step)?;
        let _ = label;
        Ok(PlannedStep {
            input: step.clone(),
            version: ContractVersion::new(1).map_err(|e| fail(Code::UnknownContract, e.reason))?,
            contract: id.clone(),
            host,
            check: subject.check.clone(),
            target: subject.target.clone(),
            args,
            reason: step.reason.chars().take(REASON_MAX).collect::<String>(),
            identity: unit.target.identity.clone(),
            retry_of,
            note,
            unit: Some(UnitBinding {
                id: unit.id,
                identity_hash: unit.identity_hash,
                card: unit.card,
            }),
        })
    }
}

/// Check 6 for a unit: the pinned subject is the one whose target is the typed name or the
/// canonical id the helper resolved it to, so an alias typed by the model still finds its unit.
fn pick_unit_subject<'a>(
    scope: &'a PlanScope,
    host: HostId,
    step: &PlanStepInput,
    typed: &str,
    canonical: &str,
) -> Result<&'a PinnedSubject, Fail> {
    if let Some(given) = &step.target
        && given != typed
        && given != canonical
    {
        return Err(fail(
            Code::TargetMismatch,
            format!("target `{given}` does not match the unit `{typed}`"),
        ));
    }
    scope
        .pinned
        .iter()
        .filter(|subject| subject.host == host)
        .find(|subject| {
            subject
                .target
                .as_ref()
                .is_some_and(|key| key.as_str() == typed || key.as_str() == canonical)
        })
        .ok_or_else(|| {
            fail(
                Code::TargetMismatch,
                format!("`{canonical}` is not a target of this investigation"),
            )
        })
}
