//! Edges: typed links from a step to an earlier step of the same operation.

use serde::{Deserialize, Serialize};

use crate::id::{OperationId, StepId};

use super::step::{Step, StepKind};

/// What an edge says about the two steps it joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The dependent step needs the prerequisite to be done first.
    DependsOn,
    /// The dependent step exists because of the earlier one.
    CausedBy,
    /// A Verification checks an Execution.
    Verifies,
    /// An Execution retries an earlier Execution.
    RetryOf,
    /// A Note answers a Question.
    Answers,
}

impl EdgeKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::DependsOn,
        Self::CausedBy,
        Self::Verifies,
        Self::RetryOf,
        Self::Answers,
    ];

    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DependsOn => "depends_on",
            Self::CausedBy => "caused_by",
            Self::Verifies => "verifies",
            Self::RetryOf => "retry_of",
            Self::Answers => "answers",
        }
    }

    /// Reads [`EdgeKind::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// A directed edge from a dependent step to a prerequisite.
///
/// `to` always has a lower ordinal than `from` and the same operation, so the graph of an
/// operation cannot contain a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepEdge {
    /// The operation both steps belong to.
    pub operation: OperationId,
    /// The dependent step, the later one.
    pub from: StepId,
    /// The prerequisite, the earlier one.
    pub to: StepId,
    /// What the edge says.
    pub kind: EdgeKind,
}

/// Why an edge is not allowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeError {
    /// The steps belong to different operations.
    #[error("an edge must join steps of the same operation")]
    DifferentOperations,
    /// The step would depend on itself.
    #[error("a step cannot point at itself")]
    SelfLoop,
    /// The prerequisite is not earlier than the dependent step.
    #[error("an edge must point to an earlier step of the same operation")]
    NotBackwards,
    /// The kinds of the two steps do not fit the edge kind.
    #[error("a {kind} edge cannot join a {from} step to a {to} step")]
    Endpoints {
        /// The edge kind.
        kind: &'static str,
        /// The dependent step's kind.
        from: &'static str,
        /// The prerequisite's kind.
        to: &'static str,
    },
}

/// Checks the edge rules for an edge of `kind` from `from` to `to`: same operation, distinct
/// steps, `to` earlier than `from`, and endpoint kinds that fit the edge kind.
pub fn check_edge(from: &Step, to: &Step, kind: EdgeKind) -> Result<(), EdgeError> {
    if from.operation != to.operation {
        return Err(EdgeError::DifferentOperations);
    }
    if from.id == to.id {
        return Err(EdgeError::SelfLoop);
    }
    if to.ordinal >= from.ordinal {
        return Err(EdgeError::NotBackwards);
    }
    let fits = match kind {
        EdgeKind::DependsOn | EdgeKind::CausedBy => true,
        EdgeKind::Verifies => from.kind == StepKind::Verification && to.kind == StepKind::Execution,
        EdgeKind::RetryOf => from.kind == StepKind::Execution && to.kind == StepKind::Execution,
        EdgeKind::Answers => from.kind == StepKind::Note && to.kind == StepKind::Question,
    };
    if fits {
        Ok(())
    } else {
        Err(EdgeError::Endpoints {
            kind: kind.as_str(),
            from: from.kind.as_str(),
            to: to.kind.as_str(),
        })
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::super::intent::StepIntent;
    use super::*;

    fn step(operation: OperationId, ordinal: u32, intent: StepIntent) -> Step {
        Step {
            id: StepId::new(),
            operation,
            ordinal,
            kind: intent.kind(),
            phase: "work".to_owned(),
            parent: None,
            cycle: 0,
            intent,
            created_at: Timestamp::UNIX_EPOCH,
        }
    }

    fn note(operation: OperationId, ordinal: u32) -> Step {
        step(
            operation,
            ordinal,
            StepIntent::Note {
                text: "n".to_owned(),
                author: crate::Actor::System("operations".to_owned()),
            },
        )
    }

    fn question(operation: OperationId, ordinal: u32) -> Step {
        step(
            operation,
            ordinal,
            StepIntent::Question {
                text: "q".to_owned(),
                expires_at: Timestamp::UNIX_EPOCH,
            },
        )
    }

    #[test]
    fn edge_kinds_round_trip() {
        assert_eq!(EdgeKind::ALL.len(), 5);
        for kind in EdgeKind::ALL {
            assert_eq!(EdgeKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::json!(kind.as_str())
            );
        }
        assert_eq!(EdgeKind::parse("blocks"), None);
    }

    #[test]
    fn edges_point_backwards_inside_one_operation() {
        let op = OperationId::new();
        let early = note(op, 0);
        let late = note(op, 1);
        assert_eq!(check_edge(&late, &early, EdgeKind::DependsOn), Ok(()));
        assert_eq!(
            check_edge(&early, &late, EdgeKind::DependsOn),
            Err(EdgeError::NotBackwards)
        );
        assert_eq!(
            check_edge(&late, &late, EdgeKind::DependsOn),
            Err(EdgeError::SelfLoop)
        );
        let other = note(OperationId::new(), 0);
        assert_eq!(
            check_edge(&late, &other, EdgeKind::DependsOn),
            Err(EdgeError::DifferentOperations)
        );
    }

    #[test]
    fn edge_kind_endpoint_rules_are_enforced() {
        let op = OperationId::new();
        let q = question(op, 0);
        let n = note(op, 1);
        assert_eq!(check_edge(&n, &q, EdgeKind::Answers), Ok(()));
        assert!(check_edge(&n, &q, EdgeKind::Verifies).is_err());
        assert!(check_edge(&n, &q, EdgeKind::RetryOf).is_err());
    }
}
