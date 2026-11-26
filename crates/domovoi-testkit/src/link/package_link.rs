//! The package half of `FakeAgentLink`: scripted transfer verdicts and capability results, one
//! ordered record of everything the link was asked, and the gates and hooks a test uses to
//! stop a call at the point it needs to look at.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use domovoi_core::ports::{AgentLink, LinkError, Presence};
use domovoi_core::{
    AgentSessionId, CapabilityHandoff, ExecutionId, HostId, JournalEpoch, PackageDispatch,
    PackageOutputs, RequestId, TransferAnswer, TransferFrame,
};
use jiff::Timestamp;
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

use super::FakeAgentLink;
use crate::lock;

/// What a test scripts for one capability call.
pub type CapabilityHandler =
    Arc<dyn Fn(HostId, &PackageDispatch) -> Result<PackageOutputs, LinkError> + Send + Sync>;

/// A hook run when `start_capability` is entered, before the link checks the session.
pub type CapabilityHook = Arc<dyn Fn(HostId, &PackageDispatch) + Send + Sync>;

/// One package call made through a `FakeAgentLink`, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageEvent {
    /// A transfer frame was accepted into the session's queue.
    Transfer {
        /// Target host.
        host: HostId,
        /// The frame.
        frame: TransferFrame,
        /// The session the caller expected.
        session: AgentSessionId,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// A capability request was accepted into the session's queue.
    Capability {
        /// Target host.
        host: HostId,
        /// The request.
        dispatch: PackageDispatch,
        /// The session the caller expected.
        session: AgentSessionId,
        /// The journal epoch the caller expected.
        epoch: JournalEpoch,
        /// The deadline passed.
        deadline: Timestamp,
    },
    /// The caller started waiting for a capability's answer.
    CapabilityResult {
        /// Target host.
        host: HostId,
        /// The execution whose answer was awaited.
        execution: ExecutionId,
    },
}

/// The scripted state behind the package operations of one fake link.
#[derive(Default)]
pub(super) struct PackageState {
    events: std::sync::Mutex<Vec<PackageEvent>>,
    verdicts: std::sync::Mutex<BTreeMap<Uuid, VecDeque<Result<TransferAnswer, LinkError>>>>,
    results: std::sync::Mutex<BTreeMap<ExecutionId, Result<PackageOutputs, LinkError>>>,
    handler: std::sync::Mutex<Option<CapabilityHandler>>,
    hook: std::sync::Mutex<Option<CapabilityHook>>,
    end_gate: std::sync::Mutex<Option<(Arc<Semaphore>, Arc<Notify>)>>,
    sent: std::sync::Mutex<BTreeMap<RequestId, (PackageDispatch, JournalEpoch)>>,
}

impl FakeAgentLink {
    /// Answers the `End` frame of `transfer` with `answer`, once. Without a script an `End`
    /// is answered [`TransferAnswer::Stored`]; several scripts for one transfer answer in order.
    pub fn answer_transfer(&self, transfer: Uuid, answer: Result<TransferAnswer, LinkError>) {
        lock(&self.package.verdicts)
            .entry(transfer)
            .or_default()
            .push_back(answer);
    }

    /// Answers every `capability_result` that has no per-execution script with `handler`.
    pub fn on_capability(
        &self,
        handler: impl Fn(HostId, &PackageDispatch) -> Result<PackageOutputs, LinkError>
        + Send
        + Sync
        + 'static,
    ) {
        *lock(&self.package.handler) = Some(Arc::new(handler));
    }

    /// Runs `hook` each time `start_capability` is entered, before the session is checked, so
    /// a test can look at its world at the moment a request is about to be sent, or change
    /// the session under the call.
    pub fn on_start_capability(
        &self,
        hook: impl Fn(HostId, &PackageDispatch) + Send + Sync + 'static,
    ) {
        *lock(&self.package.hook) = Some(Arc::new(hook));
    }

    /// Makes every `End` frame wait for a permit from `gate` before it is answered. The frame
    /// is recorded first, and the returned notifier fires each time an `End` starts waiting.
    pub fn hold_transfer_ends(&self, gate: Arc<Semaphore>) -> Arc<Notify> {
        let entered = Arc::new(Notify::new());
        *lock(&self.package.end_gate) = Some((gate, entered.clone()));
        entered
    }

    /// Every package call so far, oldest first: transfer frames, capability requests and
    /// the moments a result was awaited, in one sequence.
    #[must_use]
    pub fn package_events(&self) -> Vec<PackageEvent> {
        lock(&self.package.events).clone()
    }

    /// Only the transfer frames of [`FakeAgentLink::package_events`], oldest first.
    #[must_use]
    pub fn transfer_frames(&self) -> Vec<TransferFrame> {
        self.package_events()
            .into_iter()
            .filter_map(|event| match event {
                PackageEvent::Transfer { frame, .. } => Some(frame),
                _ => None,
            })
            .collect()
    }

    pub(super) async fn fake_transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        self.connected(host)?;
        if self.session_id(host) != expected_session {
            return Err(LinkError::SessionChanged);
        }
        let transfer = frame.transfer();
        let is_end = matches!(frame, TransferFrame::End { .. });
        lock(&self.package.events).push(PackageEvent::Transfer {
            host,
            frame,
            session: expected_session,
            deadline,
        });
        if !is_end {
            return Ok(None);
        }
        let gate = lock(&self.package.end_gate).clone();
        if let Some((gate, entered)) = gate {
            entered.notify_one();
            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|_| LinkError::Indeterminate("the transfer gate closed".to_owned()))?;
            // The session may have been replaced while the End waited.
            if self.session_id(host) != expected_session
                || !matches!(self.presence(host), Presence::Connected { .. })
            {
                return Err(LinkError::Indeterminate(
                    "the session ended before the agent answered".to_owned(),
                ));
            }
        }
        let scripted = lock(&self.package.verdicts)
            .get_mut(&transfer)
            .and_then(VecDeque::pop_front);
        scripted.unwrap_or(Ok(TransferAnswer::Stored)).map(Some)
    }

    pub(super) async fn fake_start_capability(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        let hook = lock(&self.package.hook).clone();
        if let Some(hook) = hook {
            hook(host, &req);
        }
        self.connected(host)?;
        if self.session_id(host) != expected_session
            || self.journal_epoch(host) != Some(expected_epoch)
        {
            return Err(LinkError::SessionChanged);
        }
        lock(&self.package.events).push(PackageEvent::Capability {
            host,
            dispatch: req.clone(),
            session: expected_session,
            epoch: expected_epoch,
            deadline,
        });
        let id = RequestId::new();
        lock(&self.package.sent).insert(id, (req, expected_epoch));
        Ok(CapabilityHandoff::new(id))
    }

    pub(super) async fn fake_capability_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        _deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        let Some((req, _epoch)) = lock(&self.package.sent).remove(&handoff.request()) else {
            return Err(LinkError::Indeterminate(
                "no package request was handed over".to_owned(),
            ));
        };
        lock(&self.package.events).push(PackageEvent::CapabilityResult {
            host,
            execution: req.execution,
        });
        self.connected(host)?;
        if let Some(scripted) = lock(&self.package.results).remove(&req.execution) {
            return scripted;
        }
        let handler = lock(&self.package.handler).clone();
        match handler {
            Some(handler) => handler(host, &req),
            None => Err(super::no_handler()),
        }
    }
}
