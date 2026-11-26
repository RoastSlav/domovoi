//! A scripted [`ContainRelay`]: phases handed off in order, answers queued per phase, every call
//! recorded with the order it happened in.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::containment::{ContainDispatch, ContainOutputs};
use domovoi_core::id::RequestId;
use domovoi_core::install::CapabilityHandoff;
use domovoi_core::ports::{BlockPhase, ContainCaps, ContainRelay, ContainmentRepo, LinkError};
use domovoi_core::{AgentSessionId, HostId, JournalEpoch};
use jiff::{SignedDuration, Timestamp};

use crate::clock::FakeClock;
use crate::link::FakeAgentLink;
use crate::lock;

#[derive(Default)]
struct Script {
    results: VecDeque<Result<ContainOutputs, LinkError>>,
    dispatch_errors: VecDeque<Option<LinkError>>,
    dispatches: Vec<ContainDispatch>,
    events: Vec<String>,
    advance: Option<(Arc<FakeClock>, SignedDuration)>,
    seen: Vec<(String, Option<BlockPhase>)>,
}

/// A relay that never touches a link.
#[derive(Default)]
pub struct FakeContainRelay {
    script: Mutex<Script>,
    watch: Mutex<Option<Arc<dyn ContainmentRepo>>>,
}

impl FakeContainRelay {
    /// A relay with nothing scripted: every result times out.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues what the next `result` answers.
    pub fn queue_result(&self, result: Result<ContainOutputs, LinkError>) {
        lock(&self.script).results.push_back(result);
    }

    /// Queues what the next `dispatch` does: `None` hands the frame off, `Some` fails as a link
    /// that queued nothing does.
    pub fn queue_dispatch(&self, outcome: Option<LinkError>) {
        lock(&self.script).dispatch_errors.push_back(outcome);
    }

    /// Makes every `dispatch_phase` read the block's stored phase from `repo` before it
    /// answers, so a test can prove a phase was committed before its frame was sent.
    pub fn watch(&self, repo: Arc<dyn ContainmentRepo>) {
        *lock(&self.watch) = Some(repo);
    }

    /// For every request handed off, the phase the watched repo held for its block at that
    /// moment, oldest first, as `(dispatch:<phase>, stored phase)`.
    #[must_use]
    pub fn seen_phases(&self) -> Vec<(String, Option<BlockPhase>)> {
        lock(&self.script).seen.clone()
    }

    /// Every request handed off, oldest first.
    #[must_use]
    pub fn dispatches(&self) -> Vec<ContainDispatch> {
        lock(&self.script).dispatches.clone()
    }

    /// Every call in the order it happened: `dispatch:<phase>` and `result`.
    #[must_use]
    pub fn events(&self) -> Vec<String> {
        lock(&self.script).events.clone()
    }
}

#[async_trait]
impl ContainRelay for FakeContainRelay {
    async fn dispatch_phase(
        &self,
        _host: HostId,
        req: ContainDispatch,
        _expected_session: AgentSessionId,
        _expected_epoch: JournalEpoch,
        _deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        let repo = lock(&self.watch).clone();
        let stored = match (&repo, req.call().spec()) {
            (Some(repo), Some(spec)) => repo
                .get_block(spec.block)
                .await
                .ok()
                .flatten()
                .map(|row| row.phase),
            _ => None,
        };
        let mut script = lock(&self.script);
        if let Some(Some(error)) = script.dispatch_errors.pop_front() {
            return Err(error);
        }
        if let Some((clock, by)) = &script.advance {
            clock.advance(*by);
        }
        let phase = req.call().phase().unwrap_or("list");
        script.events.push(format!("dispatch:{phase}"));
        script.seen.push((format!("dispatch:{phase}"), stored));
        script.dispatches.push(req);
        Ok(CapabilityHandoff::new(RequestId::new()))
    }

    async fn phase_result(
        &self,
        _host: HostId,
        _handoff: CapabilityHandoff,
        _deadline: Timestamp,
    ) -> Result<ContainOutputs, LinkError> {
        let mut script = lock(&self.script);
        script.events.push("result".to_owned());
        script
            .results
            .pop_front()
            .unwrap_or(Err(LinkError::Timeout))
    }
}

/// The capabilities containment needs: the scripted fake for presence, the session and the
/// journal, and `relay` for the containment calls themselves.
pub fn contain_caps(link: Arc<FakeAgentLink>, relay: Arc<FakeContainRelay>) -> ContainCaps {
    let caps = crate::link::unit_caps(link, Arc::new(crate::unit::FakeUnitRelay::new()));
    ContainCaps {
        probe: caps.probe,
        observer: caps.observer,
        journal: caps.journal,
        relay,
    }
}
