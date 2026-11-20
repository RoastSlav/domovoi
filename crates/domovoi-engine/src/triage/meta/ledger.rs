//! What the model has already been shown in one conversation, so identical read output is
//! not sent twice.

use std::collections::BTreeMap;

use domovoi_core::context::ContextItemId;
use jiff::Timestamp;
use sha2::{Digest, Sha256};

/// Output shorter than this is always returned in full: a note would not be shorter.
pub(crate) const MIN_LEDGER_BYTES: usize = 256;

/// Where and when the model first saw some output.
struct Seen {
    call_number: u32,
    taken_at: Timestamp,
    fingerprint: [u8; 32],
}

/// The reads the model has been shown in one conversation, by read.
#[derive(Default)]
pub(crate) struct ReadLedger {
    seen: BTreeMap<ContextItemId, Seen>,
}

impl ReadLedger {
    /// An empty ledger.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Records a successful read and returns the note to show instead of `text` when an
    /// earlier read of the same call returned byte-identical text. `None` for the first
    /// read, for changed text and for short text.
    pub(crate) fn check(
        &mut self,
        id: &ContextItemId,
        text: &str,
        call_number: u32,
        now: Timestamp,
    ) -> Option<String> {
        if text.len() < MIN_LEDGER_BYTES {
            return None;
        }
        let fingerprint: [u8; 32] = Sha256::digest(text.as_bytes()).into();
        if let Some(earlier) = self.seen.get(id)
            && earlier.fingerprint == fingerprint
        {
            return Some(format!(
                "Same output as your earlier read (tool call {}, taken {}); nothing changed, so it is not repeated.",
                earlier.call_number, earlier.taken_at
            ));
        }
        self.seen.insert(
            id.clone(),
            Seen {
                call_number,
                taken_at: now,
                fingerprint,
            },
        );
        None
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::context::{ContextItemId, ContextSourceId};
    use jiff::Timestamp;

    use super::ReadLedger;

    fn id(key: &str) -> ContextItemId {
        ContextItemId::new(&ContextSourceId::evidence(), key)
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("a valid time")
    }

    #[test]
    fn a_first_read_is_never_a_note() {
        let mut ledger = ReadLedger::new();
        assert_eq!(ledger.check(&id("a"), &"x".repeat(300), 1, at(0)), None);
    }

    #[test]
    fn short_output_is_never_deduped() {
        let mut ledger = ReadLedger::new();
        assert_eq!(ledger.check(&id("a"), "short", 1, at(0)), None);
        assert_eq!(ledger.check(&id("a"), "short", 2, at(1)), None);
    }

    #[test]
    fn identical_output_gets_the_fixed_note_pointing_at_the_first_read() {
        let mut ledger = ReadLedger::new();
        let text = "y".repeat(300);
        assert_eq!(ledger.check(&id("a"), &text, 1, at(0)), None);
        let note = ledger.check(&id("a"), &text, 3, at(60));
        assert_eq!(
            note.as_deref(),
            Some(
                "Same output as your earlier read (tool call 1, taken 1970-01-01T00:00:00Z); nothing changed, so it is not repeated."
            )
        );
        // The note keeps pointing at the first read.
        let again = ledger.check(&id("a"), &text, 4, at(120));
        assert_eq!(again, note);
    }

    #[test]
    fn changed_output_replaces_the_entry() {
        let mut ledger = ReadLedger::new();
        let first = "a".repeat(300);
        let second = "b".repeat(300);
        assert_eq!(ledger.check(&id("a"), &first, 1, at(0)), None);
        assert_eq!(ledger.check(&id("a"), &second, 2, at(1)), None);
        let note = ledger.check(&id("a"), &second, 3, at(2));
        assert!(note.is_some_and(|n| n.contains("tool call 2")));
    }

    #[test]
    fn different_reads_do_not_share_an_entry() {
        let mut ledger = ReadLedger::new();
        let text = "z".repeat(300);
        assert_eq!(ledger.check(&id("a"), &text, 1, at(0)), None);
        assert_eq!(ledger.check(&id("b"), &text, 2, at(1)), None);
    }
}
