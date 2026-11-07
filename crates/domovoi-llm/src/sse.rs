//! Server-sent events: the framing both streaming APIs share.

use std::collections::VecDeque;

use domovoi_core::ports::LlmError;
use secrecy::SecretString;

use crate::http::send_error;

/// Splits a byte stream into events and keeps the `data:` payload of each.
#[derive(Debug, Default)]
pub(crate) struct SseBuffer {
    partial: Vec<u8>,
    data: Vec<String>,
}

impl SseBuffer {
    /// Feeds `bytes` and returns the payloads of the events they completed.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(bytes);
        let mut ready = Vec::new();
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            self.line(line.trim_end_matches(['\r', '\n']), &mut ready);
        }
        ready
    }

    /// Ends the input and returns an event the server never terminated with a blank line.
    pub(crate) fn finish(&mut self) -> Vec<String> {
        let mut ready = Vec::new();
        if !self.partial.is_empty() {
            let rest = std::mem::take(&mut self.partial);
            self.line(String::from_utf8_lossy(&rest).trim_end(), &mut ready);
        }
        self.dispatch(&mut ready);
        ready
    }

    fn line(&mut self, line: &str, ready: &mut Vec<String>) {
        if line.is_empty() {
            self.dispatch(ready);
        } else if let Some(value) = line.strip_prefix("data:") {
            self.data
                .push(value.strip_prefix(' ').unwrap_or(value).to_owned());
        }
        // `event:`, `id:`, `retry:` and comments carry nothing either API relies on: the
        // payloads name their own type.
    }

    fn dispatch(&mut self, ready: &mut Vec<String>) {
        if !self.data.is_empty() {
            ready.push(self.data.join("\n"));
            self.data.clear();
        }
    }
}

/// A successful response read as server-sent events.
pub(crate) struct Events {
    response: reqwest::Response,
    buffer: SseBuffer,
    ready: VecDeque<String>,
}

impl Events {
    pub(crate) fn new(response: reqwest::Response) -> Self {
        Self {
            response,
            buffer: SseBuffer::default(),
            ready: VecDeque::new(),
        }
    }

    /// The next event's payload, or `None` when the server closed the stream. A read that
    /// stays silent past the client's timeout is an error.
    pub(crate) async fn next_data(
        &mut self,
        secret: Option<&SecretString>,
    ) -> Result<Option<String>, LlmError> {
        loop {
            if let Some(data) = self.ready.pop_front() {
                return Ok(Some(data));
            }
            match self
                .response
                .chunk()
                .await
                .map_err(|e| send_error(&e, secret))?
            {
                Some(chunk) => self.ready.extend(self.buffer.push(&chunk)),
                None => {
                    self.ready.extend(self.buffer.finish());
                    if self.ready.is_empty() {
                        return Ok(None);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SseBuffer;

    #[test]
    fn joins_events_split_across_chunks() {
        let mut buffer = SseBuffer::default();
        assert!(buffer.push(b"event: x\r\ndata: {\"a\"").is_empty());
        let ready = buffer.push(b":1}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(ready, vec!["{\"a\":1}".to_owned(), "[DONE]".to_owned()]);
    }

    #[test]
    fn multi_line_data_is_joined_and_comments_are_skipped() {
        let mut buffer = SseBuffer::default();
        let ready = buffer.push(b": ping\ndata: one\ndata: two\n\n");
        assert_eq!(ready, vec!["one\ntwo".to_owned()]);
    }

    #[test]
    fn an_unterminated_last_event_is_flushed_at_the_end() {
        let mut buffer = SseBuffer::default();
        assert!(buffer.push(b"data: last").is_empty());
        assert_eq!(buffer.finish(), vec!["last".to_owned()]);
    }
}
