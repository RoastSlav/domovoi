//! What a host's agent actually implements, as the hub read it from `get_catalog` pages.
//!
//! The catalogue is the agent's implementation list: contract ids and versions only. It never
//! describes a contract, and the hub never builds one from it (RW-07): a contract exists because
//! `domovoi-core` compiled it in. A snapshot says how completely the hub has read that list, and
//! an unread or half-read list proves nothing about what is missing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::ids::{ContractId, ContractVersion};
use crate::id::HostId;
use crate::sha256::sha256_newtype;
use jiff::Timestamp;

/// One page of the catalogue the hub asks the agent for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogQuery {
    /// Zero-based page number.
    pub page: u32,
    /// Entries per page, clamped to `1..=100` before the request is sent.
    pub page_size: u32,
}

/// One page the agent answered with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPageData {
    /// The entries on this page, in the agent's sorted order.
    pub entries: Vec<CatalogImplementation>,
    /// The next page, when the agent has more to give.
    pub next_page: Option<u32>,
}

/// One implementation the agent reports: an id and a version, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogImplementation {
    /// The contract this agent can run.
    pub contract: ContractId,
    /// The version of that contract it implements.
    pub version: ContractVersion,
}

/// How much of one host's catalogue the hub has read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CatalogueState {
    /// Every page arrived, the cursor ended, one session served the walk and the announced
    /// hash still matches.
    Complete {
        /// The hash the walk matched.
        hash: String,
    },
    /// The walk did not prove the list.
    Partial {
        /// Why it did not.
        gap: CatalogueGap,
    },
    /// The cached snapshot belongs to a hash the agent no longer announces.
    Stale {
        /// What the agent announces now.
        announced: String,
        /// The hash the cached entries were walked under.
        snapshot: String,
    },
}

/// Why one host's catalogue is not verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "gap", rename_all = "snake_case")]
pub enum CatalogueGap {
    /// Nothing has been read yet.
    NotWalked,
    /// The agent announced no catalogue hash.
    HashUnannounced,
    /// The announced hash is not 64 lowercase hex characters.
    HashMalformed,
    /// The agent does not answer a catalogue request at all.
    Unsupported,
    /// A page request failed.
    PageFailed {
        /// Which page failed.
        page: u32,
    },
    /// A page carried an id or version this hub cannot read.
    PageMalformed {
        /// Which page was unreadable.
        page: u32,
    },
    /// The cursor repeated itself or stepped backwards.
    CursorNotAdvancing {
        /// The page where it stopped advancing.
        page: u32,
    },
    /// The walk went past its page bound.
    TooManyPages,
    /// One contract id appeared twice in one walk.
    DuplicateEntry {
        /// The id that appeared twice.
        contract: ContractId,
    },
    /// The host's agent went away mid-walk.
    Disconnected {
        /// How many pages arrived first.
        after_page: u32,
    },
    /// The walk's single deadline passed.
    Deadline,
}

impl CatalogueGap {
    /// The wire code of this gap.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotWalked => "not_walked",
            Self::HashUnannounced => "hash_unannounced",
            Self::HashMalformed => "hash_malformed",
            Self::Unsupported => "unsupported",
            Self::PageFailed { .. } => "page_failed",
            Self::PageMalformed { .. } => "page_malformed",
            Self::CursorNotAdvancing { .. } => "cursor_not_advancing",
            Self::TooManyPages => "too_many_pages",
            Self::DuplicateEntry { .. } => "duplicate_entry",
            Self::Disconnected { .. } => "disconnected",
            Self::Deadline => "deadline",
        }
    }

    /// A short owner-facing sentence about why the list is unverified.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::NotWalked => "this host's implementation list has not been read",
            Self::HashUnannounced => "this host announced no fingerprint for its list",
            Self::HashMalformed => "this host announced an unreadable fingerprint",
            Self::Unsupported => "this host's agent does not report its list",
            Self::PageFailed { .. } => "one page of this list failed to arrive",
            Self::PageMalformed { .. } => "one page of this list could not be read",
            Self::CursorNotAdvancing { .. } => "this list stopped advancing a page",
            Self::TooManyPages => "this list is too large to verify",
            Self::DuplicateEntry { .. } => "this list names one contract twice",
            Self::Disconnected { .. } => "this host's agent left part-way through the list",
            Self::Deadline => "reading this list ran out of time",
        }
    }
}

impl CatalogueState {
    /// Whether the whole list was read and matched the announced hash.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }

    /// The hash the entries were walked under, when there is one.
    #[must_use]
    pub fn hash(&self) -> Option<String> {
        match self {
            Self::Complete { hash } | Self::Stale { snapshot: hash, .. } => Some(hash.clone()),
            Self::Partial { .. } => None,
        }
    }
}

/// One host's catalogue as far as the hub has read it, plus the reading state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    /// The host this snapshot belongs to.
    pub host: HostId,
    /// The hash the agent announced when the walk started, if it announced one.
    pub announced_hash: Option<String>,
    /// How much of the list was read.
    pub state: CatalogueState,
    /// The entries that arrived, by id.
    pub entries: BTreeMap<ContractId, ContractVersion>,
    /// Ids the hub does not know, kept for diagnostics only.
    pub unknown: Vec<String>,
    /// Pages the walk requested.
    pub pages: u32,
    /// The session the walk read. Another session means the snapshot is not reusable.
    pub session_since: Option<Timestamp>,
    /// When the walk finished or failed.
    pub at: Timestamp,
}

sha256_newtype!(
    /// SHA-256 over an implementation list: the agent's own list, fingerprinted.
    ///
    /// The hash both the agent announces in `hello.catalog_hash` and the hub compares
    /// against a walked list, so a walk that matches it read the list the agent runs.
    CatalogHash
);

impl CatalogHash {
    /// Hashes `entries` in the canonical text form: one `id<TAB>version` line per entry,
    /// in the list's order.
    #[must_use]
    pub fn of_entries(entries: &BTreeMap<ContractId, ContractVersion>) -> Self {
        Self::digest(canonical_text(entries).as_bytes())
    }

    /// The 64 lowercase hex characters of this hash.
    #[must_use]
    pub fn hex(&self) -> String {
        format!("{self}")
    }
}

/// The canonical text one implementation list hashes over.
fn canonical_text(entries: &BTreeMap<ContractId, ContractVersion>) -> String {
    let mut text = String::new();
    for (id, version) in entries.iter() {
        text.push_str(&format!("{id}\t{version}\n"));
    }
    text
}

/// The hash of a sorted implementation list, as 64 lowercase hex characters.
///
/// The agent reporting `hello.catalog_hash` and the hub checking a walked list both call
/// this, so the two cannot drift apart about the encoding.
#[must_use]
pub fn catalog_hash(entries: &BTreeMap<ContractId, ContractVersion>) -> String {
    CatalogHash::of_entries(entries).hex()
}

/// Whether `text` is a catalogue hash: 64 lowercase hex characters.
#[must_use]
pub fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
