//! Learned baselines and sliding counter windows, kept as JSON in a check's state.
//!
//! Both keep their size bounded, so a host with churn cannot grow a check's state forever.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use jiff::{SignedDuration, Timestamp};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::framework::CheckState;

/// Consecutive runs a candidate must be seen in before it becomes expected.
pub const LEARN_RUNS: u32 = 2;
/// Most entries a [`Baseline`] keeps.
pub const MAX_BASELINE_ENTRIES: usize = 4096;
/// Most samples a [`CounterWindow`] keeps.
pub const MAX_WINDOW_SAMPLES: usize = 512;

/// Reads `T` from the JSON stored under `key`; missing or unreadable state is the default.
pub(crate) fn load_json<T: DeserializeOwned + Default>(state: &CheckState, key: &str) -> T {
    match state.get(key).map(serde_json::from_str) {
        Some(Ok(value)) => value,
        Some(Err(err)) => {
            tracing::warn!(state_key = key, %err, "discarding unreadable check state");
            T::default()
        }
        None => T::default(),
    }
}

/// Stores `value` as JSON under `key`.
pub(crate) fn save_json<T: Serialize>(state: &mut CheckState, key: &str, value: &T) {
    match serde_json::to_string(value) {
        Ok(json) => state.set(key, json),
        Err(err) => tracing::warn!(state_key = key, %err, "could not save check state"),
    }
}

/// One baseline entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BaselineEntry {
    /// Consecutive runs it has been seen in, up to [`LEARN_RUNS`].
    pub consecutive: u32,
    /// Whether it has been learned.
    pub expected: bool,
    /// The last run it was seen in.
    pub last_seen: Timestamp,
}

/// Names (containers, listeners) learned after being seen in [`LEARN_RUNS`] consecutive
/// runs, and dropped after being gone for the expiry.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Baseline {
    entries: BTreeMap<String, BaselineEntry>,
    bootstrapped: bool,
}

impl Baseline {
    /// The baseline stored under `key`.
    #[must_use]
    pub fn load(state: &CheckState, key: &str) -> Self {
        load_json(state, key)
    }

    /// Stores the baseline under `key`.
    pub fn save(&self, state: &mut CheckState, key: &str) {
        save_json(state, key, self);
    }

    /// Whether [`Baseline::learn_all`] has run once.
    #[must_use]
    pub fn bootstrapped(&self) -> bool {
        self.bootstrapped
    }

    /// Marks everything in `present` as expected at once; used on a check's first run.
    pub fn learn_all<'a>(&mut self, present: impl IntoIterator<Item = &'a str>, now: Timestamp) {
        for name in present {
            if self.entries.len() >= MAX_BASELINE_ENTRIES && !self.entries.contains_key(name) {
                break;
            }
            self.entries.insert(
                name.to_owned(),
                BaselineEntry {
                    consecutive: LEARN_RUNS,
                    expected: true,
                    last_seen: now,
                },
            );
        }
        self.bootstrapped = true;
    }

    /// Records one run: names in `present` count towards learning, candidates that are
    /// absent start over. Returns the names learned in this run.
    pub fn observe<'a>(
        &mut self,
        present: impl IntoIterator<Item = &'a str>,
        now: Timestamp,
    ) -> Vec<String> {
        let present: BTreeSet<&str> = present.into_iter().collect();
        for (name, entry) in &mut self.entries {
            if !present.contains(name.as_str()) {
                entry.consecutive = 0;
            }
        }
        let mut learned = Vec::new();
        for name in present {
            if self.entries.len() >= MAX_BASELINE_ENTRIES && !self.entries.contains_key(name) {
                tracing::warn!(name, "baseline is full; not learning more entries");
                continue;
            }
            let entry = self
                .entries
                .entry(name.to_owned())
                .or_insert(BaselineEntry {
                    consecutive: 0,
                    expected: false,
                    last_seen: now,
                });
            entry.consecutive = entry.consecutive.saturating_add(1).min(LEARN_RUNS);
            entry.last_seen = now;
            if !entry.expected && entry.consecutive >= LEARN_RUNS {
                entry.expected = true;
                learned.push(name.to_owned());
            }
        }
        self.entries
            .retain(|_, entry| entry.expected || entry.consecutive > 0);
        self.bootstrapped = true;
        learned
    }

    /// Drops expected entries not seen for longer than `expiry` and returns their names.
    pub fn expire(&mut self, expiry: SignedDuration, now: Timestamp) -> Vec<String> {
        let mut expired = Vec::new();
        self.entries.retain(|name, entry| {
            let gone = now.duration_since(entry.last_seen) > expiry;
            if gone {
                expired.push(name.clone());
            }
            !gone
        });
        expired
    }

    /// Whether `name` is known at all, learned or still being learned.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Whether `name` has been learned.
    #[must_use]
    pub fn is_expected(&self, name: &str) -> bool {
        self.entries.get(name).is_some_and(|e| e.expected)
    }

    /// The learned entries, in name order.
    pub fn expected(&self) -> impl Iterator<Item = (&str, &BaselineEntry)> {
        self.entries
            .iter()
            .filter(|(_, e)| e.expected)
            .map(|(name, e)| (name.as_str(), e))
    }
}

/// Samples of a monotonic counter over a sliding window, such as restart counts.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct CounterWindow {
    samples: VecDeque<(Timestamp, u64)>,
}

impl CounterWindow {
    /// Adds a sample and drops those older than `window`.
    pub fn record(&mut self, now: Timestamp, value: u64, window: SignedDuration) {
        self.samples.push_back((now, value));
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > window)
        {
            self.samples.pop_front();
        }
        while self.samples.len() > MAX_WINDOW_SAMPLES {
            self.samples.pop_front();
        }
    }

    /// How much the counter grew across the window. A decrease is a reset (a recreated
    /// container, a restarted daemon), so the new value counts from zero.
    #[must_use]
    pub fn increase(&self) -> u64 {
        let mut total: u64 = 0;
        let mut previous: Option<u64> = None;
        for &(_, value) in &self.samples {
            if let Some(prev) = previous {
                let delta = if value >= prev { value - prev } else { value };
                total = total.saturating_add(delta);
            }
            previous = Some(value);
        }
        total
    }
}

/// Named counter windows stored together under one state key.
pub(crate) type Windows = BTreeMap<String, CounterWindow>;
