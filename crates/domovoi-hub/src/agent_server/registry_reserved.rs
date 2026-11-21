//! Queueing frames that wait for room in a session's outbox instead of failing when it is
//! full: the way a transfer of many chunks reaches an agent without a "busy" refusal.

use domovoi_core::ports::{Clock, LinkError};
use domovoi_core::{AgentSessionId, HostId, RequestId};
use domovoi_proto::{Message, encode};
use jiff::Timestamp;
use tokio::sync::{mpsc, oneshot};

use super::{MAX_PENDING, Registry, busy};

impl Registry {
    /// Queues `message` for the host's agent once the session outbox has room, without
    /// registering an answer. Returns the id the frame was tagged with.
    ///
    /// The session is checked against `expected_session` twice, each time under the registry
    /// lock: before waiting, so a call for a gone session fails at once, and again at the
    /// moment the frame is queued, so a replacement session can never receive it. The wait for
    /// room holds no lock and ends with [`LinkError::Timeout`] at `deadline` on `clock`.
    pub async fn dispatch_reserved(
        &self,
        host: HostId,
        message: &Message,
        expected_session: AgentSessionId,
        clock: &dyn Clock,
        deadline: Timestamp,
    ) -> Result<RequestId, LinkError> {
        let (id, _answer) = self
            .reserved(host, message, expected_session, clock, deadline, false)
            .await?;
        Ok(id)
    }

    /// Like [`Registry::dispatch_reserved`], and registers the answer to the frame in the same
    /// step that queues it, so a reply cannot arrive before anyone waits for it.
    pub async fn dispatch_reserved_awaiting(
        &self,
        host: HostId,
        message: &Message,
        expected_session: AgentSessionId,
        clock: &dyn Clock,
        deadline: Timestamp,
    ) -> Result<(RequestId, oneshot::Receiver<Message>), LinkError> {
        let (id, answer) = self
            .reserved(host, message, expected_session, clock, deadline, true)
            .await?;
        answer.map(|rx| (id, rx)).ok_or(LinkError::NotConnected)
    }

    async fn reserved(
        &self,
        host: HostId,
        message: &Message,
        expected_session: AgentSessionId,
        clock: &dyn Clock,
        deadline: Timestamp,
        awaiting: bool,
    ) -> Result<(RequestId, Option<oneshot::Receiver<Message>>), LinkError> {
        let id = RequestId::new();
        let frame = encode(message, id, None).map_err(|e| LinkError::Transport(e.to_string()))?;
        let sender = self.outbox_of(host, expected_session)?;
        let permit = tokio::select! {
            permit = sender.reserve() => {
                permit.map_err(|_| LinkError::SessionChanged)?
            }
            () = clock.sleep_until(deadline) => return Err(LinkError::Timeout),
        };
        // Queued and registered in one step under the lock, after the session is checked
        // again: nothing awaits between the check and the send.
        let mut hosts = self.lock();
        let live = hosts
            .get_mut(&host)
            .and_then(|e| e.live.as_mut())
            .ok_or(LinkError::NotConnected)?;
        if live.identity != expected_session {
            return Err(LinkError::SessionChanged);
        }
        let answer = if awaiting {
            if live.pending.len() >= MAX_PENDING {
                return Err(busy());
            }
            let (tx, rx) = oneshot::channel();
            live.pending.insert(id, tx);
            Some(rx)
        } else {
            None
        };
        permit.send(frame);
        Ok((id, answer))
    }

    /// A clone of the outbox of the host's live session, which must be `expected_session`.
    fn outbox_of(
        &self,
        host: HostId,
        expected_session: AgentSessionId,
    ) -> Result<mpsc::Sender<String>, LinkError> {
        let hosts = self.lock();
        let live = hosts
            .get(&host)
            .and_then(|e| e.live.as_ref())
            .ok_or(LinkError::NotConnected)?;
        if live.identity != expected_session {
            return Err(LinkError::SessionChanged);
        }
        Ok(live.outbox.clone())
    }
}
