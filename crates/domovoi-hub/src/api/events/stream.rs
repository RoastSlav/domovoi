//! The stream state machine: a cursor over the journal, never a queue per client.

use std::collections::VecDeque;
use std::sync::Arc;

use axum::response::sse::Event;
use domovoi_core::ports::{Clock, JournalFeed, Redactor, TopicSet};
use jiff::Timestamp;
use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::wire::{self, JournalFrame};
use super::{BATCH_ROWS, KEEP_ALIVE, LAG_LIMIT_ROWS, StreamGuard};
use crate::api::operator::live::LiveFrame;

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
    /// Live frames of answers being written, only for a connection that follows a conversation.
    pub live: Option<Receiver<LiveFrame>>,
    pub _guard: StreamGuard,
}

/// The next live frame, or never when this connection has no live receiver.
async fn next_live(live: &mut Option<Receiver<LiveFrame>>) -> Result<LiveFrame, RecvError> {
    match live {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
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
            // Retention may have pruned between the floor check and the read; a batch that
            // starts past deleted rows must not be sent as if it were complete.
            let floor_after = self.feed.floor_seq().await?;
            if self.cursor < floor_after {
                let head = self.feed.head_seq().await?;
                self.reset("pruned", head);
                return Ok(());
            }
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
        let mut live = self.live.take();
        let mut keep_live = true;
        tokio::select! {
            () = feed.wait_after(cursor) => {}
            () = clock.sleep_until(deadline) => {
                self.pending.push_back(wire::keepalive());
            }
            frame = next_live(&mut live) => match frame {
                Ok(frame) => {
                    // Live frames never move the cursor and are never replayed.
                    if self.topics.contains_conversation(&frame.conversation) {
                        self.pending.push_back(wire::live_event(&frame));
                    }
                }
                Err(RecvError::Lagged(missed)) => {
                    metrics::counter!("domovoi_sse_live_dropped_total").increment(missed);
                }
                Err(RecvError::Closed) => keep_live = false,
            },
        }
        if keep_live {
            self.live = live;
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

    use std::sync::atomic::{AtomicBool, Ordering};

    use async_trait::async_trait;
    use domovoi_core::ports::{
        AggregateRef, JournalDraft, JournalEntry, JournalKind, JournalRead, StoreError,
        Transaction, UnitOfWork,
    };
    use domovoi_core::{Actor, IncidentId};
    use domovoi_testkit::{FakeClock, InMemoryStore};

    /// A feed that prunes the journal while a batch is being read.
    struct PrunesDuringRead {
        store: Arc<InMemoryStore>,
        armed: AtomicBool,
    }

    #[async_trait]
    impl JournalRead for PrunesDuringRead {
        async fn entries_after(
            &self,
            after: u64,
            limit: u16,
        ) -> Result<Vec<JournalEntry>, StoreError> {
            let rows = self.store.entries_after(after, limit).await?;
            if self.armed.swap(false, Ordering::SeqCst) {
                self.store.prune_journal_through(after + 3);
            }
            Ok(rows)
        }
        async fn entries_for(
            &self,
            a: &AggregateRef,
            limit: u16,
        ) -> Result<Vec<JournalEntry>, StoreError> {
            self.store.entries_for(a, limit).await
        }
        async fn head_seq(&self) -> Result<u64, StoreError> {
            self.store.head_seq().await
        }
    }

    #[async_trait]
    impl JournalFeed for PrunesDuringRead {
        async fn floor_seq(&self) -> Result<u64, StoreError> {
            self.store.floor_seq().await
        }
        async fn wait_after(&self, seq: u64) {
            self.store.wait_after(seq).await;
        }
    }

    #[tokio::test]
    async fn pruning_between_the_floor_check_and_the_read_resets_instead_of_losing_rows() {
        let store = Arc::new(InMemoryStore::new());
        let mut tx = Transaction::new();
        for _ in 0..10 {
            tx = tx.journal(JournalDraft {
                aggregate: AggregateRef::incident(IncidentId::new()),
                kind: JournalKind::IncidentTimeline,
                payload_v: 1,
                payload: serde_json::json!({"kind": "k", "summary": "s"}),
                actor: Actor::System("test".to_owned()),
                occurred_at: Timestamp::UNIX_EPOCH,
            });
        }
        store.commit(tx).await.unwrap();
        let feed = Arc::new(PrunesDuringRead {
            store: store.clone(),
            armed: AtomicBool::new(true),
        });
        let open = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut machine = Machine {
            feed,
            clock: Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
            redactor: Arc::new(domovoi_engine::PatternRedactor::default()),
            topics: TopicSet::default_set(),
            resume: Some(2),
            started: true,
            cursor: 2,
            pending: VecDeque::new(),
            last_send: Timestamp::UNIX_EPOCH,
            live: None,
            _guard: StreamGuard::acquire(&open).unwrap(),
        };
        machine.advance().await.unwrap();
        let frames: Vec<String> = machine.pending.iter().map(|e| format!("{e:?}")).collect();
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(
            frames[0].contains("reset") && frames[0].contains("pruned"),
            "{frames:?}"
        );
        assert_eq!(machine.cursor, 10);
    }
}
