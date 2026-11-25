//! The `nft` operation group: `firewall.apply`, `firewall.confirm`, `firewall.rollback` and
//! `firewall.list_blocks`.
//!
//! Every request runs under the single `state.lock`, in this order: the journal is read, the gate
//! is computed from the stored self-test result, the table is ensured (created when absent,
//! compared when present), and then the operation runs its own checks. The only text sent to
//! `nft` is rendered by `domovoi_core::containment::templates` from validated values. The helper
//! recomputes its own exclusion set and compares `expected_safety` for every caller; only
//! `rollback` skips those checks, because it only removes a block.

pub mod apply;
pub mod classify;
pub mod command;
pub mod confirm;
pub mod drill;
pub mod ensure;
pub mod exec;
pub mod flows;
pub mod journal_file;
pub mod limits;
pub mod list;
pub mod recover;
pub mod rollback;
pub mod selftest;
mod serve;
pub mod verify;
pub mod view;

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::attest::{ExpectedSafety, SafetyIdentity};
use domovoi_core::containment::templates::{MaxElements, TableName};
use domovoi_core::containment::{Gate, Net, Token, format_message};
use domovoi_core::helper::HelperResponse;
use domovoi_core::ports::Clock;

use crate::state::LockGuard;
use exec::Runner;
use journal_file::{Journal, Loaded};

pub use serve::serve;

/// The name of the file that holds the stored self-test result.
pub const SELFTEST_FILE: &str = "nft-selftest.json";
/// The name of the file that holds the drill result.
pub const DRILLS_FILE: &str = "drills.json";
/// The output key that says how a rollback ended (`rolled_back`, `absent` or `absent_covered`).
pub const OUT_OUTCOME: &str = "outcome";
/// The output key that names the element covering a rolled-back target.
pub const OUT_COVERING: &str = "covering";
/// The output key that counts flowtables on the host.
pub const OUT_FLOWTABLES: &str = "flowtables";

/// The outputs of a served operation.
pub type Outputs = BTreeMap<String, String>;
/// What an operation gives back: outputs, or a token and a detail.
pub type Reply = Result<Outputs, Refusal>;

/// A refusal or failure: one token of the containment table and a short detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The token.
    pub token: Token,
    /// At most 200 printable ASCII characters; never a path outside the helper's own.
    pub detail: String,
}

impl Refusal {
    /// A refusal with `token` and `detail`.
    #[must_use]
    pub fn new(token: Token, detail: impl Into<String>) -> Self {
        let detail: String = detail
            .into()
            .chars()
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .take(200)
            .collect();
        Self { token, detail }
    }

    /// The wire answer.
    #[must_use]
    pub fn response(&self) -> HelperResponse {
        HelperResponse::ContainRefused {
            token: self.token.as_str().to_owned(),
            detail: (!self.detail.is_empty()).then(|| self.detail.clone()),
        }
    }

    /// The `"<token>: <detail>"` message the agent forwards.
    #[must_use]
    pub fn message(&self) -> String {
        format_message(self.token, &self.detail)
    }
}

/// Whether the table was found, created or created again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableState {
    /// It existed and matched.
    Ok,
    /// It was absent and the bootstrap created it.
    Created,
    /// It was gone with live blocks in it and was created again, empty.
    Recreated,
}

impl TableState {
    /// The wire word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Created => "created",
            Self::Recreated => "recreated",
        }
    }
}

/// What every operation needs from the host.
pub struct Cx<'a> {
    /// Runs `nft` and `conntrack`.
    pub run: Runner<'a>,
    /// The clock the journal, the deadlines and the limiter run on.
    pub clock: &'a dyn Clock,
    /// The table the helper owns.
    pub table: TableName,
    /// The size of each block set.
    pub max_elements: MaxElements,
    /// The longest TTL, in seconds.
    pub max_ttl_s: u64,
    /// `[safety.containment] max_active`.
    pub max_active: u32,
    /// `[limits] mutations_per_hour`.
    pub mutations_per_hour: u32,
    /// The helper's state directory.
    pub dir: &'a Path,
    /// Where `/sys` is.
    pub sys_root: &'a Path,
    /// The journal.
    pub journal: Journal,
    /// Proof that the state lock is held.
    pub guard: &'a LockGuard<'a>,
}

/// The facts the operations check, computed once per request under the lock.
pub struct Facts {
    /// The gate from the stored self-test result.
    pub gate: Gate,
    /// The effective exclusion set, or the token that says why none can be computed.
    pub exclusions: Result<Vec<Net>, Refusal>,
    /// The identity a verified approval must equal.
    pub identity: Result<SafetyIdentity, ()>,
    /// The kernel release, the `nft` version and the layout, for the gate text.
    pub key_text: String,
    /// The drill state as JSON text.
    pub drills: String,
}

/// The table, the journal and the facts an operation works from.
pub struct State {
    /// The table after `ensure_table`.
    pub ens: ensure::Ensured,
    /// The journal as read at the start of the request.
    pub loaded: Loaded,
    /// The facts.
    pub facts: Facts,
}

/// The request's `expected_safety`, required for `apply` and `confirm`.
pub type Expected<'a> = Option<&'a ExpectedSafety>;
