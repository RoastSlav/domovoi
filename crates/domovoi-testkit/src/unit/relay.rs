//! A scripted [`UnitRelay`]: identity reads answered by name, restarts handed off in order, and
//! every call recorded with the order it happened in.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::id::RequestId;
use domovoi_core::install::CapabilityHandoff;
use domovoi_core::ports::{LinkError, UnitRelay};
use domovoi_core::unit::{UnitDispatch, UnitName, UnitOutputs, keys};
use domovoi_core::{AgentSessionId, HostId, JournalEpoch};
use jiff::Timestamp;

use crate::lock;

/// The outputs a helper answers `unit.identity` with, for `id` and a hash of 64 `hash_byte`
/// characters, on a platform of `tier`.
#[must_use]
pub fn identity_outputs(id: &str, hash_char: char, tier: &str, opt_in: bool) -> UnitOutputs {
    let mut out = BTreeMap::new();
    out.insert(keys::OUT_ID.to_owned(), id.to_owned());
    out.insert(
        keys::OUT_IDENTITY_HASH.to_owned(),
        hash_char.to_string().repeat(64),
    );
    out.insert(keys::OUT_NAMES.to_owned(), format!("[\"{id}\"]"));
    out.insert(keys::OUT_UNIT_FILE_STATE.to_owned(), "enabled".to_owned());
    out.insert(
        keys::OUT_FRAGMENT.to_owned(),
        format!(
            "{{\"path\":\"/etc/systemd/system/{id}\",\"sha256\":\"{}\"}}",
            "a".repeat(64)
        ),
    );
    out.insert(keys::OUT_DROP_INS.to_owned(), "[]".to_owned());
    out.insert(
        keys::OUT_LOADED.to_owned(),
        "{\"exec_start\":[\"/usr/bin/example\",\"--serve\"],\"user\":\"\",\"working_directory\":\"\",\"followers\":[]}"
            .to_owned(),
    );
    out.insert(keys::OUT_ACTIVE_STATE.to_owned(), "active".to_owned());
    out.insert(keys::OUT_PLATFORM_TIER.to_owned(), tier.to_owned());
    out.insert(keys::OUT_WRITE_OPT_IN.to_owned(), opt_in.to_string());
    UnitOutputs(out)
}

#[derive(Default)]
struct Script {
    identities: BTreeMap<String, Result<UnitOutputs, LinkError>>,
    results: VecDeque<Result<UnitOutputs, LinkError>>,
    dispatch_error: Option<LinkError>,
    dispatches: Vec<UnitDispatch>,
    reads: Vec<String>,
    events: Vec<String>,
}

/// A relay that never touches a link.
#[derive(Default)]
pub struct FakeUnitRelay {
    script: Mutex<Script>,
}

impl FakeUnitRelay {
    /// A relay with nothing scripted: every read fails.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers a read of `name` with `outputs`.
    pub fn set_identity(&self, name: &str, outputs: UnitOutputs) {
        lock(&self.script)
            .identities
            .insert(name.to_owned(), Ok(outputs));
    }

    /// Fails a read of `name` with `error`.
    pub fn fail_identity(&self, name: &str, error: LinkError) {
        lock(&self.script)
            .identities
            .insert(name.to_owned(), Err(error));
    }

    /// Queues what the next `restart_result` answers.
    pub fn queue_result(&self, result: Result<UnitOutputs, LinkError>) {
        lock(&self.script).results.push_back(result);
    }

    /// Makes the next `dispatch_restart` fail, as a link that queued nothing does.
    pub fn fail_dispatch(&self, error: LinkError) {
        lock(&self.script).dispatch_error = Some(error);
    }

    /// Every restart handed off, oldest first.
    #[must_use]
    pub fn dispatches(&self) -> Vec<UnitDispatch> {
        lock(&self.script).dispatches.clone()
    }

    /// The names read so far, oldest first.
    #[must_use]
    pub fn reads(&self) -> Vec<String> {
        lock(&self.script).reads.clone()
    }

    /// Every call in the order it happened: `read:<name>`, `dispatch`, `result`.
    #[must_use]
    pub fn events(&self) -> Vec<String> {
        lock(&self.script).events.clone()
    }
}

#[async_trait]
impl UnitRelay for FakeUnitRelay {
    async fn read_identity(
        &self,
        _host: HostId,
        name: &UnitName,
        _session: AgentSessionId,
        _deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError> {
        let mut script = lock(&self.script);
        script.reads.push(name.to_string());
        script.events.push(format!("read:{name}"));
        script
            .identities
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(|| Err(LinkError::Transport("no identity scripted".to_owned())))
    }

    async fn dispatch_restart(
        &self,
        _host: HostId,
        req: UnitDispatch,
        _expected_session: AgentSessionId,
        _expected_epoch: JournalEpoch,
        _deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        let mut script = lock(&self.script);
        if let Some(error) = script.dispatch_error.take() {
            return Err(error);
        }
        script.dispatches.push(req);
        script.events.push("dispatch".to_owned());
        Ok(CapabilityHandoff::new(RequestId::new()))
    }

    async fn restart_result(
        &self,
        _host: HostId,
        _handoff: CapabilityHandoff,
        _deadline: Timestamp,
    ) -> Result<UnitOutputs, LinkError> {
        let mut script = lock(&self.script);
        script.events.push("result".to_owned());
        script
            .results
            .pop_front()
            .unwrap_or(Err(LinkError::Timeout))
    }
}
