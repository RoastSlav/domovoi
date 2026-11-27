//! Why a knowledge operation failed.

use domovoi_core::knowledge::{
    FactSource, KnowledgeStatus, Predicate, SubjectError, UnverifiableReason, ValueError,
};
use domovoi_core::ports::{CommitError, StoreError};
use std::fmt;

/// Why a knowledge operation changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeError {
    /// A subject reference is malformed.
    InvalidSubject(SubjectError),
    /// A value does not fit its predicate.
    InvalidValue(ValueError),
    /// The request is malformed in another way; the text says how.
    InvalidInput(String),
    /// The predicate cannot describe this kind of subject.
    SubjectNotAllowed {
        /// The predicate.
        predicate: Predicate,
    },
    /// The actor may not write facts from this source.
    ActorNotAllowed {
        /// The source that was refused.
        source: FactSource,
    },
    /// Nothing is stored under the id.
    NotFound,
    /// A version check failed; reload and retry.
    Conflict,
    /// The fact is no longer live.
    NotLive {
        /// Its stored status.
        status: KnowledgeStatus,
    },
    /// Only an owner-sourced fact can be confirmed.
    NotOwnerFact,
    /// A management path is confirmed, not verified.
    UseConfirmation,
    /// The helper does not cover the path.
    PathUncovered,
    /// Coverage cannot be established.
    CoverageUnverifiable(UnverifiableReason),
    /// An entity with this name exists.
    DuplicateEntity,
    /// Something live refers to the entity.
    EntityInUse,
    /// The store failed.
    Store(StoreError),
}

impl fmt::Display for KnowledgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSubject(e) => write!(f, "invalid subject: {e}"),
            Self::InvalidValue(e) => write!(f, "invalid value: {e}"),
            Self::InvalidInput(text) => f.write_str(text),
            Self::SubjectNotAllowed { predicate } => {
                write!(f, "{} does not apply to this subject", predicate.as_str())
            }
            Self::ActorNotAllowed { source } => {
                write!(f, "this actor may not write {} facts", source.as_str())
            }
            Self::NotFound => f.write_str("not found"),
            Self::Conflict => f.write_str("someone changed it meanwhile; reload and try again"),
            Self::NotLive { status } => write!(f, "the fact is {}", status.as_str()),
            Self::NotOwnerFact => f.write_str("accept the path first"),
            Self::UseConfirmation => f.write_str("confirm the path instead"),
            Self::PathUncovered => f.write_str(
                "the helper does not cover this path: add this network to management_cidrs in /etc/domovoi/helper.toml on the host and restart the helper",
            ),
            Self::CoverageUnverifiable(reason) => f.write_str(reason.describe()),
            Self::DuplicateEntity => f.write_str("an entity with this name exists"),
            Self::EntityInUse => f.write_str("something still refers to this entity"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for KnowledgeError {}

impl From<SubjectError> for KnowledgeError {
    fn from(error: SubjectError) -> Self {
        Self::InvalidSubject(error)
    }
}

impl From<ValueError> for KnowledgeError {
    fn from(error: ValueError) -> Self {
        Self::InvalidValue(error)
    }
}

impl From<StoreError> for KnowledgeError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<CommitError> for KnowledgeError {
    fn from(error: CommitError) -> Self {
        match error {
            CommitError::Conflict { .. } | CommitError::Constraint(_) => Self::Conflict,
            other => Self::Store(StoreError::Backend(other.to_string())),
        }
    }
}

#[cfg(test)]
mod uncovered_text {
    use super::KnowledgeError;

    #[test]
    fn an_uncovered_path_says_where_to_add_it() {
        let text = KnowledgeError::PathUncovered.to_string();
        assert!(text.contains("management_cidrs"), "{text}");
        assert!(text.contains("/etc/domovoi/helper.toml"), "{text}");
    }
}
