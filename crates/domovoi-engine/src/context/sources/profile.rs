//! The normal state of the host, from the profile text the hub keeps in its settings. It is
//! not a note: a person's notes stay theirs.

use async_trait::async_trait;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::ContextSource;
use jiff::SignedDuration;

use super::{Maker, Spec, host_of};
use crate::triage::TriageDeps;

/// What the hub stores under [`domovoi_core::profile_text_key`].
#[derive(serde::Deserialize)]
struct StoredText {
    markdown: String,
    collected_at: jiff::Timestamp,
}

/// The generated note puts the raw tool output after this heading, and only the part before
/// it is sent: the summary and the recent changes.
const HUB_DETAILS_MARK: &str = "\n## Details";

/// The heading the same raw output sat behind in a note an installation wrote before the hub
/// changed its note. Until that note is refreshed it is still on the hub, so the older
/// heading cuts the summary too.
const LEGACY_DETAILS_MARK: &str = "\n## What the tools said";

/// The hub refreshes a profile at least every seven days; one day of slack before it is
/// called stale.
const STALE_AFTER: SignedDuration = SignedDuration::from_hours(8 * 24);

/// The profile note of the host, as a known item or a stale one.
#[derive(Clone)]
pub(crate) struct ProfileSource {
    deps: TriageDeps,
}

impl ProfileSource {
    /// A source over the note repository.
    pub(crate) fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }
}

/// The note text up to the first heading that starts raw tool output, in the hub's current
/// note or in the older heading a note that has not been refreshed yet still carries.
fn note_summary(body: &str) -> String {
    let at = [HUB_DETAILS_MARK, LEGACY_DETAILS_MARK]
        .iter()
        .map(|mark| body.find(mark).unwrap_or(body.len()))
        .min()
        .unwrap_or(body.len());
    body[..at].to_owned()
}

#[async_trait]
impl ContextSource for ProfileSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("profile")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Host
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let maker = Maker::new(
            "profile",
            ContextLayer::Host,
            self.deps.redactor.as_ref(),
            query,
        );
        let now = self.deps.clock.now();
        if maker.expired(now) {
            return maker.gap_batch("timed out");
        }
        let stored = match self
            .deps
            .settings
            .get(&domovoi_core::profile_text_key(host_of(query)))
            .await
        {
            Ok(stored) => stored,
            Err(_) => return maker.gap_batch("the host profile could not be read"),
        };
        let Some(text) = stored.and_then(|value| serde_json::from_value::<StoredText>(value).ok())
        else {
            return ContextBatch::default();
        };
        let stale = now
            .checked_sub(STALE_AFTER)
            .is_ok_and(|limit| text.collected_at <= limit);
        let status = if stale {
            ItemStatus::Stale {
                observed_at: text.collected_at,
            }
        } else {
            ItemStatus::Known
        };
        ContextBatch {
            items: vec![maker.item(Spec {
                key: "profile",
                title: "Host profile (the normal state of this host)",
                body: &note_summary(text.markdown.trim_end()),
                status,
                origin: Origin::Observed,
                reference: "host profile",
                observed_at: Some(text.collected_at),
            })],
            gaps: Vec::new(),
        }
    }
}
