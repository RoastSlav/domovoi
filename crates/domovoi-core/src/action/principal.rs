//! Who did something.

use std::borrow::Cow;

use crate::id::UserId;

/// Who did something. Only `Human` can approve, and only through a [`HumanApproval`].
///
/// [`HumanApproval`]: super::HumanApproval
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Principal {
    /// An authenticated user.
    Human(UserId),
    /// A Domovoi component, for example `triage`.
    System(Cow<'static, str>),
}
