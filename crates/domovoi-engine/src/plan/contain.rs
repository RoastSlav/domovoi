//! The checks of a `firewall.block_source` step: the source, the TTL, the owner's pinned subject,
//! availability, the budget and the history. A refusal names its token and sentence so the model
//! can say why.
//!
//! A block can only be proposed for an `ipnet:` subject the owner pinned on the operation. The
//! approval, the helper's safety checks and the helper's own exclusions come after this.

use domovoi_core::capability::{ContractVersion, TargetQuery};
use domovoi_core::containment::{Net, keys};
use domovoi_core::meta::{PlanInput, PlanStepInput};
use domovoi_core::{ContractId, HostId, PinnedSubject};

use super::refusal::PlanRefusalCode as Code;
use super::resolve::map_error;
use super::scope::PlanScope;
use super::validate::{
    Fail, PlanValidator, PlannedStep, REASON_MAX, RunKey, check_history, fail, plain,
};
use crate::containment::STEP_ONE_SECS;
use crate::containment::input::MAX_TTL_S;

impl PlanValidator<'_> {
    /// The checks of a `firewall.block_source` step.
    pub(super) async fn check_contain(
        &self,
        scope: &PlanScope,
        staged: &[PlannedStep],
        accepted: &[PlannedStep],
        step: &PlanStepInput,
        host: HostId,
        id: &ContractId,
    ) -> Result<PlannedStep, Fail> {
        let args = PlanInput::canonical_args(step);
        let (source, ttl) = match (
            args.len(),
            args.get(keys::ARG_SOURCE),
            args.get(keys::ARG_TTL_S),
        ) {
            (2, Some(source), Some(ttl)) => (source.clone(), ttl.clone()),
            _ => {
                return Err(fail(
                    Code::BadArguments,
                    "bad arguments: firewall.block_source takes exactly `source` and `ttl_s`",
                ));
            }
        };
        let net = Net::parse(&source)
            .map_err(|error| fail(Code::TargetInvalid, format!("invalid_target: {error}")))?;
        if net.canonical() != source {
            return Err(fail(
                Code::TargetInvalid,
                format!("invalid_target: write the source as `{}`", net.canonical()),
            ));
        }
        match ttl.parse::<i64>() {
            Ok(secs) if secs > STEP_ONE_SECS && secs <= MAX_TTL_S => {}
            Ok(secs) if secs <= STEP_ONE_SECS => {
                return Err(fail(
                    Code::BadArguments,
                    format!("ttl_too_short: ttl_s must be more than {STEP_ONE_SECS}"),
                ));
            }
            _ => {
                return Err(fail(
                    Code::BadArguments,
                    format!("ttl_above_maximum: ttl_s must be at most {MAX_TTL_S}"),
                ));
            }
        }
        let resolved = self
            .resolver
            .resolve(TargetQuery::NetworkSource { host, source: net })
            .await
            .map_err(|error| plain(map_error(&error, &source)))?;
        let subject = pick_network_subject(scope, host, step, &net)?;
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
        Ok(PlannedStep {
            input: step.clone(),
            version: ContractVersion::new(1).map_err(|e| fail(Code::UnknownContract, e.reason))?,
            contract: id.clone(),
            host,
            check: subject.check.clone(),
            target: subject.target.clone(),
            args,
            reason: step.reason.chars().take(REASON_MAX).collect::<String>(),
            identity: resolved.identity,
            retry_of,
            note,
            unit: None,
        })
    }
}

/// The pinned subject whose target is `ipnet:` plus the canonical source: the owner's choice of
/// what may be blocked on this operation.
fn pick_network_subject<'a>(
    scope: &'a PlanScope,
    host: HostId,
    step: &PlanStepInput,
    net: &Net,
) -> Result<&'a PinnedSubject, Fail> {
    let wanted = format!("ipnet:{}", net.canonical());
    if let Some(given) = &step.target
        && *given != wanted
        && *given != net.canonical()
    {
        return Err(fail(
            Code::TargetMismatch,
            format!(
                "target `{given}` does not match the source `{}`",
                net.canonical()
            ),
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
                .is_some_and(|key| key.as_str() == wanted)
        })
        .ok_or_else(|| {
            fail(
                Code::TargetMismatch,
                format!("`{wanted}` is not a target the owner pinned on this investigation"),
            )
        })
}
