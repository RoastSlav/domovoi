//! Turning a failed call into a token, and an absent element into expired, rolled back, covered
//! or lost.
//!
//! Error text picks a class; the table check, the JSON membership read and the journal decide
//! what the class means. Text alone never decides `expired`.

use domovoi_core::containment::templates::SetName;
use domovoi_core::containment::{
    AbsentClass, AbsentFacts, Net, NftOp, Token, classify_absent, classify_stderr,
};
use jiff::{SignedDuration, Timestamp};

use super::ensure::read_table;
use super::exec::{Fault, Run};
use super::journal_file::{BlockState, JournalError};
use super::{Cx, Refusal};

/// How far before its deadline a missing first-step element still counts as expired, covering
/// the helper's and the kernel's clocks differing by a moment.
const TOLERANCE: SignedDuration = SignedDuration::from_secs(2);

impl Refusal {
    /// The refusal for a call that produced no result.
    #[must_use]
    pub fn of_fault(fault: &Fault) -> Self {
        match fault {
            Fault::ToolMissing => Self::new(Token::ContainmentBlocked, "nft_missing"),
            Fault::TimedOut => Self::new(Token::NftTimeout, "the call timed out"),
            Fault::Spawn(why) => Self::new(Token::PermissionDenied, why.clone()),
        }
    }

    /// The refusal for a call that exited non-zero, by its text.
    #[must_use]
    pub fn from_run(op: NftOp, run: &Run, table_exists: bool) -> Self {
        let token = classify_stderr(op, &run.stderr)
            .token(matches!(op, NftOp::Rollback), table_exists)
            .unwrap_or(Token::InternalTemplateError);
        Self::new(token, run.stderr.lines().next().unwrap_or_default())
    }

    /// The refusal for a journal that cannot be read or written.
    #[must_use]
    pub fn journal(error: JournalError) -> Self {
        match error {
            JournalError::Corrupt => Self::new(Token::JournalCorrupt, "the journal is corrupt"),
            JournalError::Io(_) => {
                Self::new(Token::JournalCorrupt, "the journal cannot be written")
            }
        }
    }
}

/// What the table says about `net` right now.
pub struct Membership {
    /// Whether the table exists.
    pub table_exists: bool,
    /// Whether an element is exactly `net`.
    pub exact: bool,
    /// The text of an element that covers `net` without being it.
    pub covering: Option<String>,
}

/// Reads the table and says where `net` stands.
pub async fn membership(cx: &Cx<'_>, net: &Net) -> Result<Membership, Refusal> {
    let Some(view) = read_table(cx).await? else {
        return Ok(Membership {
            table_exists: false,
            exact: false,
            covering: None,
        });
    };
    let set = SetName::for_net(net).as_str();
    let members = view
        .sets
        .get(set)
        .map(|s| s.members.as_slice())
        .unwrap_or(&[]);
    let exact = members.iter().any(|m| m.is(net));
    let covering = members
        .iter()
        .find(|m| m.covers(net))
        .map(super::view::Member::text);
    Ok(Membership {
        table_exists: true,
        exact,
        covering,
    })
}

/// Decides why the element is not there, from the table, the membership and the journal.
pub async fn absent(
    cx: &Cx<'_>,
    net: &Net,
    block: Option<&BlockState>,
    now: Timestamp,
) -> Result<(AbsentClass, Membership), Refusal> {
    let membership = membership(cx, net).await?;
    let past_step1_deadline = block
        .and_then(|b| b.applied_at)
        .is_some_and(|at| now >= at + SignedDuration::from_secs(30) - TOLERANCE);
    let facts = AbsentFacts {
        table_exists: membership.table_exists,
        exact_member: membership.exact,
        covering_member: membership.covering.is_some(),
        rollback_recorded: block.is_some_and(|b| b.rollback_recorded),
        past_step1_deadline,
    };
    Ok((classify_absent(&facts), membership))
}
