//! The checks and repairs applied to every item a source returned, and the dedupe rule. The
//! aggregator trusts no source: layer, source, origin, redaction and bounds are re-checked
//! here, one small pure function per rule.

use domovoi_core::context::{
    ContextGap, ContextItem, ContextLayer, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::Redactor;

/// Longest title and reference, in characters.
const TITLE_CHARS: usize = 120;
/// Longest reason, in characters.
const REASON_CHARS: usize = 200;

/// Cuts `text` to at most `max` characters.
pub(super) fn chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Cuts `text` to at most `max` bytes on a character boundary.
pub(super) fn bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Sets the item's layer and source to the source's own. Returns whether it had to change
/// either, so the caller can log the mismatch.
pub(super) fn restamp(
    item: &mut ContextItem,
    source: &ContextSourceId,
    layer: ContextLayer,
) -> bool {
    let changed = item.source != *source || item.layer != layer;
    item.source = source.clone();
    item.layer = layer;
    changed
}

/// Past operations are evidence about the past: every item of the experience layer is
/// historical, whatever its source claimed.
pub(super) fn force_origin(item: &mut ContextItem) {
    if item.layer == ContextLayer::Experience {
        item.provenance.origin = Origin::Historical;
    }
}

/// Redacts every text field of `item` and cuts it to its bound. Redaction comes first so a
/// cut cannot split a secret.
pub(super) fn redact_and_bound(item: &mut ContextItem, redactor: &dyn Redactor, item_bytes: usize) {
    let title = redactor.redact(&item.title);
    let title = title.trim();
    item.title = if title.is_empty() {
        item.source.to_string()
    } else {
        chars(title, TITLE_CHARS)
    };
    item.provenance.reference = chars(&redactor.redact(&item.provenance.reference), TITLE_CHARS);
    item.provenance.actor = item
        .provenance
        .actor
        .as_deref()
        .map(|actor| chars(&redactor.redact(actor), TITLE_CHARS));
    if let ItemStatus::Unknown { reason } = &mut item.status {
        *reason = chars(&redactor.redact(reason), REASON_CHARS);
    }
    let body = redactor.redact(&item.body);
    if body.len() > item_bytes {
        item.body = bytes(&body, item_bytes).to_owned();
        item.truncated = true;
    } else {
        item.body = body;
    }
}

/// Redacts and bounds a gap's reason.
pub(super) fn gap(gap: &mut ContextGap, source: &ContextSourceId, redactor: &dyn Redactor) {
    gap.source = source.clone();
    gap.reason = chars(&redactor.redact(&gap.reason), REASON_CHARS);
}

/// The kind of a status, ignoring what it carries.
fn kind(status: &ItemStatus) -> u8 {
    match status {
        ItemStatus::Known => 0,
        ItemStatus::Unknown { .. } => 1,
        ItemStatus::Stale { .. } => 2,
        ItemStatus::Disputed { .. } => 3,
    }
}

/// Removes an item that arrived twice (same id and same status kind), keeping the one with
/// the newest observation time (an item with no time is the oldest), or the first on a tie.
/// A kept item stays where it was. A known item and an unknown item with one id are both
/// kept: the model must see that the newest read failed.
pub(super) fn dedupe(items: Vec<ContextItem>) -> Vec<ContextItem> {
    let mut winner: Vec<usize> = (0..items.len()).collect();
    for i in 0..items.len() {
        for j in 0..i {
            if winner[j] == j
                && items[j].id == items[i].id
                && kind(&items[j].status) == kind(&items[i].status)
            {
                if items[i].provenance.observed_at > items[j].provenance.observed_at {
                    winner[j] = i;
                } else {
                    winner[i] = j;
                }
                break;
            }
        }
    }
    // A loser that lost to a loser resolves through the chain above: only winners stay.
    items
        .into_iter()
        .enumerate()
        .filter(|(i, _)| winner[*i] == *i)
        .map(|(_, item)| item)
        .collect()
}
