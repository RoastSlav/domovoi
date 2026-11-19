//! The simulator: the same algorithm over caller-supplied requests.

use super::decision::Decision;
use super::error::PolicyError;
use super::evaluate::{RuleSelection, run};
use super::request::EvaluateRequest;
use super::rule::Rule;

/// Evaluates each request through the same algorithm, returning one element per request in
/// the same order. No I/O, no effects, no permits.
///
/// A malformed request or rule set is an `Err` in that element; no element is dropped.
#[must_use]
pub fn simulate(
    rules: &[Rule],
    requests: &[EvaluateRequest],
    selection: RuleSelection,
) -> Vec<Result<Decision, PolicyError>> {
    requests
        .iter()
        .map(|request| run(rules, request, selection))
        .collect()
}
