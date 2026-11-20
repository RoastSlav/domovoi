//! Per-layer caps and the total cap. Nothing is dropped silently: every item a cap removes
//! is counted in an [`Omission`], and the renderer names it.

use domovoi_core::context::{ContextItem, ContextLayer, ContextSourceId, ItemStatus};

use super::layout::{Omission, Section};
use super::normalize::bytes;
use super::plan::{LayerCaps, MIN_ITEM_BODY};
use super::render::{render_all, render_section};

/// The layer to give up first comes first.
const SHRINK_ORDER: [ContextLayer; 6] = [
    ContextLayer::Experience,
    ContextLayer::Knowledge,
    ContextLayer::Evidence,
    ContextLayer::Host,
    ContextLayer::Capabilities,
    ContextLayer::Incident,
];

/// Which items a drop may take.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    /// Items that are not unknown, except the incident itself.
    Known,
    /// Unknown items, the last thing to give up.
    Unknown,
}

/// Applies the layer caps and then the total cap to `sections`, and returns them with the
/// omissions (`omissions` holds what the per-query limits already left out).
pub(super) fn shrink(
    sections: Vec<Section>,
    omissions: Vec<Omission>,
    caps: LayerCaps,
    total: usize,
) -> (Vec<Section>, Vec<Omission>) {
    let empty_at_start = sections
        .iter()
        .filter(|s| s.items.is_empty() && s.gaps.is_empty())
        .map(|s| s.source.clone())
        .collect();
    let mut shrinker = Shrinker {
        sections,
        omissions,
        empty_at_start,
    };
    for layer in [
        ContextLayer::Incident,
        ContextLayer::Host,
        ContextLayer::Evidence,
        ContextLayer::Experience,
        ContextLayer::Knowledge,
        ContextLayer::Capabilities,
    ] {
        shrinker.fit_layer(layer, caps.of(layer));
    }
    shrinker.fit_total(total);
    shrinker.omissions.sort_by_key(|o| o.layer);
    (shrinker.sections, shrinker.omissions)
}

struct Shrinker {
    sections: Vec<Section>,
    omissions: Vec<Omission>,
    /// Sections that had nothing to show before any cap: they keep their `None.`.
    empty_at_start: Vec<ContextSourceId>,
}

impl Shrinker {
    fn layer_len(&self, layer: ContextLayer) -> usize {
        self.sections
            .iter()
            .filter(|s| s.layer == layer)
            .map(|s| render_section(s).len())
            .sum()
    }

    fn total_len(&self) -> usize {
        render_all(&self.sections, &self.omissions).len()
    }

    fn item_count(&self, layer: ContextLayer) -> usize {
        self.sections
            .iter()
            .filter(|s| s.layer == layer)
            .map(|s| s.items.len())
            .sum()
    }

    /// The incident itself: the first item of the first incident-layer section that has one.
    fn protected(&self) -> Option<&ContextItem> {
        self.sections
            .iter()
            .filter(|s| s.layer == ContextLayer::Incident)
            .find_map(|s| s.items.first())
    }

    fn note(&mut self, layer: ContextLayer, source: &ContextSourceId) {
        if let Some(found) = self
            .omissions
            .iter_mut()
            .find(|o| o.layer == layer && o.source == *source)
        {
            found.items += 1;
        } else {
            self.omissions.push(Omission {
                layer,
                source: source.clone(),
                items: 1,
            });
        }
    }

    /// Removes the last item of `layer` that `take` allows, and the partner of a disputed
    /// pair in the same layer. Returns whether anything was removed.
    fn drop_last(&mut self, layer: ContextLayer, take: Take) -> bool {
        let protected = self.protected().map(|item| item.id.clone());
        let protected_status = self.protected().map(|item| item.status.clone());
        // A disputed pair is one unit: an item in a pair with the protected incident item
        // cannot go, or the incident would point at something no longer shown.
        let pairs_with_protected = |item: &ContextItem| {
            let with_it = matches!(&item.status, ItemStatus::Disputed { with } if Some(with) == protected.as_ref());
            let it_with = matches!(&protected_status, Some(ItemStatus::Disputed { with }) if *with == item.id);
            with_it || it_with
        };
        let mut found = None;
        'search: for (si, section) in self.sections.iter().enumerate().rev() {
            if section.layer != layer {
                continue;
            }
            for (ii, item) in section.items.iter().enumerate().rev() {
                let unknown = matches!(item.status, ItemStatus::Unknown { .. });
                let ok = match take {
                    Take::Known => {
                        !unknown
                            && Some(&item.id) != protected.as_ref()
                            && !pairs_with_protected(item)
                    }
                    Take::Unknown => unknown,
                };
                if ok {
                    found = Some((si, ii));
                    break 'search;
                }
            }
        }
        let Some((si, ii)) = found else {
            return false;
        };
        let victim = self.sections[si].items.remove(ii);
        let source = self.sections[si].source.clone();
        self.note(layer, &source);
        let with = match &victim.status {
            ItemStatus::Disputed { with } => Some(with.clone()),
            _ => None,
        };
        let mut partners: Vec<ContextSourceId> = Vec::new();
        for section in self.sections.iter_mut().filter(|s| s.layer == layer) {
            let before = section.items.len();
            section.items.retain(|item| {
                let partner = with.as_ref() == Some(&item.id)
                    || matches!(&item.status, ItemStatus::Disputed { with } if *with == victim.id);
                !(partner && Some(&item.id) != protected.as_ref())
            });
            for _ in section.items.len()..before {
                partners.push(section.source.clone());
            }
        }
        for source in partners {
            self.note(layer, &source);
        }
        let originally_empty = &self.empty_at_start;
        self.sections.retain(|s| {
            !s.items.is_empty() || !s.gaps.is_empty() || originally_empty.contains(&s.source)
        });
        true
    }

    /// Cuts the longest body of `layer` by `excess` bytes, never below the floor.
    fn cut_longest(&mut self, layer: ContextLayer, excess: usize) -> bool {
        let mut longest: Option<&mut ContextItem> = None;
        for item in self
            .sections
            .iter_mut()
            .filter(|s| s.layer == layer)
            .flat_map(|s| s.items.iter_mut())
        {
            if item.body.len() > MIN_ITEM_BODY
                && longest
                    .as_ref()
                    .is_none_or(|l| item.body.len() > l.body.len())
            {
                longest = Some(item);
            }
        }
        let Some(item) = longest else {
            return false;
        };
        let keep = item.body.len().saturating_sub(excess).max(MIN_ITEM_BODY);
        item.body = bytes(&item.body, keep).to_owned();
        item.truncated = true;
        true
    }

    fn fit_layer(&mut self, layer: ContextLayer, cap: usize) {
        while self.layer_len(layer) > cap && self.item_count(layer) > 1 {
            if !self.drop_last(layer, Take::Known) {
                break;
            }
        }
        loop {
            let len = self.layer_len(layer);
            if len <= cap || !self.cut_longest(layer, len - cap) {
                break;
            }
        }
    }

    fn fit_total(&mut self, total: usize) {
        while self.total_len() > total {
            if !SHRINK_ORDER.iter().any(|l| self.drop_last(*l, Take::Known)) {
                break;
            }
        }
        while self.total_len() > total {
            if !SHRINK_ORDER
                .iter()
                .any(|l| self.drop_last(*l, Take::Unknown))
            {
                break;
            }
        }
        if self.total_len() > total {
            let excess = self.total_len() - total;
            let incident = self
                .sections
                .iter_mut()
                .filter(|s| s.layer == ContextLayer::Incident)
                .find_map(|s| s.items.first_mut());
            if let Some(item) = incident {
                let keep = item.body.len().saturating_sub(excess).max(MIN_ITEM_BODY);
                item.body = bytes(&item.body, keep).to_owned();
                item.truncated = true;
            }
        }
    }
}
