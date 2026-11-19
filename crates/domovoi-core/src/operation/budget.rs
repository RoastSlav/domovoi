//! Budgets: what an operation may spend before it pauses.

use serde::{Deserialize, Serialize};

/// What can run out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    /// Model turns.
    ModelTurns,
    /// Evidence requests.
    EvidenceRequests,
    /// Executions.
    Executions,
    /// Verification cycles.
    Cycles,
    /// Wall-clock time since the operation started.
    WallTime,
    /// Model tokens.
    Tokens,
    /// Model cost in millionths of a currency unit.
    Cost,
}

impl BudgetKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 7] = [
        Self::ModelTurns,
        Self::EvidenceRequests,
        Self::Executions,
        Self::Cycles,
        Self::WallTime,
        Self::Tokens,
        Self::Cost,
    ];

    /// Whether this budget is counted before a step runs (the step is refused once the limit
    /// is used up) rather than measured afterwards.
    #[must_use]
    pub const fn is_counted(self) -> bool {
        matches!(
            self,
            Self::ModelTurns | Self::EvidenceRequests | Self::Executions | Self::Cycles
        )
    }
}

/// The most an operation may spend. `None` means unlimited.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetLimits {
    /// Most model turns.
    pub model_turns: Option<u32>,
    /// Most evidence requests.
    pub evidence_requests: Option<u32>,
    /// Most executions.
    pub executions: Option<u32>,
    /// Most verification cycles.
    pub cycles: Option<u32>,
    /// Most seconds of wall-clock time.
    pub wall_seconds: Option<u64>,
    /// Most model tokens.
    pub tokens: Option<u64>,
    /// Most model cost, in millionths.
    pub cost_micros: Option<u64>,
}

/// What an operation has spent so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetUsed {
    /// Model turns taken.
    pub model_turns: u32,
    /// Evidence requests made.
    pub evidence_requests: u32,
    /// Executions proposed and accepted.
    pub executions: u32,
    /// Seconds of wall-clock time since the operation started.
    pub wall_seconds: u64,
    /// Model tokens used.
    pub tokens: u64,
    /// Model cost so far, in millionths.
    pub cost_micros: u64,
}

/// Limits and use, together. "Tool calls" is derived (evidence requests plus executions),
/// never a counter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budgets {
    /// What may be spent.
    pub limits: BudgetLimits,
    /// What has been spent.
    pub used: BudgetUsed,
}

impl Budgets {
    /// Fresh budgets with nothing used.
    #[must_use]
    pub fn new(limits: BudgetLimits) -> Self {
        Self {
            limits,
            used: BudgetUsed::default(),
        }
    }

    /// Whether `kind` has no room left. Counted kinds are spent once use reaches the limit
    /// (`Cycles` compares the operation's current `cycle`); measured kinds only once use is
    /// past it, because they are noticed after the fact.
    #[must_use]
    pub fn is_spent(&self, kind: BudgetKind, cycle: u32) -> bool {
        let l = &self.limits;
        let u = &self.used;
        match kind {
            BudgetKind::ModelTurns => l.model_turns.is_some_and(|l| u.model_turns >= l),
            BudgetKind::EvidenceRequests => l
                .evidence_requests
                .is_some_and(|l| u.evidence_requests >= l),
            BudgetKind::Executions => l.executions.is_some_and(|l| u.executions >= l),
            BudgetKind::Cycles => l.cycles.is_some_and(|l| cycle >= l),
            BudgetKind::WallTime => l.wall_seconds.is_some_and(|l| u.wall_seconds > l),
            BudgetKind::Tokens => l.tokens.is_some_and(|l| u.tokens > l),
            BudgetKind::Cost => l.cost_micros.is_some_and(|l| u.cost_micros > l),
        }
    }

    /// The first measured budget (wall time, tokens, cost) whose use is past its limit.
    #[must_use]
    pub fn overrun(&self) -> Option<BudgetKind> {
        [BudgetKind::WallTime, BudgetKind::Tokens, BudgetKind::Cost]
            .into_iter()
            .find(|kind| self.is_spent(*kind, 0))
    }

    /// Adds `amount` to what `kind` has used. `Cycles` has no counter here: the operation's
    /// `cycle` is the count.
    pub fn charge(&mut self, kind: BudgetKind, amount: u64) {
        let small = u32::try_from(amount).unwrap_or(u32::MAX);
        let u = &mut self.used;
        match kind {
            BudgetKind::ModelTurns => u.model_turns = u.model_turns.saturating_add(small),
            BudgetKind::EvidenceRequests => {
                u.evidence_requests = u.evidence_requests.saturating_add(small);
            }
            BudgetKind::Executions => u.executions = u.executions.saturating_add(small),
            BudgetKind::Cycles => {}
            BudgetKind::WallTime => u.wall_seconds = u.wall_seconds.saturating_add(amount),
            BudgetKind::Tokens => u.tokens = u.tokens.saturating_add(amount),
            BudgetKind::Cost => u.cost_micros = u.cost_micros.saturating_add(amount),
        }
    }

    /// Replaces the limit of `kind`; `None` removes it. A limit too large for a counted kind
    /// is clamped.
    pub fn extend(&mut self, kind: BudgetKind, new_limit: Option<u64>) {
        let small = new_limit.map(|v| u32::try_from(v).unwrap_or(u32::MAX));
        let l = &mut self.limits;
        match kind {
            BudgetKind::ModelTurns => l.model_turns = small,
            BudgetKind::EvidenceRequests => l.evidence_requests = small,
            BudgetKind::Executions => l.executions = small,
            BudgetKind::Cycles => l.cycles = small,
            BudgetKind::WallTime => l.wall_seconds = new_limit,
            BudgetKind::Tokens => l.tokens = new_limit,
            BudgetKind::Cost => l.cost_micros = new_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counted_budgets_allow_exactly_the_limit() {
        let mut budgets = Budgets::new(BudgetLimits {
            executions: Some(2),
            ..BudgetLimits::default()
        });
        assert!(!budgets.is_spent(BudgetKind::Executions, 0));
        budgets.charge(BudgetKind::Executions, 1);
        assert!(!budgets.is_spent(BudgetKind::Executions, 0));
        budgets.charge(BudgetKind::Executions, 1);
        assert!(budgets.is_spent(BudgetKind::Executions, 0));
        assert!(!budgets.is_spent(BudgetKind::ModelTurns, 0), "unlimited");
    }

    #[test]
    fn cycles_are_counted_from_the_operation_cycle() {
        let budgets = Budgets::new(BudgetLimits {
            cycles: Some(3),
            ..BudgetLimits::default()
        });
        assert!(!budgets.is_spent(BudgetKind::Cycles, 2));
        assert!(budgets.is_spent(BudgetKind::Cycles, 3));
    }

    #[test]
    fn measured_budgets_overrun_only_past_the_limit() {
        let mut budgets = Budgets::new(BudgetLimits {
            tokens: Some(100),
            wall_seconds: Some(60),
            ..BudgetLimits::default()
        });
        budgets.charge(BudgetKind::Tokens, 100);
        assert_eq!(budgets.overrun(), None);
        budgets.charge(BudgetKind::Tokens, 1);
        assert_eq!(budgets.overrun(), Some(BudgetKind::Tokens));
        budgets.extend(BudgetKind::Tokens, None);
        assert_eq!(budgets.overrun(), None);
        budgets.used.wall_seconds = 61;
        assert_eq!(budgets.overrun(), Some(BudgetKind::WallTime));
    }
}
