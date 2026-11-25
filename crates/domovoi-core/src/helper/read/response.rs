//! The failure detail of a read answer: short, printable and free of host paths.

use super::token::ReadToken;

/// The most bytes a failure detail may hold.
pub const DETAIL_MAX_BYTES: usize = 200;

/// A failed read: a token and an optional cleaned detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFailure {
    /// Why it failed.
    pub token: ReadToken,
    /// The first line of what the tool said, cleaned by [`ReadFailure::clean_detail`].
    pub detail: Option<String>,
}

impl ReadFailure {
    /// A failure without a detail.
    #[must_use]
    pub const fn bare(token: ReadToken) -> Self {
        Self {
            token,
            detail: None,
        }
    }

    /// A failure with `raw` cleaned into a detail.
    #[must_use]
    pub fn with_detail(token: ReadToken, raw: &str) -> Self {
        Self {
            token,
            detail: Self::clean_detail(raw),
        }
    }

    /// Keeps the first line, replaces every byte outside printable ASCII with `?`, replaces any
    /// word that is a path outside `/dev` with `?`, and cuts at [`DETAIL_MAX_BYTES`]. Empty
    /// input gives `None`.
    #[must_use]
    pub fn clean_detail(raw: &str) -> Option<String> {
        let line = raw.lines().next().unwrap_or("").trim();
        let ascii: String = line
            .chars()
            .map(|c| {
                if c.is_ascii_graphic() || c == ' ' {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        let cleaned = ascii
            .split(' ')
            .map(|word| {
                if word.contains('/') && !word.starts_with("/dev/") {
                    "?"
                } else {
                    word
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        let cut: String = cleaned.chars().take(DETAIL_MAX_BYTES).collect();
        if cut.is_empty() { None } else { Some(cut) }
    }
}
