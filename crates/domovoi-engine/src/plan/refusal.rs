//! Why a plan step was refused, in a form the model can act on.

/// A stable code for one refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanRefusalCode {
    /// More steps than one investigation may take.
    TooManySteps,
    /// The host name is not known, or not named when it has to be.
    UnknownHost,
    /// The host is not part of the investigation.
    HostNotInScope,
    /// No such capability on the host.
    UnknownContract,
    /// The capability only reads.
    NotAPlanStep,
    /// The capability cannot be run from a plan yet.
    NotExecutable,
    /// The target does not resolve now.
    TargetUnresolved,
    /// More than one thing has the target's name.
    TargetAmbiguous,
    /// The target name is not valid.
    TargetInvalid,
    /// The target is not the investigation's target.
    TargetMismatch,
    /// The host's agent is not connected.
    HostOffline,
    /// The skill does not apply to the pinned check.
    NotApplicable,
    /// The arguments do not fit.
    BadArguments,
    /// The capability is not available on the host.
    Unavailable,
    /// The operation has no execution budget left.
    BudgetSpent,
    /// The same step is already waiting in the operation.
    AlreadyProposed,
    /// The same step is already in this plan.
    DuplicateStep,
    /// The same run failed earlier and nothing says what changed.
    BlindRetry,
    /// The retry claim does not hold.
    BadRetry,
    /// The helper refused the unit; the refusal's token and sentence say why.
    UnitRefused,
}

impl PlanRefusalCode {
    /// The stable name shown to the model.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::TooManySteps => "too_many_steps",
            Self::UnknownHost => "unknown_host",
            Self::HostNotInScope => "host_not_in_scope",
            Self::UnknownContract => "unknown_contract",
            Self::NotAPlanStep => "not_a_plan_step",
            Self::NotExecutable => "not_executable",
            Self::TargetUnresolved => "target_unresolved",
            Self::TargetAmbiguous => "target_ambiguous",
            Self::TargetInvalid => "target_invalid",
            Self::TargetMismatch => "target_mismatch",
            Self::HostOffline => "host_offline",
            Self::NotApplicable => "not_applicable",
            Self::BadArguments => "bad_arguments",
            Self::Unavailable => "unavailable",
            Self::BudgetSpent => "budget_spent",
            Self::AlreadyProposed => "already_proposed",
            Self::DuplicateStep => "duplicate_step",
            Self::BlindRetry => "blind_retry",
            Self::BadRetry => "bad_retry",
            Self::UnitRefused => "unit_refused",
        }
    }

    /// Whether the step is not lost: it already exists in this plan or operation.
    #[must_use]
    pub const fn is_duplicate(&self) -> bool {
        matches!(self, Self::AlreadyProposed | Self::DuplicateStep)
    }
}

/// One refused step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRefusal {
    /// The step's position in the call.
    pub index: usize,
    /// The capability the model named, redacted.
    pub contract: String,
    /// Why.
    pub code: PlanRefusalCode,
    /// A short sentence the model can act on, redacted.
    pub message: String,
    /// What to try, if there is something.
    pub hint: Option<String>,
}

impl PlanRefusal {
    /// The line a report lists under "Proposals not offered": `<skill>: <message>`.
    #[must_use]
    pub fn dropped_line(&self) -> String {
        let name = self
            .contract
            .strip_prefix("skill.")
            .unwrap_or(&self.contract);
        format!("{name}: {}", self.message)
    }
}
