//! The package half of [`WsAgentLink`]: transfer frames and `agent.package.*` requests.

use std::collections::BTreeMap;

use domovoi_core::install::MAX_CHUNK_BYTES;
use domovoi_core::ports::LinkError;
use domovoi_core::{
    AgentSessionId, CapabilityHandoff, HostId, JournalEpoch, PackageDispatch, PackageOutputs,
    TargetKey, TransferAnswer, TransferFrame, TransferKind,
};
use domovoi_proto::messages::{
    AuthorizationKind, CapabilityOutcome, RunCapability, TransferBegin, TransferChunk, TransferEnd,
    TransferKind as WireKind, TransferOutcome,
};
use domovoi_proto::{Base64Bytes, Sha256Bytes};
use domovoi_proto::{Message, Open};
use jiff::Timestamp;

use super::{PendingRequest, WsAgentLink};

/// The contract version every package capability is requested at.
const CONTRACT_VERSION: u32 = 1;

impl WsAgentLink {
    /// Sends one transfer frame. `Begin` and `Chunk` return once the frame is queued; `End`
    /// waits for the agent's verdict on that frame.
    pub(super) async fn transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        if self.clock.now() >= deadline {
            return Err(LinkError::Timeout);
        }
        let transfer = frame.transfer();
        let is_end = matches!(frame, TransferFrame::End { .. });
        let message = wire_frame(frame)?;
        if !is_end {
            let queued = self
                .registry
                .dispatch_reserved(host, &message, expected_session, &*self.clock, deadline)
                .await;
            return match queued {
                Ok(_) => Ok(None),
                Err(err) => Err(self.queue_failure(host, deadline, err).await),
            };
        }
        let queued = self
            .registry
            .dispatch_reserved_awaiting(host, &message, expected_session, &*self.clock, deadline)
            .await;
        let (id, answer) = match queued {
            Ok(parts) => parts,
            Err(err) => return Err(self.queue_failure(host, deadline, err).await),
        };
        let _pending = PendingRequest {
            registry: &self.registry,
            host,
            id,
        };
        // Once the End frame is queued the agent may have stored the bytes, so no failure
        // below is proof that it did not.
        let reply = tokio::select! {
            reply = answer => reply.map_err(|_| {
                LinkError::Indeterminate("the agent disconnected before answering".to_owned())
            }),
            () = self.clock.sleep_until(deadline) => Err(LinkError::Indeterminate(
                "the agent did not answer the end of the transfer in time".to_owned(),
            )),
        };
        match reply? {
            Message::TransferResult(result) if result.transfer_id == transfer => {
                Ok(Some(match result.outcome {
                    TransferOutcome::Stored => TransferAnswer::Stored,
                    TransferOutcome::Failed { code, detail } => TransferAnswer::Failed {
                        code: code.raw().to_owned(),
                        detail,
                    },
                }))
            }
            Message::Refused(refused) => Err(LinkError::Rejected(format!(
                "{}: {}",
                refused.code.raw(),
                refused.detail
            ))),
            Message::Error(err) => Err(LinkError::Rejected(format!(
                "{}: {}",
                err.code, err.message
            ))),
            other => Err(LinkError::Indeterminate(format!(
                "the agent answered with {}",
                other.kind()
            ))),
        }
    }

    /// Queues one package capability request, to exactly the session and journal epoch given.
    pub(super) async fn capability_dispatch(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        if self.clock.now() >= deadline {
            return Err(LinkError::Timeout);
        }
        let message = Message::RunCapability(run_capability_of(&req)?);
        let (id, answer, _facts) = match self.registry.dispatch(
            host,
            &message,
            Some(expected_session),
            Some(expected_epoch),
        ) {
            Ok(parts) => parts,
            Err(LinkError::NotConnected) => return Err(self.not_connected(host, deadline).await),
            Err(err) => return Err(err),
        };
        self.sends().insert((host, id), answer);
        Ok(CapabilityHandoff::new(id))
    }

    /// Waits for the terminal answer to a request queued by [`Self::capability_dispatch`].
    pub(super) async fn capability_answer(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        let id = handoff.request();
        let Some(answer) = self.sends().remove(&(host, id)) else {
            return Err(LinkError::Indeterminate(
                "the request was no longer awaited".to_owned(),
            ));
        };
        let _pending = PendingRequest {
            registry: &self.registry,
            host,
            id,
        };
        let reply = tokio::select! {
            reply = answer => reply.map_err(|_| {
                LinkError::Indeterminate("the agent disconnected before answering".to_owned())
            }),
            () = self.clock.sleep_until(deadline) => Err(LinkError::Timeout),
        };
        match reply? {
            Message::CapabilityResult(result) => match result.outcome {
                CapabilityOutcome::Succeeded { outputs } => Ok(PackageOutputs(outputs)),
                // The agent's own text already reads `<code>: <detail>`.
                CapabilityOutcome::Failed { message, .. } => Err(LinkError::Rejected(message)),
            },
            Message::Refused(refused) => Err(LinkError::Rejected(format!(
                "{}: {}",
                refused.code.raw(),
                refused.detail
            ))),
            // `in_progress` and `outcome_unknown` are the journal's answers for an execution
            // that is still running or whose end was lost: neither says it did not happen.
            Message::Error(err) => Err(LinkError::Indeterminate(format!(
                "{}: {}",
                err.code, err.message
            ))),
            other => Err(LinkError::Indeterminate(format!(
                "the agent answered with {}",
                other.kind()
            ))),
        }
    }

    /// What a queueing failure means: a host with no session is told apart from an incompatible
    /// agent as every other request does, and any other failure passes through.
    async fn queue_failure(&self, host: HostId, deadline: Timestamp, err: LinkError) -> LinkError {
        match err {
            LinkError::NotConnected => self.not_connected(host, deadline).await,
            other => other,
        }
    }
}

/// The wire message of one transfer frame, refusing a chunk the wire could not carry.
fn wire_frame(frame: TransferFrame) -> Result<Message, LinkError> {
    Ok(match frame {
        TransferFrame::Begin {
            transfer,
            kind,
            size,
            sha256,
        } => Message::TransferBegin(TransferBegin {
            transfer_id: transfer,
            kind: Open::new(
                match kind {
                    TransferKind::PackageArtifact => "package_artifact",
                    TransferKind::PackageManifest => "package_manifest",
                },
                Some(match kind {
                    TransferKind::PackageArtifact => WireKind::PackageArtifact,
                    TransferKind::PackageManifest => WireKind::PackageManifest,
                }),
            ),
            size,
            sha256: Sha256Bytes(sha256),
        }),
        TransferFrame::Chunk {
            transfer,
            seq,
            bytes,
        } => {
            if bytes.is_empty() || bytes.len() > MAX_CHUNK_BYTES {
                return Err(LinkError::Rejected(format!(
                    "a chunk carries 1 to {MAX_CHUNK_BYTES} bytes, not {}",
                    bytes.len()
                )));
            }
            Message::TransferChunk(TransferChunk {
                transfer_id: transfer,
                seq,
                bytes: Base64Bytes(bytes),
            })
        }
        TransferFrame::End { transfer } => Message::TransferEnd(TransferEnd {
            transfer_id: transfer,
        }),
    })
}

/// The `run_capability` request of one package call: contract, target and arguments come from
/// the closed call, and the authorization is always a human's.
fn run_capability_of(req: &PackageDispatch) -> Result<RunCapability, LinkError> {
    let target = TargetKey::parse(&req.call.target_text())
        .map_err(|err| LinkError::Rejected(format!("the package target is invalid: {err}")))?;
    let args: BTreeMap<String, String> = req
        .call
        .args()
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
    Ok(RunCapability {
        execution_id: req.execution.as_uuid(),
        contract: req.call.contract_id().to_owned(),
        version: CONTRACT_VERSION,
        target,
        args,
        plan_hash: req.plan_hash,
        authorization_kind: AuthorizationKind::Human,
        expires_at: req.expires_at,
        expected_safety: None,
    })
}
