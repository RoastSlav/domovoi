//! The stream state machine: a cursor over the journal, never a queue per client.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::response::sse::Event;
use domovoi_core::ports::{Clock, JournalFeed, Redactor, TopicSet};
use jiff::Timestamp;

use super::wire::{self, JournalFrame};
use super::{BATCH_ROWS, KEEP_ALIVE, LAG_LIMIT_ROWS, StreamGuard};

/// Everything one open connection owns.
pub(super) struct Machine {
    pub feed: Arc<dyn JournalFeed>,
    pub clock: Arc<dyn Clock>,
    pub redactor: Arc<dyn Redactor>,
    pub topics: TopicSet,
    /// The position the client asked to resume from, until the first frame is chosen.
    pub resume: Option<u64>,
    pub started: bool,
    pub cursor: u64,
    /// At most one batch of frames waiting to be written to the socket.
    pub pending: VecDeque<Event>,
    pub last_send: Timestamp,
    pub _guard: StreamGuard,
}

/// Why a cursor can no longer be served from the journal.
fn reset_reason(cursor: u64, head: u64, floor: u64) -> Option<&'static str> {
    if cursor < floor {
        Some("pruned")
    } else if cursor > head {
        Some("ahead")
    } else if head - cursor > LAG_LIMIT_ROWS {
        Some("lagged")
    } else {
        None
    }
}

impl Machine {
    fn push(&mut self, event: Option<Event>) {
        if let Some(event) = event {
            self.pending.push_back(event);
        }
    }

    fn reset(&mut self, reason: &str, head: u64) {
        metrics::counter!("domovoi_sse_resets_total", "reason" => reason.to_owned()).increment(1);
        self.push(wire::reset(reason, head));
        self.cursor = head;
    }

    /// The first frame: `ready`, or a `reset` when the requested position cannot be served.
    async fn open(&mut self) -> Result<(), domovoi_core::ports::StoreError> {
        let head = self.feed.head_seq().await?;
        let floor = self.feed.floor_seq().await?;
        match self.resume {
            None => {
                self.push(wire::ready(head, false, &self.topics.wire()));
                self.cursor = head;
            }
            Some(from) => match reset_reason(from, head, floor) {
                Some(reason) => self.reset(reason, head),
                None => {
                    self.push(wire::ready(head, true, &self.topics.wire()));
                    self.cursor = from;
                }
            },
        }
        Ok(())
    }

    /// Reads whatever is committed past the cursor, or waits for it.
    async fn advance(&mut self) -> Result<(), domovoi_core::ports::StoreError> {
        let head = self.feed.head_seq().await?;
        let floor = self.feed.floor_seq().await?;
        if let Some(reason) = reset_reason(self.cursor, head, floor) {
            self.reset(reason, head);
            return Ok(());
        }
        if head > self.cursor {
            let rows = self.feed.entries_after(self.cursor, BATCH_ROWS).await?;
            if rows.is_empty() {
                self.cursor = head;
            }
            for row in rows {
                self.cursor = row.seq;
                if self.topics.matches(&row) {
                    self.push(JournalFrame::new(&row, &*self.redactor).event());
                }
            }
            return Ok(());
        }
        let feed = self.feed.clone();
        let clock = self.clock.clone();
        let cursor = self.cursor;
        let deadline = self.last_send + KEEP_ALIVE;
        tokio::select! {
            () = feed.wait_after(cursor) => {}
            () = clock.sleep_until(deadline) => {
                self.pending.push_back(wire::keepalive());
            }
        }
        Ok(())
    }

    /// The next frame to write, or `None` when the journal cannot be read any more.
    pub(super) async fn next(&mut self) -> Option<Event> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                self.last_send = self.clock.now();
                return Some(event);
            }
            let step = if self.started {
                self.advance().await
            } else {
                self.started = true;
                self.open().await
            };
            if let Err(err) = step {
                tracing::warn!(error = %err, "ending an event stream: the journal could not be read");
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cursor_is_served_only_between_the_floor_and_the_head() {
        assert_eq!(reset_reason(5, 10, 3), None);
        assert_eq!(reset_reason(3, 10, 3), None);
        assert_eq!(reset_reason(2, 10, 3), Some("pruned"));
        assert_eq!(reset_reason(11, 10, 3), Some("ahead"));
        assert_eq!(reset_reason(0, LAG_LIMIT_ROWS + 1, 0), Some("lagged"));
        assert_eq!(reset_reason(1, LAG_LIMIT_ROWS + 1, 0), None);
    }
}
