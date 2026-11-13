//! Walking a chain of stored rows and reporting what it proves.

use std::fmt;

use super::{AuditAnchor, ChainRow, Hash, entry_hash};

/// Where verification starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStart {
    /// The first row must have `prev_hash == anchor_hash` and `seq == first_seq`.
    Anchor {
        /// The anchor's hash, which the first row must name.
        anchor_hash: Hash,
        /// The sequence number the first row must have.
        first_seq: u64,
    },
    /// Verify from a row in the middle: the first row's own `prev_hash` is accepted as given.
    Trusted,
}

/// Why the chain does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakReason {
    /// A row after the anchor has no `prev_hash` or `hash`.
    MissingChainFields,
    /// The recomputed hash differs from the stored hash.
    HashMismatch,
    /// `prev_hash` differs from the previous row's hash (or the anchor's).
    LinkMismatch,
    /// The sequence numbers are not consecutive.
    SequenceGap,
    /// The retained start does not match the newest checkpoint.
    CheckpointMismatch,
}

impl fmt::Display for BreakReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::MissingChainFields => "a row has no hash chain fields",
            Self::HashMismatch => "a row was edited",
            Self::LinkMismatch => "a row was removed or inserted",
            Self::SequenceGap => "rows are missing",
            Self::CheckpointMismatch => "the retained start does not match the checkpoint",
        };
        f.write_str(text)
    }
}

/// The first row that does not hold the chain together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainBreak {
    /// The sequence number of the row where verification stopped.
    pub seq: u64,
    /// What was wrong.
    pub reason: BreakReason,
}

/// Walks rows in ascending sequence order and checks links, sequence and hashes.
#[derive(Debug, Clone)]
pub struct ChainVerifier {
    start: ChainStart,
    previous: Option<(u64, Hash)>,
    checked: u64,
}

impl ChainVerifier {
    /// A verifier for one chain start.
    #[must_use]
    pub fn new(start: ChainStart) -> Self {
        Self {
            start,
            previous: None,
            checked: 0,
        }
    }

    /// Feeds rows in ascending `seq`. Stops being useful after the first `Err`.
    ///
    /// Checks, in this order: chain fields present, `seq` consecutive (once a previous row or
    /// the anchor's `first_seq` is known), `prev_hash` linked (skipped for the first row of a
    /// [`ChainStart::Trusted`] start), and the recomputed hash equal to the stored hash.
    pub fn push(&mut self, row: &ChainRow) -> Result<(), ChainBreak> {
        let (prev_hash, hash) = match (row.prev_hash, row.hash) {
            (Some(prev), Some(hash)) => (prev, hash),
            _ => {
                return Err(ChainBreak {
                    seq: row.seq,
                    reason: BreakReason::MissingChainFields,
                });
            }
        };

        let expected_seq = match (&self.previous, &self.start) {
            (Some((seq, _)), _) => Some(seq.saturating_add(1)),
            (None, ChainStart::Anchor { first_seq, .. }) => Some(*first_seq),
            (None, ChainStart::Trusted) => None,
        };
        if expected_seq.is_some_and(|expected| row.seq != expected) {
            return Err(ChainBreak {
                seq: row.seq,
                reason: BreakReason::SequenceGap,
            });
        }

        let expected_link = match (&self.previous, &self.start) {
            (Some((_, previous_hash)), _) => Some(*previous_hash),
            (None, ChainStart::Anchor { anchor_hash, .. }) => Some(*anchor_hash),
            (None, ChainStart::Trusted) => None,
        };
        if expected_link.is_some_and(|expected| prev_hash != expected) {
            return Err(ChainBreak {
                seq: row.seq,
                reason: BreakReason::LinkMismatch,
            });
        }

        if entry_hash(row.seq, &prev_hash, &row.event) != hash {
            return Err(ChainBreak {
                seq: row.seq,
                reason: BreakReason::HashMismatch,
            });
        }

        self.previous = Some((row.seq, hash));
        self.checked += 1;
        Ok(())
    }

    /// How many rows have been accepted.
    #[must_use]
    pub fn checked(&self) -> u64 {
        self.checked
    }

    /// The newest accepted row's sequence number and hash.
    #[must_use]
    pub fn head(&self) -> Option<(u64, Hash)> {
        self.previous
    }
}

/// What verification says about the rows written before the chain existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyStatus {
    /// The count and digest match the anchor.
    Verified {
        /// How many legacy rows were read.
        count: u64,
    },
    /// The legacy history no longer matches the anchor.
    Mismatch {
        /// The count the anchor committed to.
        expected_count: u64,
        /// The count found now.
        actual_count: u64,
        /// Whether the digest still matches.
        digest_matches: bool,
    },
    /// A bounded check counted the rows but did not compute the digest.
    CountOnly {
        /// How many legacy rows were counted.
        count: u64,
    },
    /// Legacy rows were pruned, so the digest can no longer be recomputed.
    NotVerifiable,
}

/// What verification says about the chained rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainStatus {
    /// There are no chained rows yet.
    Empty,
    /// Every checked row links and hashes correctly.
    Intact {
        /// How many rows were checked.
        checked: u64,
        /// The newest checked row's sequence number.
        head_seq: u64,
        /// The newest checked row's hash.
        head_hash: Hash,
    },
    /// Verification stopped at the first row that does not hold the chain together.
    Broken {
        /// How many rows were accepted before the break.
        checked: u64,
        /// The sequence number of the first broken row.
        first_broken_seq: u64,
        /// What was wrong with it.
        reason: BreakReason,
    },
}

/// Everything one verification found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// The legacy history's status.
    pub legacy: LegacyStatus,
    /// The chained history's status.
    pub chained: ChainStatus,
    /// The anchor both parts are judged against.
    pub anchor: AuditAnchor,
}

/// How much history a verification checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyScope {
    /// Check the legacy digest and every retained chained row.
    Full,
    /// Count the legacy rows and check the newest `n` chained rows.
    Last(u64),
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use serde_json::Value;

    use super::*;
    use crate::ports::repos::AuditEvent;

    fn anchor_hash() -> Hash {
        [7u8; 32]
    }

    fn event(kind: &str) -> AuditEvent {
        AuditEvent {
            at: Timestamp::from_millisecond(1_700_000_000_000).unwrap(),
            actor: "system:test".to_owned(),
            kind: kind.to_owned(),
            subject: "subject".to_owned(),
            detail: Value::Null,
        }
    }

    fn chain(length: u64) -> Vec<ChainRow> {
        let mut rows = Vec::new();
        let mut prev = anchor_hash();
        for seq in 1..=length {
            let event = event(&format!("test.{seq}"));
            let hash = entry_hash(seq, &prev, &event);
            rows.push(ChainRow {
                seq,
                prev_hash: Some(prev),
                hash: Some(hash),
                event,
            });
            prev = hash;
        }
        rows
    }

    fn verify(rows: &[ChainRow]) -> ChainVerifier {
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: anchor_hash(),
            first_seq: 1,
        });
        for row in rows {
            verifier.push(row).unwrap();
        }
        verifier
    }

    #[test]
    fn verifier_accepts_a_valid_chain_from_the_anchor() {
        let rows = chain(4);
        let verifier = verify(&rows);
        assert_eq!(verifier.checked(), 4);
        assert_eq!(verifier.head(), Some((4, rows[3].hash.unwrap())));
    }

    #[test]
    fn verifier_detects_an_edited_row() {
        let mut rows = chain(3);
        rows[1].event.detail = serde_json::json!({"edited": true});
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: anchor_hash(),
            first_seq: 1,
        });
        verifier.push(&rows[0]).unwrap();
        assert_eq!(
            verifier.push(&rows[1]),
            Err(ChainBreak {
                seq: 2,
                reason: BreakReason::HashMismatch,
            })
        );
    }

    #[test]
    fn verifier_detects_a_removed_row() {
        let rows = chain(4);
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: anchor_hash(),
            first_seq: 1,
        });
        verifier.push(&rows[0]).unwrap();
        assert_eq!(
            verifier.push(&rows[2]),
            Err(ChainBreak {
                seq: 3,
                reason: BreakReason::SequenceGap,
            })
        );
    }

    #[test]
    fn verifier_detects_a_forged_link() {
        let mut rows = chain(3);
        rows[2].prev_hash = Some([9u8; 32]);
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: anchor_hash(),
            first_seq: 1,
        });
        verifier.push(&rows[0]).unwrap();
        verifier.push(&rows[1]).unwrap();
        assert_eq!(
            verifier.push(&rows[2]),
            Err(ChainBreak {
                seq: 3,
                reason: BreakReason::LinkMismatch,
            })
        );
    }

    #[test]
    fn verifier_rejects_a_row_without_chain_fields() {
        let mut rows = chain(2);
        rows[1].hash = None;
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: anchor_hash(),
            first_seq: 1,
        });
        verifier.push(&rows[0]).unwrap();
        assert_eq!(
            verifier.push(&rows[1]),
            Err(ChainBreak {
                seq: 2,
                reason: BreakReason::MissingChainFields,
            })
        );
    }

    #[test]
    fn trusted_start_accepts_a_window_in_the_middle() {
        let rows = chain(6);
        let mut verifier = ChainVerifier::new(ChainStart::Trusted);
        for row in &rows[3..] {
            verifier.push(row).unwrap();
        }
        assert_eq!(verifier.checked(), 3);
        assert_eq!(verifier.head(), Some((6, rows[5].hash.unwrap())));
    }

    #[test]
    fn anchor_start_rejects_a_first_row_that_does_not_link_to_the_anchor() {
        let rows = chain(1);
        let mut verifier = ChainVerifier::new(ChainStart::Anchor {
            anchor_hash: [8u8; 32],
            first_seq: 1,
        });
        assert_eq!(
            verifier.push(&rows[0]),
            Err(ChainBreak {
                seq: 1,
                reason: BreakReason::LinkMismatch,
            })
        );
    }
}
