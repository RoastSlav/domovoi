//! Records the listeners an agent finds on its first run as one reviewable group of expected
//! listeners, instead of one incident each.
//!
//! The `security.new_port` check marks its first run's observations with `first_run`. This
//! sink sits in front of the incident manager: for each such signal it adds a known
//! condition (an ack rule) for that listener on that host, unless one is there already, and
//! it does not pass the signal on. Everything else goes through untouched. The entries carry
//! [`FIRST_RUN_NOTE`] until a person edits their note, which is how the Known conditions
//! page counts the ones that are not reviewed yet. This is the one place where entries are
//! made without a person dismissing an incident.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{AckRepo, AuditEvent, AuditLog, Clock, IngestError, SignalSink};
use domovoi_core::{AckRule, AckRuleId, EvidenceValue, Signal, TargetPattern};
use serde_json::json;

/// The note of an entry made from a first run; editing the note marks the entry reviewed.
pub const FIRST_RUN_NOTE: &str = "Found on first run (not reviewed)";

/// The check whose first run is recorded.
const CHECK: &str = "security.new_port";

/// A [`SignalSink`] that turns first-run listeners into known conditions.
pub struct FirstRunSink {
    inner: Arc<dyn SignalSink>,
    acks: Arc<dyn AckRepo>,
    audit: Arc<dyn AuditLog>,
    clock: Arc<dyn Clock>,
}

impl FirstRunSink {
    /// A sink in front of `inner`.
    #[must_use]
    pub fn new(
        inner: Arc<dyn SignalSink>,
        acks: Arc<dyn AckRepo>,
        audit: Arc<dyn AuditLog>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            inner,
            acks,
            audit,
            clock,
        }
    }
}

fn is_first_run(signal: &Signal) -> bool {
    signal.check().as_str() == CHECK
        && matches!(
            signal.evidence().get("first_run"),
            Some(EvidenceValue::Bool(true))
        )
}

#[async_trait]
impl SignalSink for FirstRunSink {
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError> {
        let (first, rest): (Vec<_>, Vec<_>) = signals.into_iter().partition(is_first_run);
        if !first.is_empty() {
            let now = self.clock.now();
            let existing = self
                .acks
                .list_rules()
                .await
                .map_err(|e| IngestError::Unavailable(e.to_string()))?;
            let mut added = Vec::new();
            for signal in &first {
                let Some(target) = signal.target() else {
                    continue;
                };
                let known = existing.iter().any(|rule| {
                    rule.host == Some(signal.host())
                        && rule.check.as_str() == CHECK
                        && rule
                            .target_pattern
                            .as_ref()
                            .is_some_and(|p| p.to_string() == target.as_str())
                });
                if known {
                    continue;
                }
                let Ok(pattern) = TargetPattern::parse(target.as_str()) else {
                    continue;
                };
                let rule = AckRule {
                    id: AckRuleId::new(),
                    host: Some(signal.host()),
                    check: signal.check().clone(),
                    target_pattern: Some(pattern),
                    predicates: Vec::new(),
                    note: FIRST_RUN_NOTE.to_owned(),
                    expires_at: None,
                    created_at: Some(now),
                    created_by: Some("agent first run".to_owned()),
                };
                self.acks
                    .put_rule(&rule)
                    .await
                    .map_err(|e| IngestError::Unavailable(e.to_string()))?;
                added.push(target.as_str().to_owned());
            }
            if !added.is_empty() {
                let host = first[0].host();
                self.audit
                    .append(&AuditEvent {
                        at: now,
                        actor: "agent:first-run".to_owned(),
                        kind: "ack_rule.first_run".to_owned(),
                        subject: format!("host:{host}"),
                        detail: json!({ "check": CHECK, "count": added.len(), "targets": added }),
                    })
                    .await
                    .map_err(|e| IngestError::Unavailable(e.to_string()))?;
            }
        }
        if rest.is_empty() {
            return Ok(());
        }
        self.inner.submit(rest).await
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::ports::AckRepo;
    use domovoi_core::{CheckKey, HostId, Severity, SourceKind, TargetKey};
    use domovoi_testkit::build::T0;
    use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};

    use super::*;

    fn listener(host: HostId, target: &str, first_run: bool) -> Signal {
        let mut evidence = domovoi_core::Evidence::new();
        if first_run {
            evidence.insert("first_run", EvidenceValue::Bool(true));
        }
        Signal::firing(
            SourceKind::Agent,
            host,
            CheckKey::parse(CHECK).unwrap(),
            Some(TargetKey::parse(target).unwrap()),
            Severity::Warning,
            "new listener",
            T0,
        )
        .with_evidence(evidence)
    }

    fn rig() -> (FirstRunSink, Arc<InMemoryStore>, Arc<RecordingSignalSink>) {
        let store = Arc::new(InMemoryStore::new());
        let inner = Arc::new(RecordingSignalSink::new());
        let sink = FirstRunSink::new(
            inner.clone(),
            store.clone(),
            store.clone(),
            Arc::new(FakeClock::new(T0)),
        );
        (sink, store, inner)
    }

    #[tokio::test]
    async fn first_run_listeners_become_one_reviewable_group_and_no_incidents() {
        let (sink, store, inner) = rig();
        let host = HostId::new();
        let batch = vec![
            listener(host, "0.0.0.0:22/tcp", true),
            listener(host, "0.0.0.0:80/tcp", true),
        ];
        sink.submit(batch.clone()).await.unwrap();
        let rules = store.list_rules().await.unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().all(|r| r.note == FIRST_RUN_NOTE));
        assert!(
            inner.batches().is_empty(),
            "no signal reaches the incidents"
        );
        // The same group again adds nothing.
        sink.submit(batch).await.unwrap();
        assert_eq!(store.list_rules().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn later_listeners_pass_through_as_signals() {
        let (sink, store, inner) = rig();
        let host = HostId::new();
        sink.submit(vec![listener(host, "0.0.0.0:4324/tcp", false)])
            .await
            .unwrap();
        assert!(store.list_rules().await.unwrap().is_empty());
        assert_eq!(inner.batches().len(), 1);
    }
}
