//! The verification probe after `apply` and `confirm`.
//!
//! (a) the element is in its set with time left in `(0, T]`; (b) the chain has the template's
//! hook, priority and rules in order; (c) the arrival counter of the first rule did not go
//! backwards. A failure of (a) or (b) is `postcondition_failed` and the caller rolls back; (c)
//! alone is reported as `chain_ok=false`. The hub's round trip is the fourth check. The helper
//! cannot send a packet from the blocked source: packet effectiveness is the self-test's job.

use domovoi_core::containment::templates::SetName;
use domovoi_core::containment::{Layout, Net};

use super::ensure::chain_drift;
use super::view::TableView;

/// The outcome of one probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// (a): the element is present with time left in range.
    pub element_ok: bool,
    /// (b): the chain and its rules match the template.
    pub chain_template_ok: bool,
    /// (c): the arrival counter did not decrease.
    pub counter_ok: bool,
    /// The arrival counter now.
    pub arrivals: u64,
    /// The seconds the element has left.
    pub remaining: Option<i64>,
}

impl Probe {
    /// Whether (a) and (b) hold; the block may stay.
    #[must_use]
    pub const fn passes(&self) -> bool {
        self.element_ok && self.chain_template_ok
    }
}

/// Reads the probe from `view`, taken after the call. `upper` is the longest time the element
/// may have left; `arrivals_before` is the counter read before the call.
#[must_use]
pub fn probe(
    view: &TableView,
    layout: Layout,
    net: &Net,
    upper: i64,
    arrivals_before: Option<u64>,
) -> Probe {
    let set = SetName::for_net(net).as_str();
    let element = view
        .sets
        .get(set)
        .and_then(|s| s.members.iter().find(|m| m.is(net)));
    let remaining = element.and_then(|m| m.expires);
    let element_ok = remaining.is_some_and(|r| r > 0 && r <= upper);
    let chain_template_ok = chain_drift(view, layout).is_none();
    let arrivals = view.counters.first().copied().unwrap_or(0);
    let counter_ok = arrivals_before.is_none_or(|before| arrivals >= before);
    Probe {
        element_ok,
        chain_template_ok,
        counter_ok,
        arrivals,
        remaining,
    }
}
