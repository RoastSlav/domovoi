//! Totals over a batch of re-runs.

use serde::Serialize;

use super::doc::DecisionEnvelope;
use super::replay::{Comparison, Replayed};
use crate::policy::Outcome;

/// How a batch of re-runs compares with what was recorded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SimulationSummary {
    /// Rows looked at.
    pub evaluated: u32,
    /// Rows whose decision did not change.
    pub same: u32,
    /// Rows whose decision changed.
    pub changed: u32,
    /// Rows that could not be replayed.
    pub not_replayable: u32,
    /// Changed rows that are now `allow`.
    pub newly_allowed: u32,
    /// Changed rows that are now `deny`.
    pub newly_denied: u32,
    /// Changed rows that now `require_approval`.
    pub newly_requires_approval: u32,
}

/// Counts `items`. `unavailable` and error results count only in `changed`.
#[must_use]
pub fn summarise(items: &[Replayed]) -> SimulationSummary {
    let mut summary = SimulationSummary::default();
    for item in items {
        summary.evaluated = summary.evaluated.saturating_add(1);
        match item.comparison {
            Comparison::Same => summary.same = summary.same.saturating_add(1),
            Comparison::NotReplayable => {
                summary.not_replayable = summary.not_replayable.saturating_add(1);
            }
            Comparison::Changed => {
                summary.changed = summary.changed.saturating_add(1);
                if let DecisionEnvelope::Decision(doc) = &item.simulated {
                    match doc.outcome {
                        Outcome::Allow => {
                            summary.newly_allowed = summary.newly_allowed.saturating_add(1);
                        }
                        Outcome::Deny => {
                            summary.newly_denied = summary.newly_denied.saturating_add(1);
                        }
                        Outcome::RequireApproval => {
                            summary.newly_requires_approval =
                                summary.newly_requires_approval.saturating_add(1);
                        }
                        Outcome::Unavailable(_) => {}
                    }
                }
            }
        }
    }
    summary
}
