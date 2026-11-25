//! The values the policy use cases take and return: constants, the step-up proof, previews
//! and the error every command and read can end in.

use std::net::IpAddr;
use std::sync::Arc;

use domovoi_core::policy_store::PolicyVersionRecord;
use domovoi_core::ports::{Clock, CommitError, PolicyRepo, StoreError, UnitOfWork};
use domovoi_engine::policy::PublicationReport;
use domovoi_engine::policy_shadow::{LockChange, ShadowEnvironment, SimulationSummary};

use crate::auth::AuthState;

/// What a refused step-up says when no password came with the request.
pub const PASSWORD_REQUIRED_TEXT: &str = "enter your password to change the policy";
/// What a refused step-up says for a wrong password.
pub const BAD_PASSWORD_TEXT: &str = "wrong password";
/// Audit kind of a published version.
pub const AUDIT_PUBLISHED: &str = "policy.published";
/// Audit kind of a change to a locked prohibition.
pub const AUDIT_LOCK_CHANGED: &str = "policy.lock_changed";
/// Audit kind of a saved draft.
pub const AUDIT_DRAFT_SAVED: &str = "policy.draft_saved";
/// Audit kind of a discarded draft.
pub const AUDIT_DRAFT_DISCARDED: &str = "policy.draft_discarded";
/// Rows a preview replays to summarise what a change would do.
pub const SIM_PREVIEW_LIMIT: u16 = 200;
/// The largest encoded rule set a draft may hold, in bytes.
pub const MAX_DRAFT_BYTES: usize = 512 * 1024;
/// Contract ids with this prefix are never policy-routed: package installs are standalone jobs.
pub const PACKAGE_PREFIX: &str = "agent.package.";

/// The three standing notes the overview and every preview carry.
pub const STANDING_NOTES: [&str; 3] = [
    "No agent reports an unattended ceiling yet, so every recorded decision treats the ceiling as unavailable.",
    "Package installs are standalone jobs and are never evaluated by policy.",
    "Published rules that name a service restart are enforced, within the pause switch, the circuit breaker and the limits. Every other rule is recorded for comparison only.",
];

/// What the policy service is built from.
pub struct PolicyDeps {
    /// Policy versions and recorded decisions, read only.
    pub repo: Arc<dyn PolicyRepo>,
    /// Where commands commit.
    pub uow: Arc<dyn UnitOfWork>,
    /// What the hub knows of hosts, for the publication review and `current` simulations.
    pub env: Arc<dyn ShadowEnvironment>,
    /// The password check for step-up.
    pub auth: Arc<AuthState>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Whether `[policy] shadow` is on.
    pub shadow_enabled: bool,
}

/// The step-up proof a transport collects: the client address (after trusted-proxy handling)
/// and the password from the request body.
#[derive(Debug, Clone, Copy)]
pub struct StepUp<'a> {
    /// The client address, shared with the login limiter.
    pub ip: IpAddr,
    /// The password typed again, if any.
    pub password: Option<&'a str>,
}

/// One problem found in a rule set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleIssue {
    /// The rule it is about, when it is about one.
    pub rule: Option<String>,
    /// What is wrong; ids only, never owner text.
    pub message: String,
}

/// A saved draft and the locked prohibitions the save touched.
#[derive(Debug, Clone)]
pub struct SavedDraft {
    /// The draft now stored.
    pub record: PolicyVersionRecord,
    /// Locked rules the save created, edited, unlocked or removed.
    pub locked_changes: Vec<LockChange>,
}

/// What a draft or a preview shows about a candidate rule set.
#[derive(Debug, Clone)]
pub struct Preview {
    /// The active version number the change is measured from, 0 when none.
    pub base_number: u32,
    /// Hash of the candidate rules.
    pub rules_hash: String,
    /// Problems that stop the rules from being saved or published.
    pub validation: Vec<RuleIssue>,
    /// The publication review, when the rules are valid.
    pub report: Option<PublicationReport>,
    /// Locked prohibitions the change touches.
    pub locked_changes: Vec<LockChange>,
    /// What the change would do to recent recorded decisions.
    pub simulation: Option<SimulationSummary>,
    /// The hash a save or publication echoes to prove the owner saw exactly this.
    pub preview_hash: String,
    /// Keys of the findings that must be acknowledged before publishing.
    pub blocking: Vec<String>,
    /// Standing notes.
    pub notes: Vec<&'static str>,
}

/// Why a policy command or read was refused.
#[derive(Debug, thiserror::Error)]
pub enum PolicyCommandError {
    /// A step-up was needed and no password came with it.
    #[error("enter your password to change the policy")]
    PasswordRequired,
    /// The password is wrong.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    TooMany {
        /// Seconds until the next attempt.
        retry_after_secs: i64,
    },
    /// The rules are not valid.
    #[error("the rules are not valid")]
    Invalid(Vec<RuleIssue>),
    /// More rules than a version may hold.
    #[error("a policy holds at most {max} rules")]
    TooManyRules {
        /// The limit.
        max: usize,
    },
    /// A rule's note is too long.
    #[error("the note of rule {rule} is too long")]
    NoteTooLong {
        /// The rule.
        rule: String,
    },
    /// A rule names a package contract.
    #[error("rule {rule} names a package contract, which policy never evaluates")]
    PackageContract {
        /// The rule.
        rule: String,
    },
    /// There is no draft.
    #[error("there is no draft")]
    NoDraft,
    /// A draft exists already.
    #[error("a draft already exists; replace it with its current revision")]
    DraftExists,
    /// The draft changed meanwhile.
    #[error("the draft changed meanwhile; it is now at revision {current}")]
    StaleDraft {
        /// The draft's revision now.
        current: u64,
    },
    /// Another version was published meanwhile.
    #[error("another version was published meanwhile; save the draft again to re-base it")]
    StaleActive {
        /// The active version number now.
        current: u32,
    },
    /// The echoed preview hash does not match what the rules now preview as.
    #[error("the preview is out of date; review it again")]
    StalePreview,
    /// A blocking finding was not acknowledged.
    #[error("acknowledge every blocking finding first: {}", keys.join(", "))]
    Unacknowledged {
        /// The findings still to acknowledge.
        keys: Vec<String>,
    },
    /// An acknowledgement names a finding that does not block.
    #[error("not blocking findings of this draft: {}", keys.join(", "))]
    UnknownAcknowledgement {
        /// The keys that do not match.
        keys: Vec<String>,
    },
    /// The encoded rule set is over the size limit.
    #[error("the rule set is too large")]
    TooLarge,
    /// The version does not exist.
    #[error("not found")]
    NotFound,
    /// The request is not valid; the text says why.
    #[error("{0}")]
    BadRequest(String),
    /// The state does not allow this now; the text says why.
    #[error("{0}")]
    Conflict(String),
    /// This hub does not run unattended execution.
    #[error("unattended execution is not set up on this hub")]
    Unavailable,
    /// A read failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A commit failed.
    #[error(transparent)]
    Commit(CommitError),
}
