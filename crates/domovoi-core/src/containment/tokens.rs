//! The token table: one spelling per meaning, and the classifier of `nft` failures.
//!
//! A token travels as `"<token>: <detail>"` inside an existing message. Error text alone never
//! decides that an element expired; it picks a class, and the table check, the membership read
//! and the journal decide the rest.

/// Every token a containment refusal, result or failure may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Token {
    /// `expected_safety` missing or malformed.
    MalformedSafety,
    /// The helper's identity differs from the approved one.
    SafetyChanged,
    /// The helper could not compute its identity.
    SafetyUnknown,
    /// The leak self-test gate is not open.
    ContainmentBlocked,
    /// A prefix target on a platform without the interval path.
    PrefixUnsupportedPlatform,
    /// The target is not a canonical network.
    InvalidTarget,
    /// An IPv4-mapped IPv6 target.
    MappedForm,
    /// The prefix is below the floor.
    PrefixTooWide,
    /// The target overlaps an exclusion.
    ExclusionOverlap,
    /// An exclusion input could not be established.
    ExclusionsIncomplete,
    /// No management CIDR is configured.
    NoManagementCidrs,
    /// The confirm TTL is not above 30 s.
    TtlTooShort,
    /// The TTL is above the maximum; refused, never clamped.
    TtlAboveMaximum,
    /// The active count reached the maximum.
    MaxActiveReached,
    /// The per-hour mutation limit is spent.
    MutationLimit,
    /// `confirm` or `rollback` names a block the journal does not hold.
    UnknownBlock,
    /// The block's source or TTL differs from the apply.
    BlockMismatch,
    /// The table differs from the template.
    TableDrift,
    /// The helper's journal is unreadable.
    JournalCorrupt,
    /// Drills have not passed on this host (hub only).
    DrillsNotPassed,
    /// The element was already in the set.
    AlreadyPresent,
    /// The block was removed.
    RolledBack,
    /// The element is absent but a covering element exists; never deleted.
    AbsentCovered,
    /// The kernel timeout removed the element.
    Expired,
    /// Another element covers the target.
    CoveredByOther,
    /// The table or the element vanished without a record.
    ContainmentLost,
    /// The element or the chain failed the check after the change.
    PostconditionFailed,
    /// The set is full.
    SetFull,
    /// The kernel is out of memory or handles.
    ResourceExhausted,
    /// An `nft` call timed out.
    NftTimeout,
    /// The kernel refused the operation.
    PermissionDenied,
    /// A template was rejected by `nft`: a bug.
    InternalTemplateError,
    /// The kernel lacks the chain type.
    UnsupportedKernel,
    /// Another actor created the table first.
    TableExistsRace,
    /// The helper could not be reached.
    HelperUnavailable,
}

impl Token {
    /// Every token.
    pub const ALL: [Self; 35] = [
        Self::MalformedSafety,
        Self::SafetyChanged,
        Self::SafetyUnknown,
        Self::ContainmentBlocked,
        Self::PrefixUnsupportedPlatform,
        Self::InvalidTarget,
        Self::MappedForm,
        Self::PrefixTooWide,
        Self::ExclusionOverlap,
        Self::ExclusionsIncomplete,
        Self::NoManagementCidrs,
        Self::TtlTooShort,
        Self::TtlAboveMaximum,
        Self::MaxActiveReached,
        Self::MutationLimit,
        Self::UnknownBlock,
        Self::BlockMismatch,
        Self::TableDrift,
        Self::JournalCorrupt,
        Self::DrillsNotPassed,
        Self::AlreadyPresent,
        Self::RolledBack,
        Self::AbsentCovered,
        Self::Expired,
        Self::CoveredByOther,
        Self::ContainmentLost,
        Self::PostconditionFailed,
        Self::SetFull,
        Self::ResourceExhausted,
        Self::NftTimeout,
        Self::PermissionDenied,
        Self::InternalTemplateError,
        Self::UnsupportedKernel,
        Self::TableExistsRace,
        Self::HelperUnavailable,
    ];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MalformedSafety => "malformed_safety",
            Self::SafetyChanged => "safety_changed",
            Self::SafetyUnknown => "safety_unknown",
            Self::ContainmentBlocked => "containment_blocked",
            Self::PrefixUnsupportedPlatform => "prefix_unsupported_platform",
            Self::InvalidTarget => "invalid_target",
            Self::MappedForm => "mapped_form",
            Self::PrefixTooWide => "prefix_too_wide",
            Self::ExclusionOverlap => "exclusion_overlap",
            Self::ExclusionsIncomplete => "exclusions_incomplete",
            Self::NoManagementCidrs => "no_management_cidrs",
            Self::TtlTooShort => "ttl_too_short",
            Self::TtlAboveMaximum => "ttl_above_maximum",
            Self::MaxActiveReached => "max_active_reached",
            Self::MutationLimit => "mutation_limit",
            Self::UnknownBlock => "unknown_block",
            Self::BlockMismatch => "block_mismatch",
            Self::TableDrift => "table_drift",
            Self::JournalCorrupt => "journal_corrupt",
            Self::DrillsNotPassed => "drills_not_passed",
            Self::AlreadyPresent => "already_present",
            Self::RolledBack => "rolled_back",
            Self::AbsentCovered => "absent_covered",
            Self::Expired => "expired",
            Self::CoveredByOther => "covered_by_other",
            Self::ContainmentLost => "containment_lost",
            Self::PostconditionFailed => "postcondition_failed",
            Self::SetFull => "set_full",
            Self::ResourceExhausted => "resource_exhausted",
            Self::NftTimeout => "nft_timeout",
            Self::PermissionDenied => "permission_denied",
            Self::InternalTemplateError => "internal_template_error",
            Self::UnsupportedKernel => "unsupported_kernel",
            Self::TableExistsRace => "table_exists_race",
            Self::HelperUnavailable => "helper_unavailable",
        }
    }

    /// Parses a wire spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == text)
    }
}

/// The message form `"<token>: <detail>"`.
#[must_use]
pub fn format_message(token: Token, detail: &str) -> String {
    if detail.is_empty() {
        token.as_str().to_owned()
    } else {
        format!("{}: {detail}", token.as_str())
    }
}

/// Splits a message back into its token and detail; `None` when it starts with no token.
#[must_use]
pub fn parse_message(message: &str) -> Option<(Token, &str)> {
    let (head, detail) = match message.split_once(": ") {
        Some((head, detail)) => (head, detail),
        None => (message, ""),
    };
    Token::parse(head).map(|token| (token, detail))
}

/// What `nft` was asked to do when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NftOp {
    /// `create table` in the bootstrap.
    CreateTable,
    /// Another bootstrap line.
    Bootstrap,
    /// `create element`, step one.
    CreateElement,
    /// The confirm batch.
    Confirm,
    /// The rollback batch or its fallback.
    Rollback,
    /// An exclusion rewrite.
    ExclusionRewrite,
    /// A JSON read.
    Read,
}

/// The class `nft`'s text puts a failure in, before the table check decides what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StderrClass {
    /// The set is full.
    SetFull,
    /// The element is not in the set; see [`classify_absent`].
    ElementAbsent,
    /// Bare `No such file or directory`: the table or a set is gone. Never "absent".
    TableOrSetMissing,
    /// `File exists` on `create element`.
    AlreadyPresent,
    /// `File exists` on `create table`.
    TableExistsRace,
    /// An interval overlaps an existing one.
    IntervalOverlap,
    /// The table differs from the template.
    TableDrift,
    /// `nft` rejected our template.
    InternalTemplateError,
    /// A target `nft` should never have seen.
    InvalidTarget,
    /// Out of memory.
    ResourceExhausted,
    /// No `filter` chain support; drift if the table exists on a re-read.
    ChainUnsupported,
    /// Not permitted.
    PermissionDenied,
    /// No row matched.
    Unknown,
}

/// Classifies `stderr` of a failed `nft` call by the operation and the text.
#[must_use]
pub fn classify_stderr(op: NftOp, stderr: &str) -> StderrClass {
    let rule = stderr.contains("Could not process rule");
    if stderr.contains("Too many open files in system") {
        return StderrClass::SetFull;
    }
    if stderr.contains("destroy")
        || stderr.contains("value too large")
        || stderr.contains("wrong time format")
        || stderr.contains("syntax error")
    {
        return StderrClass::InternalTemplateError;
    }
    if stderr.contains("Cannot allocate memory") {
        return StderrClass::ResourceExhausted;
    }
    if stderr.contains("Chain of type \"filter\" is not supported") {
        return StderrClass::ChainUnsupported;
    }
    if stderr.contains("interval overlaps with an existing one") {
        return StderrClass::IntervalOverlap;
    }
    if stderr.contains("element does not exist") || stderr.contains("interval not found in set") {
        return StderrClass::ElementAbsent;
    }
    if stderr.contains("You must add 'flags interval'") {
        return StderrClass::TableDrift;
    }
    if rule && stderr.contains("File exists") {
        return match op {
            NftOp::CreateTable => StderrClass::TableExistsRace,
            NftOp::CreateElement => StderrClass::AlreadyPresent,
            _ => StderrClass::Unknown,
        };
    }
    if rule && stderr.contains("Invalid argument") {
        return StderrClass::TableDrift;
    }
    if stderr.contains("Prefix length") && stderr.contains("is invalid")
        || stderr.contains("Could not resolve hostname")
    {
        return StderrClass::InvalidTarget;
    }
    if stderr.contains("Operation not permitted") {
        return StderrClass::PermissionDenied;
    }
    if rule && stderr.contains("No such file or directory") {
        return if matches!(op, NftOp::Confirm | NftOp::Rollback) {
            StderrClass::ElementAbsent
        } else {
            StderrClass::TableOrSetMissing
        };
    }
    if stderr.contains("No such file or directory") {
        return StderrClass::TableOrSetMissing;
    }
    StderrClass::Unknown
}

impl StderrClass {
    /// The wire token for a class that has one on its own. `table_exists_on_reread` settles
    /// the chain-support case; `rollback` picks the overlap meaning. The absent and missing
    /// classes return `None`: the table check and the membership read decide them.
    #[must_use]
    pub fn token(self, rollback: bool, table_exists_on_reread: bool) -> Option<Token> {
        Some(match self {
            Self::SetFull => Token::SetFull,
            Self::AlreadyPresent => Token::AlreadyPresent,
            Self::TableExistsRace => Token::TableExistsRace,
            Self::IntervalOverlap if rollback => Token::CoveredByOther,
            Self::IntervalOverlap | Self::TableDrift => Token::TableDrift,
            Self::InternalTemplateError => Token::InternalTemplateError,
            Self::InvalidTarget => Token::InvalidTarget,
            Self::ResourceExhausted => Token::ResourceExhausted,
            Self::ChainUnsupported if table_exists_on_reread => Token::TableDrift,
            Self::ChainUnsupported => Token::UnsupportedKernel,
            Self::PermissionDenied => Token::PermissionDenied,
            Self::ElementAbsent | Self::TableOrSetMissing | Self::Unknown => return None,
        })
    }
}

/// What the helper observed after an exit 1 of `confirm` or `rollback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsentFacts {
    /// Whether the table exists.
    pub table_exists: bool,
    /// Whether the JSON membership read holds the exact element.
    pub exact_member: bool,
    /// Whether another element covers the target.
    pub covering_member: bool,
    /// Whether the journal has a `rollback_intent` or `rolled_back` after the apply.
    pub rollback_recorded: bool,
    /// Whether `now >= applied_at + 30 s - tolerance` on the clock port.
    pub past_step1_deadline: bool,
}

/// The decision of 6.7.2: why an element is not there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsentClass {
    /// No table: the block is lost.
    TableMissing,
    /// The element is there after all; re-read once.
    PresentUnexpectedly,
    /// A covering element holds the address.
    CoveredByOther,
    /// The journal says we removed it.
    RolledBack,
    /// The kernel timeout removed it.
    Expired,
    /// Gone without an explanation: lost.
    AbsentUnexplained,
}

impl AbsentClass {
    /// The token that travels; `table_missing` and `absent_unexplained` are internal and
    /// both travel as `containment_lost`.
    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::TableMissing | Self::AbsentUnexplained => Token::ContainmentLost,
            Self::PresentUnexpectedly => Token::PostconditionFailed,
            Self::CoveredByOther => Token::CoveredByOther,
            Self::RolledBack => Token::RolledBack,
            Self::Expired => Token::Expired,
        }
    }
}

/// Decides expired versus rolled back from facts, never from error text.
#[must_use]
pub fn classify_absent(facts: &AbsentFacts) -> AbsentClass {
    if !facts.table_exists {
        AbsentClass::TableMissing
    } else if facts.exact_member {
        AbsentClass::PresentUnexpectedly
    } else if facts.covering_member {
        AbsentClass::CoveredByOther
    } else if facts.rollback_recorded {
        AbsentClass::RolledBack
    } else if facts.past_step1_deadline {
        AbsentClass::Expired
    } else {
        AbsentClass::AbsentUnexplained
    }
}
