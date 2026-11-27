//! Incident commands every transport shares: acknowledge, mute, unmute, ask a follow-up
//! question and run triage again.

use domovoi_core::ports::IncidentTimelineEntry;
use domovoi_core::{Incident, IncidentId, TriageOutcome};
use domovoi_engine::CommandError;
use domovoi_engine::ManagerHandle;
use domovoi_engine::operations::{InvestigateError, OperationError, OperationHandle};
use domovoi_engine::triage::{TriageError, TriageHandle};
use jiff::Timestamp;

use super::Caller;

/// Why a follow-up question was not answered.
#[derive(Debug, thiserror::Error)]
pub enum FollowUpError {
    /// Triage is not enabled, so there is no model to ask.
    #[error("triage is not enabled")]
    TriageOff,
    /// The triage service refused or failed.
    #[error(transparent)]
    Triage(#[from] TriageError),
}

/// How a re-triage went.
#[derive(Debug, Clone, PartialEq)]
pub enum Retriaged {
    /// It finished with this outcome.
    Outcome(Box<TriageOutcome>),
    /// It is still running; its result appears on the incident.
    StillRunning,
}

/// Why a re-triage did not start or did not finish.
#[derive(Debug)]
pub enum RetriageError {
    /// Triage is not enabled.
    TriageOff,
    /// Investigations are not enabled (`[orchestration] enabled = false`).
    OperationsOff,
    /// An investigation of this incident is already running.
    AlreadyRunning,
    /// The incident says a check is blind: it already gives the reason and the fix.
    MonitoringBlind,
    /// The operation service refused or failed.
    Operation(OperationError),
}

/// The incident commands.
#[derive(Clone)]
pub struct IncidentService {
    pub(super) manager: ManagerHandle,
    pub(super) triage: Option<TriageHandle>,
    pub(super) operations: Option<OperationHandle>,
}

impl IncidentService {
    /// Acknowledges `incident` for the caller.
    pub async fn ack(&self, caller: &Caller, incident: IncidentId) -> Result<(), CommandError> {
        self.manager.ack(incident, caller.user()).await
    }

    /// Mutes `incident` until `until` for the caller.
    pub async fn mute(
        &self,
        caller: &Caller,
        incident: IncidentId,
        until: Timestamp,
    ) -> Result<(), CommandError> {
        self.manager.mute(incident, until, caller.user()).await
    }

    /// Lifts the mute of `incident`.
    pub async fn unmute(&self, caller: &Caller, incident: IncidentId) -> Result<(), CommandError> {
        tracing::debug!(user = %caller.user(), %incident, "unmute");
        self.manager.unmute(incident).await
    }

    /// Whether triage is enabled.
    #[must_use]
    pub fn triage_enabled(&self) -> bool {
        self.triage.is_some()
    }

    /// Asks the triage model a follow-up question about `incident`.
    pub async fn follow_up(
        &self,
        caller: &Caller,
        incident: IncidentId,
        question: &str,
    ) -> Result<String, FollowUpError> {
        let triage = self.triage.as_ref().ok_or(FollowUpError::TriageOff)?;
        Ok(triage.follow_up(incident, question, caller.user()).await?)
    }

    /// Investigates `incident` again as an operation; `fresh` skips a cached report.
    pub async fn retriage(
        &self,
        caller: &Caller,
        incident: &Incident,
        fresh: bool,
    ) -> Result<Retriaged, RetriageError> {
        if incident.is_monitoring_blind() {
            return Err(RetriageError::MonitoringBlind);
        }
        if self.triage.is_none() {
            return Err(RetriageError::TriageOff);
        }
        let operations = self
            .operations
            .as_ref()
            .ok_or(RetriageError::OperationsOff)?;
        tracing::debug!(user = %caller.user(), incident = %incident.id(), fresh, "retriage");
        match operations
            .investigate_on_request_and_wait(incident, fresh)
            .await
        {
            Ok(outcome) => Ok(Retriaged::Outcome(Box::new(outcome))),
            Err(InvestigateError::AlreadyRunning(_)) => Err(RetriageError::AlreadyRunning),
            Err(InvestigateError::TimedOut(_)) => Ok(Retriaged::StillRunning),
            Err(InvestigateError::Operation(err)) => Err(RetriageError::Operation(err)),
        }
    }
}

/// The newest report the timeline holds, fresh or cached, even when a later triage was
/// skipped or failed. The detail view shows it beside the later state, so an earlier answer
/// does not vanish while a retry is pending.
#[must_use]
pub fn latest_report(timeline: &[IncidentTimelineEntry]) -> Option<domovoi_core::TriageReport> {
    timeline
        .iter()
        .rev()
        .filter(|e| e.kind.starts_with("triage."))
        .find_map(|e| match serde_json::from_value(e.detail.clone()).ok()? {
            TriageOutcome::Report(r) | TriageOutcome::Cached { report: r, .. } => Some(r),
            _ => None,
        })
}

/// The latest triage outcome in the timeline.
#[must_use]
pub fn latest_triage(timeline: &[IncidentTimelineEntry]) -> Option<TriageOutcome> {
    timeline
        .iter()
        .rev()
        .filter(|e| {
            e.kind.starts_with("triage.") && e.kind != "triage.followup" && e.kind != "triage.tools"
        })
        .find_map(|e| serde_json::from_value(e.detail.clone()).ok())
}
