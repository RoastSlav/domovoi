//! Signals, a simulated store for the pure decision function, and event helpers.

use domovoi_core::{
    CheckKey, Evidence, HostId, Incident, IncidentId, IncidentStatus, Severity, Signal,
    SignalClass, SourceKind, TargetKey,
};
use domovoi_engine::manager::{Command, CommandError, Decision, State, Write, decide};
use domovoi_engine::{EnginePolicy, EventKind};
use domovoi_testkit::build::at;
use jiff::Timestamp;

pub fn kuma() -> SourceKind {
    SourceKind::Webhook {
        kind: "uptime-kuma".to_owned(),
    }
}

/// A signal builder with sensible defaults: an agent warning, normal class.
#[derive(Clone)]
pub struct Sig {
    pub host: HostId,
    pub check: &'static str,
    pub target: Option<&'static str>,
    pub source: SourceKind,
    pub severity: Severity,
    pub class: SignalClass,
    pub evidence: Evidence,
}

impl Sig {
    pub fn new(host: HostId, check: &'static str, target: Option<&'static str>) -> Self {
        Self {
            host,
            check,
            target,
            source: SourceKind::Agent,
            severity: Severity::Warning,
            class: SignalClass::Normal,
            evidence: Evidence::new(),
        }
    }

    pub fn source(mut self, source: SourceKind) -> Self {
        self.source = source;
        self
    }

    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    pub fn immediate(mut self) -> Self {
        self.class = SignalClass::Immediate;
        self
    }

    pub fn evidence(mut self, key: &str, value: domovoi_core::EvidenceValue) -> Self {
        self.evidence.insert(key, value);
        self
    }

    fn target_key(&self) -> Option<TargetKey> {
        self.target.map(|t| TargetKey::parse(t).unwrap())
    }

    pub fn firing(&self, secs: i64) -> Signal {
        Signal::firing(
            self.source.clone(),
            self.host,
            CheckKey::parse(self.check).unwrap(),
            self.target_key(),
            self.severity,
            format!("{} firing", self.check),
            at(secs),
        )
        .with_class(self.class)
        .with_evidence(self.evidence.clone())
    }

    pub fn resolved(&self, secs: i64) -> Signal {
        Signal::resolved(
            self.source.clone(),
            self.host,
            CheckKey::parse(self.check).unwrap(),
            self.target_key(),
            format!("{} ok", self.check),
            at(secs),
        )
    }
}

/// The pure decision function plus a stand-in for the store, so tests can run several
/// commands in a row.
pub struct Sim {
    pub state: State,
    pub policy: EnginePolicy,
}

impl Sim {
    pub fn new(policy: EnginePolicy) -> Self {
        Self {
            state: State::default(),
            policy,
        }
    }

    pub fn try_run(&mut self, command: Command, secs: i64) -> Result<Decision, CommandError> {
        self.try_run_at(command, at(secs))
    }

    pub fn try_run_at(
        &mut self,
        command: Command,
        now: Timestamp,
    ) -> Result<Decision, CommandError> {
        let decision = decide(&self.state, &command, &self.policy, now)?;
        for write in &decision.writes {
            match write {
                Write::InsertIncident(i) | Write::UpdateIncident(i) => {
                    self.state.active.retain(|a| a.id() != i.id());
                    if i.status() == IncidentStatus::Resolved {
                        self.state.resolved.insert(i.fingerprint(), i.clone());
                    } else {
                        self.state.active.push(i.clone());
                    }
                }
                Write::DiscardPending(id) => self.state.active.retain(|a| a.id() != *id),
                Write::InsertSignal(..) | Write::Timeline(_) => {}
            }
        }
        Ok(decision)
    }

    pub fn run(&mut self, command: Command, secs: i64) -> Decision {
        self.try_run(command, secs).unwrap()
    }

    pub fn ingest(&mut self, signal: Signal, secs: i64) -> Decision {
        self.run(Command::Ingest(vec![signal]), secs)
    }

    pub fn only_active(&self) -> &Incident {
        assert_eq!(self.state.active.len(), 1, "{:#?}", self.state.active);
        &self.state.active[0]
    }
}

/// The kinds of a decision's events, for compact assertions.
pub fn kinds(decision: &Decision) -> Vec<EventKind> {
    decision.events.iter().map(|e| e.kind.clone()).collect()
}

/// The ids of the incidents a decision's events are about.
pub fn incident_ids(decision: &Decision) -> Vec<IncidentId> {
    decision.events.iter().map(|e| e.incident.id()).collect()
}
