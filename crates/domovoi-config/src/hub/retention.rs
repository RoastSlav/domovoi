//! `[retention]`: how many days each kind of raw data is kept.

use schemars::JsonSchema;
use serde::Deserialize;

/// `[retention]`: how many days each kind of raw data is kept. Operations, resolution
/// summaries and the audit log are kept and have no setting here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetentionSection {
    /// Days a signal is kept. Default 30.
    #[serde(default = "d30")]
    pub signals_days: u32,
    /// Days an incident timeline entry other than a follow-up conversation is kept.
    /// Default 90.
    #[serde(default = "d90")]
    pub incident_timeline_days: u32,
    /// Days follow-up questions on an incident and Operator conversation messages are kept. Default 180.
    #[serde(default = "d180")]
    pub conversations_days: u32,
    /// Days the notification state of a resolved incident is kept. Default 30.
    #[serde(default = "d30")]
    pub notifications_days: u32,
    /// Days a cached triage report is kept. Default 90.
    #[serde(default = "d90")]
    pub triage_cache_days: u32,
    /// Days a resolved advisory is kept. Default 30.
    #[serde(default = "d30")]
    pub resolved_advisories_days: u32,
    /// Days the operation journal is kept. Not applied yet: applies once operations exist.
    /// Default 180.
    #[serde(default = "d180")]
    pub operation_journal_days: u32,
    /// Days raw execution output is kept. Not applied yet: applies once execution outputs
    /// are stored apart from the audit log. Default 30.
    #[serde(default = "d30")]
    pub execution_outputs_days: u32,
    /// Days the content of a retracted fact or relation is kept; its tombstone stays.
    /// Default 30.
    #[serde(default = "d30")]
    pub retracted_facts_days: u32,
}

impl Default for RetentionSection {
    fn default() -> Self {
        Self {
            signals_days: d30(),
            incident_timeline_days: d90(),
            conversations_days: d180(),
            notifications_days: d30(),
            triage_cache_days: d90(),
            resolved_advisories_days: d30(),
            operation_journal_days: d180(),
            execution_outputs_days: d30(),
            retracted_facts_days: d30(),
        }
    }
}

impl RetentionSection {
    /// Every key with its value, in [`RETENTION_KEYS`] order.
    pub fn days(&self) -> [(&'static str, u32); 9] {
        [
            ("signals_days", self.signals_days),
            ("incident_timeline_days", self.incident_timeline_days),
            ("conversations_days", self.conversations_days),
            ("notifications_days", self.notifications_days),
            ("triage_cache_days", self.triage_cache_days),
            ("resolved_advisories_days", self.resolved_advisories_days),
            ("operation_journal_days", self.operation_journal_days),
            ("execution_outputs_days", self.execution_outputs_days),
            ("retracted_facts_days", self.retracted_facts_days),
        ]
    }
}

/// Whether a retention key does anything today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// The purge applies the key now.
    Now,
    /// The key is accepted and documented, and applies once the named feature exists.
    Once(&'static str),
}

/// One `[retention]` key: its name, its default and whether it applies today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionKey {
    /// The key as written in `[retention]`, such as `signals_days`.
    pub key: &'static str,
    /// The number of days kept when nothing sets the key.
    pub default_days: u32,
    /// Whether the purge applies the key today.
    pub applies: Applies,
}

/// The registry of every `[retention]` key, in the order of the configuration reference.
/// Docs and tests read it.
pub const RETENTION_KEYS: &[RetentionKey] = &[
    RetentionKey {
        key: "signals_days",
        default_days: 30,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "incident_timeline_days",
        default_days: 90,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "conversations_days",
        default_days: 180,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "notifications_days",
        default_days: 30,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "triage_cache_days",
        default_days: 90,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "resolved_advisories_days",
        default_days: 30,
        applies: Applies::Now,
    },
    RetentionKey {
        key: "operation_journal_days",
        default_days: 180,
        applies: Applies::Once("operations exist"),
    },
    RetentionKey {
        key: "execution_outputs_days",
        default_days: 30,
        applies: Applies::Once("execution outputs are stored apart from the audit log"),
    },
    RetentionKey {
        key: "retracted_facts_days",
        default_days: 30,
        applies: Applies::Now,
    },
];

fn d30() -> u32 {
    30
}
fn d90() -> u32 {
    90
}
fn d180() -> u32 {
    180
}

#[cfg(test)]
mod tests {
    use super::{Applies, RETENTION_KEYS, RetentionSection};

    #[test]
    fn retention_defaults_match_the_owner_decision() {
        let expected = [
            ("signals_days", 30),
            ("incident_timeline_days", 90),
            ("conversations_days", 180),
            ("notifications_days", 30),
            ("triage_cache_days", 90),
            ("resolved_advisories_days", 30),
            ("operation_journal_days", 180),
            ("execution_outputs_days", 30),
            ("retracted_facts_days", 30),
        ];
        assert_eq!(RetentionSection::default().days(), expected);
        assert_eq!(
            RETENTION_KEYS
                .iter()
                .map(|entry| (entry.key, entry.default_days))
                .collect::<Vec<_>>(),
            expected.to_vec()
        );
    }

    #[test]
    fn retention_registry_marks_the_unbuilt_keys() {
        let unbuilt: Vec<_> = RETENTION_KEYS
            .iter()
            .filter(|entry| matches!(entry.applies, Applies::Once(_)))
            .map(|entry| entry.key)
            .collect();
        assert_eq!(
            unbuilt,
            vec!["operation_journal_days", "execution_outputs_days"]
        );
        for entry in RETENTION_KEYS {
            match entry.applies {
                Applies::Once(reason) => assert!(!reason.is_empty(), "{}", entry.key),
                Applies::Now => assert!(!unbuilt.contains(&entry.key)),
            }
        }
    }
}
