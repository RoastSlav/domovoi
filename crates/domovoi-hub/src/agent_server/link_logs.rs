//! The log reads of [`WsAgentLink`].

use domovoi_core::HostId;
use domovoi_core::ports::{LinkError, LogLine};
use domovoi_proto::Message;
use domovoi_proto::messages::GetLogs;
use jiff::SignedDuration;

use super::{WsAgentLink, unexpected};

impl WsAgentLink {
    /// The recent log lines of the agent on `host`: waits 10 s for a reply.
    /// The window may not fit one frame, so the agent answers in chunks and the
    /// hub asks again with `offset` advanced, until the reply says it has no
    /// more lines. An agent that predates `get_logs` replies `unsupported`,
    /// reported as [`LinkError::Rejected`] telling the person to update the agent.
    pub async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<LogLine>, LinkError> {
        let deadline = self.clock.now() + SignedDuration::from_secs(10);
        let window = usize::from(lines);
        let mut got: Vec<LogLine> = Vec::new();
        loop {
            let get = GetLogs {
                lines,
                min_level: min_level.clone(),
                // The window is at most `lines`, which fits in a `u16`.
                offset: got.len().min(window) as u16,
                deadline,
            };
            let reply = match self.request(host, &Message::GetLogs(get), deadline).await? {
                Message::Logs(reply) => reply,
                Message::Unsupported(_) => {
                    if !got.is_empty() {
                        return Ok(got);
                    }
                    return Err(LinkError::Rejected(
                        "the agent is too old to send logs; update it".to_owned(),
                    ));
                }
                Message::Error(err) => {
                    // A full request queue answers with a busy error right away, so
                    // the caller does not wait out the deadline for it.
                    return Err(LinkError::Rejected(err.message));
                }
                other => return Err(unexpected(&other)),
            };
            let more = reply.has_more;
            let empty = reply.lines.is_empty();
            got.extend(reply.lines);
            if got.len() >= window || !more || empty {
                break;
            }
        }
        got.truncate(window);
        Ok(got)
    }
}
