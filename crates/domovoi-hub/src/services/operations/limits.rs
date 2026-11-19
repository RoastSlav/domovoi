//! The bounds on what a person may type into an operation control.

use domovoi_core::BudgetKind;

use super::{BudgetExtension, OperationCommandError};

/// The most characters of a cancel reason.
pub const CANCEL_REASON_MAX: usize = 500;
/// The most characters of an answer.
pub const ANSWER_MAX: usize = 2000;
/// The most limits one resume may raise.
pub const EXTEND_MAX_ENTRIES: usize = 7;
/// The highest limit of a counted budget (model turns, evidence requests, executions, cycles).
pub const EXTEND_MAX_COUNT: u64 = 10_000;
/// The highest limit of a measured budget (seconds, tokens, micro-units).
pub const EXTEND_MAX_MEASURED: u64 = 10_000_000_000;
/// The reason recorded when a person gives none.
pub const DEFAULT_CANCEL_REASON: &str = "cancelled by a person";

fn invalid(text: impl Into<String>) -> OperationCommandError {
    OperationCommandError::Invalid(text.into())
}

/// The budget named `kind`, by its snake_case name.
fn parse_kind(kind: &str) -> Option<BudgetKind> {
    serde_json::from_value(serde_json::Value::String(kind.to_owned())).ok()
}

/// Checks every extension and turns it into the writer's form: each limit is a number,
/// never "unlimited".
pub(super) fn checked_extensions(
    extend: &[BudgetExtension],
) -> Result<Vec<(BudgetKind, Option<u64>)>, OperationCommandError> {
    if extend.len() > EXTEND_MAX_ENTRIES {
        return Err(invalid("at most 7 limits can be raised at once"));
    }
    let mut out: Vec<(BudgetKind, Option<u64>)> = Vec::with_capacity(extend.len());
    for entry in extend {
        let kind = parse_kind(&entry.kind)
            .ok_or_else(|| invalid(format!("unknown limit `{}`", entry.kind)))?;
        if out.iter().any(|(seen, _)| *seen == kind) {
            return Err(invalid("each limit can be raised once"));
        }
        let cap = if kind.is_counted() {
            EXTEND_MAX_COUNT
        } else {
            EXTEND_MAX_MEASURED
        };
        if entry.limit < 1 || entry.limit > cap {
            return Err(invalid(format!(
                "the limit for `{}` must be between 1 and {cap}",
                entry.kind
            )));
        }
        out.push((kind, Some(entry.limit)));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext(kind: &str, limit: u64) -> BudgetExtension {
        BudgetExtension {
            kind: kind.to_owned(),
            limit,
        }
    }

    fn text(err: OperationCommandError) -> String {
        match err {
            OperationCommandError::Invalid(text) => text,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn extension_limits_are_checked_per_kind() {
        assert_eq!(
            text(checked_extensions(&[ext("nope", 1)]).unwrap_err()),
            "unknown limit `nope`"
        );
        assert_eq!(
            text(checked_extensions(&[ext("tokens", 1), ext("tokens", 2)]).unwrap_err()),
            "each limit can be raised once"
        );
        assert_eq!(
            text(checked_extensions(&[ext("executions", 0)]).unwrap_err()),
            "the limit for `executions` must be between 1 and 10000"
        );
        assert_eq!(
            text(checked_extensions(&[ext("executions", 10_001)]).unwrap_err()),
            "the limit for `executions` must be between 1 and 10000"
        );
        assert_eq!(
            text(checked_extensions(&[ext("tokens", 10_000_000_001)]).unwrap_err()),
            "the limit for `tokens` must be between 1 and 10000000000"
        );
        assert!(checked_extensions(&[ext("wall_time", 7200), ext("executions", 6)]).is_ok());
        let too_many: Vec<BudgetExtension> = (0..8).map(|_| ext("cycles", 1)).collect();
        assert_eq!(
            text(checked_extensions(&too_many).unwrap_err()),
            "at most 7 limits can be raised at once"
        );
    }
}
