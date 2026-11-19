//! Subscription topics of the journal stream, and which journal rows belong to which topic.

use std::collections::BTreeSet;

use uuid::Uuid;

use super::{AggregateKind, JournalEntry};

/// A subscription topic of the journal stream.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    /// Every incident row, and every action row (an incident's card shows its actions).
    Incidents,
    /// Rows that change what awaits a human decision: `action.transition` and
    /// `operation.execution`.
    Approvals,
    /// Every row of every operation.
    Operations,
    /// Every row of one operation (canonical UUID text).
    Operation(String),
    /// Reserved: host rows. Accepted, delivers nothing until a mapping is added.
    Hosts,
    /// Reserved: one conversation (canonical UUID text). Accepted, delivers nothing yet.
    Conversation(String),
}

/// Why a topic name was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TopicError {
    /// The name is not part of the grammar.
    #[error("unknown topic `{0}`")]
    Unknown(String),
    /// The part after the colon is not a lowercase hyphenated UUID.
    #[error("`{0}` is not a valid id")]
    BadId(String),
}

fn canonical_uuid(text: &str) -> Result<String, TopicError> {
    let parsed = Uuid::parse_str(text).map_err(|_| TopicError::BadId(text.to_owned()))?;
    let canonical = parsed.hyphenated().to_string();
    if canonical == text {
        Ok(canonical)
    } else {
        Err(TopicError::BadId(text.to_owned()))
    }
}

impl Topic {
    /// The name used on the wire, for example `operation:<uuid>`.
    #[must_use]
    pub fn as_wire(&self) -> String {
        match self {
            Self::Incidents => "incidents".to_owned(),
            Self::Approvals => "approvals".to_owned(),
            Self::Operations => "operations".to_owned(),
            Self::Operation(id) => format!("operation:{id}"),
            Self::Hosts => "hosts".to_owned(),
            Self::Conversation(id) => format!("conversations:{id}"),
        }
    }

    /// Reads a wire name back.
    ///
    /// # Errors
    /// [`TopicError::Unknown`] for a name outside the grammar and [`TopicError::BadId`] for a
    /// malformed id.
    pub fn parse(text: &str) -> Result<Self, TopicError> {
        match text {
            "incidents" => Ok(Self::Incidents),
            "approvals" => Ok(Self::Approvals),
            "operations" => Ok(Self::Operations),
            "hosts" => Ok(Self::Hosts),
            _ => {
                if let Some(id) = text.strip_prefix("operation:") {
                    Ok(Self::Operation(canonical_uuid(id)?))
                } else if let Some(id) = text.strip_prefix("conversations:") {
                    Ok(Self::Conversation(canonical_uuid(id)?))
                } else {
                    Err(TopicError::Unknown(text.to_owned()))
                }
            }
        }
    }
}

/// The set of topics a connection asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicSet(BTreeSet<Topic>);

impl TopicSet {
    /// The most topics one connection may ask for.
    pub const MAX: usize = 16;

    /// Parses a comma-separated list; `None` means the default set. Empty entries and more than
    /// [`TopicSet::MAX`] distinct topics are refused.
    ///
    /// # Errors
    /// The first [`TopicError`] met; too many topics is reported as `Unknown` naming the count.
    pub fn parse_list(csv: Option<&str>) -> Result<Self, TopicError> {
        let Some(csv) = csv else {
            return Ok(Self::default_set());
        };
        let mut set = BTreeSet::new();
        for part in csv.split(',') {
            set.insert(Topic::parse(part)?);
        }
        if set.len() > Self::MAX {
            return Err(TopicError::Unknown(format!("{} topics", set.len())));
        }
        Ok(Self(set))
    }

    /// `incidents`, `approvals` and `operations`.
    #[must_use]
    pub fn default_set() -> Self {
        Self(BTreeSet::from([
            Topic::Incidents,
            Topic::Approvals,
            Topic::Operations,
        ]))
    }

    /// The canonical sorted wire names.
    #[must_use]
    pub fn wire(&self) -> Vec<String> {
        let mut names: Vec<String> = self.0.iter().map(Topic::as_wire).collect();
        names.sort();
        names
    }

    /// Whether any topic of `entry` is in the set.
    #[must_use]
    pub fn matches(&self, entry: &JournalEntry) -> bool {
        topics_of(entry).iter().any(|topic| self.0.contains(topic))
    }
}

/// The topics one journal row belongs to; empty means "never streamed".
#[must_use]
pub fn topics_of(entry: &JournalEntry) -> Vec<Topic> {
    match entry.aggregate.kind {
        AggregateKind::Incident => vec![Topic::Incidents],
        AggregateKind::Action => vec![Topic::Approvals, Topic::Incidents],
        AggregateKind::Operation => {
            let mut topics = vec![Topic::Operations];
            if canonical_uuid(&entry.aggregate.id).is_ok() {
                topics.push(Topic::Operation(entry.aggregate.id.clone()));
            }
            if entry.kind == "operation.execution" {
                topics.push(Topic::Approvals);
            }
            topics
        }
        // Aggregates with no topic yet; add an arm here when a consumer needs it.
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use serde_json::json;

    use super::super::AggregateRef;
    use super::*;

    const OP: &str = "0190a0a0-0000-7000-8000-0000000000a1";

    fn entry(kind: AggregateKind, id: &str, name: &str) -> JournalEntry {
        JournalEntry {
            seq: 1,
            aggregate: AggregateRef {
                kind,
                id: id.to_owned(),
            },
            kind: name.to_owned(),
            payload_v: 1,
            payload: json!({}),
            actor: "system:test".to_owned(),
            occurred_at: Timestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn topic_grammar_accepts_the_documented_names_and_rejects_the_rest() {
        for name in [
            "incidents".to_owned(),
            "approvals".to_owned(),
            "operations".to_owned(),
            format!("operation:{OP}"),
            "hosts".to_owned(),
            format!("conversations:{OP}"),
        ] {
            assert_eq!(Topic::parse(&name).map(|t| t.as_wire()), Ok(name));
        }
        for bad in [
            "Incidents".to_owned(),
            "operation:".to_owned(),
            "operation:xyz".to_owned(),
            "incident".to_owned(),
            String::new(),
            format!("operations:{OP}"),
        ] {
            assert!(Topic::parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn default_topic_set_is_incidents_approvals_operations() {
        assert_eq!(TopicSet::parse_list(None), Ok(TopicSet::default_set()));
        assert_eq!(
            TopicSet::default_set().wire(),
            ["approvals", "incidents", "operations"]
        );
    }

    #[test]
    fn topic_set_rejects_more_than_sixteen_and_empty_entries() {
        let many: Vec<String> = (0..17)
            .map(|n| format!("operation:0190a0a0-0000-7000-8000-{n:012}"))
            .collect();
        assert!(TopicSet::parse_list(Some(&many.join(","))).is_err());
        assert!(TopicSet::parse_list(Some("incidents,,approvals")).is_err());
        assert!(TopicSet::parse_list(Some("incidents,")).is_err());
        let dup = TopicSet::parse_list(Some("incidents,incidents")).unwrap();
        assert_eq!(dup.wire(), ["incidents"]);
    }

    #[test]
    fn incident_rows_belong_to_incidents() {
        let e = entry(AggregateKind::Incident, OP, "incident.timeline");
        assert_eq!(topics_of(&e), [Topic::Incidents]);
    }

    #[test]
    fn action_rows_belong_to_approvals_and_incidents() {
        let e = entry(AggregateKind::Action, OP, "action.transition");
        assert_eq!(topics_of(&e), [Topic::Approvals, Topic::Incidents]);
    }

    #[test]
    fn operation_rows_belong_to_operations_and_their_own_topic() {
        let e = entry(AggregateKind::Operation, OP, "operation.step_finished");
        assert_eq!(
            topics_of(&e),
            [Topic::Operations, Topic::Operation(OP.to_owned())]
        );
    }

    #[test]
    fn operation_execution_rows_also_belong_to_approvals() {
        let e = entry(AggregateKind::Operation, OP, "operation.execution");
        assert!(topics_of(&e).contains(&Topic::Approvals));
    }

    #[test]
    fn knowledge_and_note_rows_belong_to_no_topic() {
        assert!(topics_of(&entry(AggregateKind::Note, OP, "note.changed")).is_empty());
    }

    #[test]
    fn reserved_topics_match_nothing() {
        let set = TopicSet::parse_list(Some(&format!("hosts,conversations:{OP}"))).unwrap();
        for e in [
            entry(AggregateKind::Incident, OP, "incident.timeline"),
            entry(AggregateKind::Action, OP, "action.transition"),
            entry(AggregateKind::Operation, OP, "operation.execution"),
            entry(AggregateKind::Note, OP, "x"),
        ] {
            assert!(!set.matches(&e));
        }
    }
}
