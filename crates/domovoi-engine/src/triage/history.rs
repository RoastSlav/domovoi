//! The follow-up conversation of one incident read back from its timeline: question and
//! answer turns, and the timeline entry that cuts earlier ones out.

use domovoi_core::ports::IncidentTimelineEntry;
use jiff::Timestamp;

/// The timeline kind a follow-up question and its answer are recorded under.
pub const FOLLOW_UP_KIND: &str = "triage.followup";

/// The timeline kind that cuts earlier questions and answers out of the conversation the
/// model sees. It deliberately does not start with `triage.`, so the incident's latest
/// triage report is still found after a clear.
pub const CLEAR_KIND: &str = "followup.cleared";

/// One question and its answer, as the timeline recorded them.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// What the operator asked.
    pub question: String,
    /// What the model answered.
    pub answer: String,
    /// Who asked, as its user id text.
    pub asked_by: String,
    /// When the answer was recorded.
    pub at: Timestamp,
    /// When the question was asked. Two questions answered out of order are put
    /// back in the order the operator asked them by this.
    pub asked_at: Timestamp,
}

/// One text of a follow-up entry's detail; empty when the detail does not carry it.
fn text(entry: &IncidentTimelineEntry, key: &str) -> String {
    match entry.detail.get(key) {
        Some(found) => found.as_str().unwrap_or_default().to_owned(),
        None => String::new(),
    }
}

/// When the question was asked, as stored in the entry's detail in unix milliseconds.
/// Entries written before this field existed fall back to the entry's own time.
fn asked_at(entry: &IncidentTimelineEntry) -> Timestamp {
    match entry.detail.get("asked_at").and_then(|v| v.as_i64()) {
        Some(ms) => Timestamp::from_millisecond(ms).unwrap_or(entry.at),
        None => entry.at,
    }
}

/// How many clear markers the timeline carries. A follow-up request remembers this
/// number when it starts; the answer is recorded only while the count still matches,
/// so a clear that lands mid-request wins and the answer is dropped.
pub fn count_clear(timeline: &[IncidentTimelineEntry]) -> u32 {
    let markers = timeline
        .iter()
        .filter(|entry| entry.kind == CLEAR_KIND)
        .count();
    u32::try_from(markers).unwrap_or(u32::MAX)
}

/// One past the last clear entry, or the front of the timeline when there was none.
fn after_clear(timeline: &[IncidentTimelineEntry]) -> usize {
    let mut start = 0;
    for (index, entry) in timeline.iter().enumerate() {
        if entry.kind == CLEAR_KIND {
            start = index + 1;
        }
    }
    start
}

/// The follow-up questions and answers still in the conversation, oldest first. Entries
/// before the last clear are dropped, and entries that carry no question and no answer
/// are skipped.
pub fn since_clear(timeline: &[IncidentTimelineEntry]) -> Vec<Turn> {
    let mut turns: Vec<Turn> = Vec::new();
    for entry in &timeline[after_clear(timeline)..] {
        if entry.kind != FOLLOW_UP_KIND {
            continue;
        }
        let question = text(entry, "question");
        let answer = text(entry, "answer");
        if question.is_empty() || answer.is_empty() {
            tracing::debug!(
                %entry.incident,
                "a follow-up timeline entry had nothing readable in its detail",
            );
            continue;
        }
        turns.push(Turn {
            question,
            answer,
            asked_by: text(entry, "asked_by"),
            at: entry.at,
            asked_at: asked_at(entry),
        });
    }
    // The store hands entries back in write order, but two questions running at once
    // are written in the order their answers arrived. Put them back in the order the
    // operator asked them; the sort is stable, so equal times keep write order.
    turns.sort_by_key(|a| a.asked_at);
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    use domovoi_core::IncidentId;
    use serde_json::json;

    fn at(second: u8) -> Timestamp {
        format!("2026-10-31T12:00:{second:02}Z").parse().unwrap()
    }

    fn asked(
        question: &str,
        answer: &str,
        at: Timestamp,
        asked_at: Timestamp,
    ) -> IncidentTimelineEntry {
        IncidentTimelineEntry {
            incident: IncidentId::new(),
            at,
            kind: FOLLOW_UP_KIND.to_owned(),
            summary: question.to_owned(),
            detail: json!({
                "asked_by": "user:admin",
                "question": question,
                "answer": answer,
                "asked_at": asked_at.as_millisecond(),
            }),
        }
    }

    fn cleared(at: Timestamp) -> IncidentTimelineEntry {
        IncidentTimelineEntry {
            incident: IncidentId::new(),
            at,
            kind: CLEAR_KIND.to_owned(),
            summary: "cleared by user:admin".to_owned(),
            detail: serde_json::Value::Null,
        }
    }

    #[test]
    fn questions_before_a_clear_are_dropped() {
        let turns = since_clear(&[
            asked("Which disk is full?", "/srv is.", at(1), at(1)),
            cleared(at(2)),
            asked("What else?", "Nothing yet.", at(3), at(3)),
        ]);
        assert_eq!(
            turns,
            vec![Turn {
                question: "What else?".to_owned(),
                answer: "Nothing yet.".to_owned(),
                asked_by: "user:admin".to_owned(),
                at: at(3),
                asked_at: at(3),
            }],
            "only the question after the clear stays in the conversation",
        );
    }

    #[test]
    fn turns_come_back_oldest_first() {
        let turns = since_clear(&[
            asked("First?", "One.", at(1), at(1)),
            asked("Second?", "Two.", at(2), at(2)),
        ]);
        assert_eq!(
            turns
                .iter()
                .map(|t| t.question.clone())
                .collect::<Vec<String>>(),
            vec!["First?".to_owned(), "Second?".to_owned()],
            "the timeline is read in the order it was written",
        );
    }

    #[test]
    fn other_timeline_kinds_are_not_turns() {
        let noise = IncidentTimelineEntry {
            incident: IncidentId::new(),
            at: at(1),
            kind: "triage.report".to_owned(),
            summary: "The disk is full.".to_owned(),
            detail: json!({"headline": "The disk is full."}),
        };
        assert!(
            since_clear(&[noise]).is_empty(),
            "a report is not a question",
        );
    }

    #[test]
    fn questions_answered_out_of_order_come_back_in_the_order_they_were_asked() {
        let turns = since_clear(&[
            asked("Second?", "Two.", at(2), at(1)),
            asked("First?", "One.", at(3), at(0)),
        ]);
        assert_eq!(
            turns
                .iter()
                .map(|t| t.question.clone())
                .collect::<Vec<String>>(),
            vec!["First?".to_owned(), "Second?".to_owned()],
            "the entry written first belongs to the question asked second",
        );
    }

    #[test]
    fn an_entry_without_an_asking_time_falls_back_to_its_own_time() {
        let legacy = IncidentTimelineEntry {
            incident: IncidentId::new(),
            at: at(4),
            kind: FOLLOW_UP_KIND.to_owned(),
            summary: "Old question?".to_owned(),
            detail: json!({
                "asked_by": "user:admin",
                "question": "Old question?",
                "answer": "Old answer.",
            }),
        };
        assert_eq!(
            since_clear(&[legacy]),
            vec![Turn {
                question: "Old question?".to_owned(),
                answer: "Old answer.".to_owned(),
                asked_by: "user:admin".to_owned(),
                at: at(4),
                asked_at: at(4),
            }],
            "the fallback is the entry's own time",
        );
    }

    #[test]
    fn clear_markers_are_counted_once_each() {
        assert_eq!(
            count_clear(&[]),
            0,
            "no marker means the oldest conversation"
        );
        assert_eq!(
            count_clear(&[
                asked("Q?", "A.", at(1), at(0)),
                cleared(at(2)),
                asked("R?", "B.", at(3), at(3))
            ]),
            1,
            "one clear marker sits in the timeline",
        );
        assert_eq!(
            count_clear(&[cleared(at(1)), cleared(at(2))]),
            2,
            "every clear marker counts, not just the last",
        );
    }
}
