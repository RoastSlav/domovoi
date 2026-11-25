//! The thirteen checks that decide whether a suggested step may be staged.

use std::collections::BTreeMap;
use std::str::FromStr;

use domovoi_core::capability::{
    ContractVersion, EXECUTABLE_BUILTINS, TargetIdentity, skill_input_schema,
};
use domovoi_core::meta::{PlanInput, PlanStepInput};
use domovoi_core::ports::{CapabilityRegistry, HostRepo, Redactor, TargetResolver};
use domovoi_core::triage::MAX_PROPOSALS;
use domovoi_core::{
    Availability, CheckKey, ContractId, EffectClass, HostId, HostName, PinnedSubject, RetryOf,
    SkillSummary, StepId, StepOutcome, TargetKey,
};

use super::refusal::{PlanRefusal, PlanRefusalCode as Code};
use super::resolve::{Refuse, resolve_host};
use super::schema::check_schema;
use super::scope::{PlanScope, PriorRun};
use super::unit::UnitBinding;

/// Longest reason kept on a planned step.
pub(super) const REASON_MAX: usize = 200;
/// The refusal a check returns: code, message and an optional hint.
pub(super) type Fail = (Code, String, Option<String>);

/// Whether a plan may name this capability today: the `skill.` family, and the built-ins on the
/// closed `EXECUTABLE_BUILTINS` list (`unit.restart`, `firewall.block_source`). `container.restart` is on neither.
#[must_use]
pub fn is_executable(id: &ContractId) -> bool {
    id.is_skill() || EXECUTABLE_BUILTINS.contains(&id.as_str())
}

/// A step that passed every check; what the store-time validation re-checks and stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStep {
    /// The step as the model wrote it, kept for the second validation.
    pub input: PlanStepInput,
    /// The capability.
    pub contract: ContractId,
    /// The contract version the plan was made against.
    pub version: ContractVersion,
    /// The host.
    pub host: HostId,
    /// The check of the pinned subject.
    pub check: CheckKey,
    /// The target of the pinned subject.
    pub target: Option<TargetKey>,
    /// Canonical string arguments.
    pub args: BTreeMap<String, String>,
    /// Why, redacted, at most 200 characters.
    pub reason: String,
    /// The host identity the resolver proved (display only; the writer recomputes the bound hash).
    pub identity: TargetIdentity,
    /// The earlier failed step this one repeats, with what changed.
    pub retry_of: Option<RetryOf>,
    /// An availability sentence worth telling the model, when there is one.
    pub note: Option<String>,
    /// What a `unit.restart` step is bound to; `None` for a skill.
    pub unit: Option<UnitBinding>,
}

/// The result of validating one call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanVerdict {
    /// Steps that passed.
    pub accepted: Vec<PlannedStep>,
    /// Steps that did not, with why.
    pub refused: Vec<PlanRefusal>,
}

/// Validates the steps a model suggests. Never writes and never panics.
pub struct PlanValidator<'a> {
    /// What the hub offers where.
    pub registry: &'a dyn CapabilityRegistry,
    /// Proves host identities.
    pub resolver: &'a dyn TargetResolver,
    /// Host lookup by name.
    pub hosts: &'a dyn HostRepo,
    /// Removes secrets from every text that leaves.
    pub redactor: &'a dyn Redactor,
}

enum Capability {
    Skill(Box<SkillSummary>),
    Builtin,
}

impl PlanValidator<'_> {
    /// Validates every step of `input` against `scope`, given the steps already staged in this
    /// turn. Steps are checked in order; an accepted step counts for the next one.
    pub async fn validate(
        &self,
        scope: &PlanScope,
        staged: &[PlannedStep],
        input: &PlanInput,
    ) -> PlanVerdict {
        let mut verdict = PlanVerdict::default();
        for (index, step) in input.steps.iter().enumerate() {
            match self
                .check_step(scope, staged, &verdict.accepted, step)
                .await
            {
                Ok(planned) => verdict.accepted.push(planned),
                Err((code, message, hint)) => verdict.refused.push(PlanRefusal {
                    index,
                    contract: self.redactor.redact(&step.contract),
                    code,
                    message: self.redactor.redact(&message),
                    hint: hint.map(|hint| self.redactor.redact(&hint)),
                }),
            }
        }
        verdict
    }

    async fn check_step(
        &self,
        scope: &PlanScope,
        staged: &[PlannedStep],
        accepted: &[PlannedStep],
        step: &PlanStepInput,
    ) -> Result<PlannedStep, Fail> {
        if staged.len() + accepted.len() >= MAX_PROPOSALS {
            return Err(fail(
                Code::TooManySteps,
                "at most 3 steps per investigation; this one was not taken",
            ));
        }
        let (host, label) = self.pick_host(scope, step).await?;
        let (id, capability) = self.find_capability(host, &step.contract)?;
        let effect = match &capability {
            Capability::Skill(skill) => self.registry.effective_effect(host, &skill.name),
            Capability::Builtin => self.registry.contracts().get(&id).map(|c| c.effect),
        }
        .ok_or_else(|| unknown_contract(&step.contract, &id))?;
        if effect == EffectClass::Read {
            return Err(fail(
                Code::NotAPlanStep,
                format!("`{id}` only reads; get it with request_evidence"),
            ));
        }
        if !is_executable(&id) {
            return Err(fail(
                Code::NotExecutable,
                format!("`{id}` cannot be run from a plan yet"),
            ));
        }
        let skill = match capability {
            Capability::Skill(skill) => skill,
            Capability::Builtin if id.as_str() == "unit.restart" => {
                return self
                    .check_unit(scope, staged, accepted, step, (host, &label), &id)
                    .await;
            }
            Capability::Builtin if id.as_str() == "firewall.block_source" => {
                return self
                    .check_contain(scope, staged, accepted, step, host, &id)
                    .await;
            }
            Capability::Builtin => {
                return Err(fail(
                    Code::NotExecutable,
                    format!("`{id}` cannot be run from a plan yet"),
                ));
            }
        };
        let subject = pick_subject(scope, host, step)?;
        let resolved = resolve_host(self.resolver, host, &label)
            .await
            .map_err(plain)?;
        if !skill.applies_to.is_empty() && !skill.applies_to.contains(&subject.check) {
            return Err(fail(
                Code::NotApplicable,
                format!("does not apply to {}", subject.check),
            ));
        }
        let args = PlanInput::canonical_args(step);
        check_args(&skill, &args)?;
        let note = self.availability(host, &id).await?;
        if let Some(left) = scope.executions_left
            && staged.len() + accepted.len() >= left as usize
        {
            return Err(fail(
                Code::BudgetSpent,
                "this operation has no execution budget left",
            ));
        }
        let key = RunKey {
            contract: &id,
            host,
            check: &subject.check,
            target: subject.target.as_ref(),
            args: &args,
        };
        let retry_of = check_history(scope, staged, accepted, &key, step)?;
        Ok(PlannedStep {
            input: step.clone(),
            version: ContractVersion::new(1).map_err(|e| fail(Code::UnknownContract, e.reason))?,
            contract: id,
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

    /// Check 2: the host.
    async fn pick_host(
        &self,
        scope: &PlanScope,
        step: &PlanStepInput,
    ) -> Result<(HostId, String), Fail> {
        let pinned_hosts = || scope.pinned.iter().map(|subject| subject.host);
        let Some(name) = &step.host else {
            let first = pinned_hosts().next();
            return match first {
                Some(host) if pinned_hosts().all(|other| other == host) => {
                    let label = match self.hosts.get_host(host).await {
                        Ok(Some(found)) => found.name.to_string(),
                        _ => host.to_string(),
                    };
                    Ok((host, label))
                }
                _ => Err(fail(
                    Code::UnknownHost,
                    "name the host: several are part of this investigation",
                )),
            };
        };
        let unknown = || fail(Code::UnknownHost, format!("no host named `{name}`"));
        let parsed = HostName::parse(name).map_err(|_| unknown())?;
        let found = match self.hosts.get_host_by_name(&parsed).await {
            Ok(found) => found,
            Err(error) => {
                tracing::error!(%error, "plan validation could not read the host");
                return Err(fail(Code::TargetUnresolved, "could not read the host"));
            }
        };
        let host = found.ok_or_else(unknown)?;
        if !pinned_hosts().any(|pinned| pinned == host.id) {
            return Err(fail(
                Code::HostNotInScope,
                format!("host `{name}` is not part of this investigation"),
            ));
        }
        Ok((host.id, name.clone()))
    }

    /// Check 3: the capability.
    fn find_capability(&self, host: HostId, text: &str) -> Result<(ContractId, Capability), Fail> {
        let skills = self.registry.enabled_skills(host);
        let family = text.starts_with("skill.");
        let Ok(id) = ContractId::parse(text) else {
            let mut error = unknown_contract(text, text);
            error.2 = skills
                .iter()
                .find(|skill| skill.name.as_str() == text)
                .map(|skill| format!("did you mean `skill.{}`?", skill.name))
                .or(error.2);
            return Err(error);
        };
        if let Some(name) = id.skill_name()
            && let Some(skill) = skills.into_iter().find(|skill| skill.name == name)
        {
            return Ok((id, Capability::Skill(Box::new(skill))));
        }
        if !family && self.registry.contracts().get(&id).is_some() {
            return Ok((id, Capability::Builtin));
        }
        Err(unknown_contract(text, &id))
    }

    /// Check 9: availability. Not allowed on host and unavailable are notes, not refusals.
    pub(super) async fn availability(
        &self,
        host: HostId,
        id: &ContractId,
    ) -> Result<Option<String>, Fail> {
        let state = self.registry.availability(host, id).await;
        match state {
            Availability::Available => Ok(None),
            Availability::NotAllowedOnHost { .. } | Availability::Unavailable { .. } => {
                Ok(Some(state.describe()))
            }
            Availability::HostUnknown | Availability::HostAbsent { .. } => Err(fail(
                Code::HostOffline,
                "the host's agent is not connected; propose again after it reconnects",
            )),
            other => Err(fail(Code::Unavailable, other.describe())),
        }
    }
}

pub(super) fn plain((code, message): Refuse) -> Fail {
    (code, message, None)
}

pub(super) fn fail(code: Code, message: impl Into<String>) -> Fail {
    (code, message.into(), None)
}

fn unknown_contract(text: &str, id: impl std::fmt::Display) -> Fail {
    if text.starts_with("skill.") {
        (
            Code::UnknownContract,
            "not a skill on this host".to_owned(),
            Some("call list_capabilities".to_owned()),
        )
    } else {
        (
            Code::UnknownContract,
            format!("no capability `{id}` on this host"),
            Some("call list_capabilities".to_owned()),
        )
    }
}

/// Check 6: which pinned subject the step acts on, and that its target is that subject's.
fn pick_subject<'a>(
    scope: &'a PlanScope,
    host: HostId,
    step: &PlanStepInput,
) -> Result<&'a PinnedSubject, Fail> {
    let candidates: Vec<&PinnedSubject> = scope
        .pinned
        .iter()
        .filter(|subject| subject.host == host)
        .collect();
    let subject = match candidates.as_slice() {
        [only] => *only,
        _ => candidates
            .iter()
            .copied()
            .find(|subject| {
                step.target.as_deref() == subject.target.as_ref().map(TargetKey::as_str)
            })
            .ok_or_else(|| fail(Code::TargetMismatch, "name which target you mean"))?,
    };
    if let Some(given) = &step.target {
        let pinned = subject.target.as_ref().map(TargetKey::as_str);
        if pinned != Some(given.as_str()) {
            return Err(fail(
                Code::TargetMismatch,
                format!(
                    "target `{given}` is not this investigation's target (`{}`)",
                    pinned.unwrap_or("none")
                ),
            ));
        }
    }
    Ok(subject)
}

/// Check 8: the manifest's typed check first, then the schema as a second line of defence.
fn check_args(skill: &SkillSummary, args: &BTreeMap<String, String>) -> Result<(), Fail> {
    skill
        .validate_args(args)
        .map_err(|error| fail(Code::BadArguments, format!("bad arguments: {error}")))?;
    check_schema(&skill_input_schema(&skill.params), args).map_err(|problems| {
        let text: Vec<String> = problems
            .iter()
            .take(3)
            .map(|p| format!("{}: {}", p.path, p.problem))
            .collect();
        fail(
            Code::BadArguments,
            format!("arguments: {}", text.join("; ")),
        )
    })
}

pub(super) struct RunKey<'a> {
    pub(super) contract: &'a ContractId,
    pub(super) host: HostId,
    pub(super) check: &'a CheckKey,
    pub(super) target: Option<&'a TargetKey>,
    pub(super) args: &'a BTreeMap<String, String>,
}

fn same_prior(run: &PriorRun, key: &RunKey<'_>) -> bool {
    &run.contract == key.contract
        && run.host == key.host
        && &run.check == key.check
        && run.target.as_ref() == key.target
        && run.args == *key.args
}

fn same_planned(step: &PlannedStep, key: &RunKey<'_>) -> bool {
    &step.contract == key.contract
        && step.host == key.host
        && &step.check == key.check
        && step.target.as_ref() == key.target
        && step.args == *key.args
}

fn is_failure(outcome: StepOutcome) -> bool {
    matches!(outcome, StepOutcome::Failed | StepOutcome::OutcomeUnknown)
}

/// Checks 11 to 13: duplicates and retries. Returns the carried `retry_of`.
pub(super) fn check_history(
    scope: &PlanScope,
    staged: &[PlannedStep],
    accepted: &[PlannedStep],
    key: &RunKey<'_>,
    step: &PlanStepInput,
) -> Result<Option<RetryOf>, Fail> {
    let open = scope.prior.iter().any(|run| {
        matches!(run.outcome, StepOutcome::Pending | StepOutcome::Running) && same_prior(run, key)
    });
    if open {
        return Err(fail(
            Code::AlreadyProposed,
            "the same step is already waiting in this operation",
        ));
    }
    if staged
        .iter()
        .chain(accepted)
        .any(|planned| same_planned(planned, key))
    {
        return Err(fail(
            Code::DuplicateStep,
            "the same step is already in your plan",
        ));
    }
    if let Some(claim) = &step.retry_of {
        let bad = || {
            fail(
                Code::BadRetry,
                "retry_of must name a failed or unknown-outcome step of this operation and say what changed",
            )
        };
        let id = StepId::from_str(&claim.step).map_err(|_| bad())?;
        let changed = claim.changed.trim();
        let named = scope
            .prior
            .iter()
            .any(|run| run.step == id && is_failure(run.outcome));
        if !named || changed.is_empty() || changed.chars().count() > 280 {
            return Err(bad());
        }
        return Ok(Some(RetryOf {
            step: id,
            changed: changed.to_owned(),
        }));
    }
    if scope
        .prior
        .iter()
        .any(|run| is_failure(run.outcome) && same_prior(run, key))
    {
        return Err(fail(
            Code::BlindRetry,
            "the same run failed earlier; say what changed with retry_of",
        ));
    }
    Ok(None)
}
