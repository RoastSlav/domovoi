//! Typed messages over a [`Transport`].

use domovoi_core::RequestId;
use domovoi_proto::{Decoded, Message, Transport, decode, encode};

use crate::error::LinkError;

/// Encodes and sends `message`, as a reply to `reply_to` when given. Returns its id.
pub async fn send(
    transport: &mut dyn Transport,
    message: &Message,
    reply_to: Option<RequestId>,
) -> Result<RequestId, LinkError> {
    let id = RequestId::new();
    let frame = encode(message, id, reply_to).map_err(|e| LinkError::Protocol(e.to_string()))?;
    transport
        .send(frame)
        .await
        .map_err(|e| LinkError::Lost(e.to_string()))?;
    Ok(id)
}

/// Receives and decodes the next message; `None` when the hub closed the connection.
pub async fn recv(transport: &mut dyn Transport) -> Result<Option<Decoded>, LinkError> {
    let Some(frame) = transport
        .recv()
        .await
        .map_err(|e| LinkError::Lost(e.to_string()))?
    else {
        return Ok(None);
    };
    decode(&frame)
        .map(Some)
        .map_err(|e| LinkError::Protocol(e.to_string()))
}

/// Receives the next message, treating a closed connection as lost.
pub async fn expect(transport: &mut dyn Transport) -> Result<Decoded, LinkError> {
    recv(transport)
        .await?
        .ok_or_else(|| LinkError::Lost("the hub closed the connection".to_owned()))
}
