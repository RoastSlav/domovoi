//! Forwards the hub's `transfer_*` frames to the package transfer receiver.
//!
//! The frames of one transfer must stay in order, so they bypass the parallel request
//! handler and go through their own channel to a single receiver task. Without a receiver
//! (no `[packages]` section) the link ignores `transfer_begin` and `transfer_chunk` and
//! answers `transfer_end` at once, so the hub never waits for a result that cannot come.

use domovoi_core::RequestId;
use domovoi_proto::messages::{
    TransferBegin, TransferChunk, TransferEnd, TransferFailureCode, TransferOutcome, TransferResult,
};
use domovoi_proto::{Message, Open, Transport};

use super::{Link, frame};
use crate::error::LinkError;

/// One `transfer_*` frame and the envelope id its answer must carry.
#[derive(Debug, Clone, PartialEq)]
pub struct IncomingTransfer {
    /// The envelope id of the frame.
    pub id: RequestId,
    /// The frame.
    pub frame: TransferFrame,
}

/// The frames the receiver acts on.
#[derive(Debug, Clone, PartialEq)]
pub enum TransferFrame {
    /// `transfer_begin`.
    Begin(TransferBegin),
    /// `transfer_chunk`.
    Chunk(TransferChunk),
    /// `transfer_end`; the receiver answers it.
    End(TransferEnd),
    /// The link session ended: transfers still arriving are dropped.
    Reset,
}

impl Link {
    /// Hands `frame` to the receiver, or answers `transfer_end` when there is none.
    pub(super) async fn transfer(
        &self,
        transport: &mut dyn Transport,
        id: RequestId,
        frame: TransferFrame,
    ) -> Result<(), LinkError> {
        let Some(receiver) = &self.parts.transfers else {
            if let TransferFrame::End(end) = frame {
                let reply = Message::TransferResult(TransferResult {
                    transfer_id: end.transfer_id,
                    outcome: TransferOutcome::Failed {
                        code: Open::new("aborted", Some(TransferFailureCode::Aborted)),
                        detail: "package installation is not enabled on this agent".to_owned(),
                    },
                });
                frame::send(transport, &reply, Some(id)).await?;
            }
            return Ok(());
        };
        if receiver.send(IncomingTransfer { id, frame }).await.is_err() {
            tracing::warn!("the transfer receiver stopped");
        }
        Ok(())
    }

    /// Tells the receiver the session ended so it drops transfers still arriving.
    pub(super) fn end_transfers(&self) {
        if let Some(receiver) = &self.parts.transfers {
            let _ = receiver.try_send(IncomingTransfer {
                id: RequestId::new(),
                frame: TransferFrame::Reset,
            });
        }
    }
}
