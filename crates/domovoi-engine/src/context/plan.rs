//! What one aggregation is asked to build, and the size limits it works under.

use domovoi_core::context::{ContextLayer, ContextScope};
use jiff::SignedDuration;

/// Longest body of one item, in bytes.
pub const ITEM_BYTES: u32 = 4096;
/// A body is never cut below this length by a cap, in bytes.
pub const MIN_ITEM_BODY: usize = 256;
/// Smallest total context, in bytes.
pub const TOTAL_MIN: usize = 8 * 1024;
/// Room left for the text the investigation handler appends after the context is built.
pub const APPENDED_RESERVE: usize = 2 * 1024;
/// Most items asked of a source for an overview.
pub const OVERVIEW_LIMIT: u16 = 10;
/// Most items asked of a source for a search.
pub const SEARCH_LIMIT: u16 = 5;
/// Longest a source may take to answer one query.
pub const SOURCE_DEADLINE: SignedDuration = SignedDuration::from_secs(5);

/// Which call the context is for. It chooses the built-in sources and the size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPurpose {
    /// The first turn of an investigation.
    Investigate,
    /// A follow-up question about an incident.
    FollowUp,
    /// A chat about one host.
    Chat,
}

impl ContextPurpose {
    /// Largest total context for this purpose, in bytes.
    const fn total_max(self) -> usize {
        match self {
            Self::Investigate => 32 * 1024,
            Self::FollowUp => 40 * 1024,
            Self::Chat => 24 * 1024,
        }
    }

    /// Bytes kept free for text added after the context is built.
    const fn reserve(self) -> usize {
        match self {
            Self::Investigate => APPENDED_RESERVE,
            Self::FollowUp | Self::Chat => 0,
        }
    }
}

/// What one aggregation is asked to build.
#[derive(Debug, Clone)]
pub struct ContextPlan {
    /// Which call the context is for.
    pub purpose: ContextPurpose,
    /// `Incident` for an investigation or follow-up (an operation uses its first incident);
    /// `Host` for a chat.
    pub scope: ContextScope,
    /// Text for a search: only for an incident scope, already redacted, 1 to 200 characters.
    pub search_text: Option<String>,
    /// The size limits and per-query bounds.
    pub budget: ContextBudget,
}

/// Size limits and per-query bounds of one aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudget {
    /// Cap on the rendered text, in bytes.
    pub total_bytes: usize,
    /// Cap per layer, in bytes.
    pub layers: LayerCaps,
    /// Longest body of one item, in bytes.
    pub item_bytes: u32,
    /// Items asked of a source for an overview.
    pub overview_limit: u16,
    /// Items asked of a source for a search.
    pub search_limit: u16,
    /// Longest a source may take to answer one query.
    pub source_deadline: SignedDuration,
}

impl ContextBudget {
    /// The limits for `purpose` under a per-call prompt cap of `max_prompt_bytes`: a quarter
    /// of the prompt cap, clamped, so the opening message that the prompt cap never trims
    /// stays small by construction.
    #[must_use]
    pub fn for_purpose(purpose: ContextPurpose, max_prompt_bytes: usize) -> Self {
        let total = (max_prompt_bytes / 4).clamp(TOTAL_MIN, purpose.total_max());
        Self {
            total_bytes: total - purpose.reserve(),
            layers: LayerCaps::DEFAULT,
            item_bytes: ITEM_BYTES,
            overview_limit: OVERVIEW_LIMIT,
            search_limit: SEARCH_LIMIT,
            source_deadline: SOURCE_DEADLINE,
        }
    }
}

/// How many bytes each layer may take in the rendered text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerCaps {
    /// The incident layer.
    pub incident: usize,
    /// The host layer.
    pub host: usize,
    /// The evidence layer.
    pub evidence: usize,
    /// The experience layer.
    pub experience: usize,
    /// The knowledge layer.
    pub knowledge: usize,
    /// The capabilities layer.
    pub capabilities: usize,
}

impl LayerCaps {
    /// The caps every purpose uses.
    pub const DEFAULT: Self = Self {
        incident: 16 * 1024,
        host: 16 * 1024,
        evidence: 8 * 1024,
        experience: 4 * 1024,
        knowledge: 8 * 1024,
        capabilities: 6 * 1024,
    };

    /// The cap of `layer`.
    #[must_use]
    pub const fn of(&self, layer: ContextLayer) -> usize {
        match layer {
            ContextLayer::Incident => self.incident,
            ContextLayer::Host => self.host,
            ContextLayer::Evidence => self.evidence,
            ContextLayer::Experience => self.experience,
            ContextLayer::Knowledge => self.knowledge,
            ContextLayer::Capabilities => self.capabilities,
        }
    }
}
