//! The receiver for `transfer_begin`, `transfer_chunk` and `transfer_end`.
//!
//! One task drives it, so the frames of a transfer are never reordered. Bytes go to a
//! private `.part` file while a SHA-256 and a counter run alongside; the file becomes a
//! stored transfer only when the size and hash match what `transfer_begin` announced. A
//! failure is remembered and reported once, as the answer to `transfer_end`, because the hub
//! correlates the result with the frame it waits on. Time comes from the `Clock` port.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::sync::Arc;

use domovoi_core::package::{MAX_SIGNATURE_FILE_BYTES, PackageFailureCode};
use domovoi_core::ports::Clock;
use domovoi_proto::messages::{
    TransferBegin, TransferChunk, TransferEnd, TransferFailureCode, TransferKind, TransferOutcome,
    TransferResult,
};
use domovoi_proto::{Message, Open};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest as _, Sha256};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::events::{EventKind, EventSink, PackageEvent};
use super::staging::Staging;
use crate::link::LinkHandle;
use crate::link::transfers::{IncomingTransfer, TransferFrame};

/// Transfers that may be arriving at once.
pub const MAX_ACTIVE: usize = 2;
/// Transfers that may be stored at once.
pub const MAX_STAGED: usize = 8;
/// Records kept at once, arriving or failed and waiting for their `transfer_end`. A failed
/// record past this bound is not kept: its `transfer_end` is answered as an unknown transfer.
pub const MAX_RECORDS: usize = 8;
/// An arriving transfer that sends nothing for this long is dropped.
pub const IDLE_LIMIT: SignedDuration = SignedDuration::from_secs(60);
/// An arriving transfer older than this is dropped.
pub const TOTAL_LIMIT: SignedDuration = SignedDuration::from_mins(10);
/// How often the receiver looks for transfers to drop.
pub const SWEEP_EVERY: SignedDuration = SignedDuration::from_secs(5);

/// Where a transfer stands.
enum State {
    Receiving {
        file: File,
        hasher: Sha256,
        received: u64,
        next_seq: u64,
    },
    Failed(TransferFailureCode, String),
}

/// One transfer between `transfer_begin` and `transfer_end`.
struct Active {
    kind: TransferKind,
    size: u64,
    sha256: [u8; 32],
    started: Timestamp,
    last_activity: Timestamp,
    state: State,
}

const DISABLED: &str = "package installation is not allowed on this agent";
const UNKNOWN_KIND: &str = "unknown transfer kind";

/// The audit code for a transfer failure.
fn audit_code(code: TransferFailureCode, detail: &str) -> PackageFailureCode {
    match (code, detail) {
        (TransferFailureCode::Aborted, DISABLED) => PackageFailureCode::PackagesDisabled,
        (TransferFailureCode::Aborted, UNKNOWN_KIND) => PackageFailureCode::TransferKindMismatch,
        (TransferFailureCode::Aborted, _) => PackageFailureCode::TransferUnknown,
        (TransferFailureCode::ChecksumMismatch, _) => PackageFailureCode::DigestMismatch,
        (TransferFailureCode::TooLarge, _) => PackageFailureCode::TooLarge,
        (TransferFailureCode::Storage, _) => PackageFailureCode::Storage,
    }
}

/// Stores the bytes the hub sends and answers each transfer once.
pub struct TransferReceiver {
    store: Arc<Staging>,
    allow_install: bool,
    compressed_bytes: u64,
    clock: Arc<dyn Clock>,
    events: Arc<dyn EventSink>,
    active: HashMap<Uuid, Active>,
}

impl TransferReceiver {
    /// A receiver storing into `store`. With `allow_install` false every transfer fails
    /// at its end; `compressed_bytes` caps a package artifact. Every failed transfer is
    /// reported once to `events`.
    #[must_use]
    pub fn new(
        store: Arc<Staging>,
        allow_install: bool,
        compressed_bytes: u64,
        clock: Arc<dyn Clock>,
        events: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            store,
            allow_install,
            compressed_bytes,
            clock,
            events,
            active: HashMap::new(),
        }
    }

    /// The staging area, shared with the installer.
    #[must_use]
    pub fn staging(&self) -> Arc<Staging> {
        self.store.clone()
    }

    fn budget(&self) -> u64 {
        self.compressed_bytes
            .saturating_mul(2)
            .saturating_add(MAX_SIGNATURE_FILE_BYTES as u64)
    }

    /// Reports one failed transfer to the audit sink.
    fn rejected(&self, id: Uuid, code: TransferFailureCode, detail: &str) {
        self.events.emit(
            PackageEvent::new(EventKind::TransferRejected)
                .code(audit_code(code, detail))
                .detail(format!("transfer {id}: {detail}")),
        );
    }

    /// Makes room for a record that must be kept by dropping the oldest failed ones.
    fn make_room(&mut self) {
        while self.active.len() >= MAX_RECORDS {
            let oldest = self
                .active
                .iter()
                .filter(|(_, active)| matches!(active.state, State::Failed(..)))
                .min_by_key(|(_, active)| active.started)
                .map(|(id, _)| *id);
            match oldest {
                Some(id) => {
                    self.active.remove(&id);
                }
                None => break,
            }
        }
    }

    fn receiving(&self) -> impl Iterator<Item = &Active> {
        self.active
            .values()
            .filter(|active| matches!(active.state, State::Receiving { .. }))
    }

    /// Starts transfer `begin.transfer_id`, or records why it cannot.
    pub fn begin(&mut self, begin: &TransferBegin) {
        let id = begin.transfer_id;
        let now = self.clock.now();
        if self.active.contains_key(&id) {
            tracing::warn!(%id, "a transfer id was announced twice; the first carries on");
            return;
        }
        let kind = match begin.kind.known() {
            Some(TransferKind::PackageArtifact) => Some(TransferKind::PackageArtifact),
            Some(TransferKind::PackageManifest) => Some(TransferKind::PackageManifest),
            None => None,
        };
        let sha256 = *begin.sha256.as_bytes();
        let fail = |code, detail: &str| Active {
            kind: kind.unwrap_or(TransferKind::PackageArtifact),
            size: begin.size,
            sha256,
            started: now,
            last_activity: now,
            state: State::Failed(code, detail.to_owned()),
        };
        let refusal = self.refusal(begin, kind, id);
        let active = match (refusal, kind) {
            (Some((code, detail)), _) => {
                self.rejected(id, code, detail);
                if self.active.len() >= MAX_RECORDS {
                    // No room to remember it: its `transfer_end` is answered as unknown.
                    return;
                }
                fail(code, detail)
            }
            (None, Some(kind)) => match self.open_part(id) {
                Ok(file) => Active {
                    kind,
                    size: begin.size,
                    sha256,
                    started: now,
                    last_activity: now,
                    state: State::Receiving {
                        file,
                        hasher: Sha256::new(),
                        received: 0,
                        next_seq: 0,
                    },
                },
                Err(err) => {
                    tracing::warn!(%err, "could not create a transfer file");
                    let detail = "could not store the transfer";
                    self.rejected(id, TransferFailureCode::Storage, detail);
                    fail(TransferFailureCode::Storage, detail)
                }
            },
            (None, None) => fail(TransferFailureCode::Aborted, UNKNOWN_KIND),
        };
        if matches!(active.state, State::Receiving { .. }) {
            self.make_room();
        }
        self.active.insert(id, active);
    }

    /// Why `begin` cannot be accepted, in the order of the receiver's rules.
    fn refusal(
        &self,
        begin: &TransferBegin,
        kind: Option<TransferKind>,
        id: Uuid,
    ) -> Option<(TransferFailureCode, &'static str)> {
        let aborted = TransferFailureCode::Aborted;
        if !self.allow_install {
            return Some((aborted, DISABLED));
        }
        let cap = match kind {
            None => return Some((aborted, UNKNOWN_KIND)),
            Some(TransferKind::PackageArtifact) => self.compressed_bytes,
            Some(TransferKind::PackageManifest) => MAX_SIGNATURE_FILE_BYTES as u64,
        };
        if begin.size == 0 || begin.size > cap {
            return Some((
                TransferFailureCode::TooLarge,
                "the announced size is not allowed",
            ));
        }
        if self.store.is_staged(id) {
            return Some((aborted, "duplicate transfer id"));
        }
        let held =
            self.store.staged_bytes() + self.receiving().map(|active| active.size).sum::<u64>();
        if self.receiving().count() >= MAX_ACTIVE
            || self.store.staged_count() >= MAX_STAGED
            || held.saturating_add(begin.size) > self.budget()
        {
            return Some((TransferFailureCode::Storage, "too many staged transfers"));
        }
        None
    }

    fn open_part(&self, id: Uuid) -> std::io::Result<File> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(self.store.part_path(id))
    }

    /// Adds one chunk to its transfer, or fails the transfer.
    pub fn chunk(&mut self, chunk: &TransferChunk) {
        let now = self.clock.now();
        let id = chunk.transfer_id;
        let Some(active) = self.active.get_mut(&id) else {
            tracing::debug!(%id, "a chunk for an unknown transfer");
            return;
        };
        let State::Receiving {
            file,
            hasher,
            received,
            next_seq,
        } = &mut active.state
        else {
            return;
        };
        let bytes = chunk.bytes.as_bytes();
        let failure = if chunk.seq != *next_seq {
            Some((TransferFailureCode::Aborted, "chunk out of order"))
        } else if received.saturating_add(bytes.len() as u64) > active.size {
            Some((TransferFailureCode::TooLarge, "more bytes than announced"))
        } else if file.write_all(bytes).is_err() {
            Some((TransferFailureCode::Storage, "could not store the transfer"))
        } else {
            None
        };
        match failure {
            None => {
                hasher.update(bytes);
                *received += bytes.len() as u64;
                *next_seq += 1;
                active.last_activity = now;
            }
            Some((code, detail)) => {
                active.state = State::Failed(code, detail.to_owned());
                remove(&self.store.part_path(id));
                self.rejected(id, code, detail);
            }
        }
    }

    /// Finishes a transfer and returns the one answer the hub waits for.
    pub fn end(&mut self, end: &TransferEnd) -> TransferResult {
        let id = end.transfer_id;
        let outcome = match self.active.remove(&id) {
            None => failure(TransferFailureCode::Aborted, "unknown transfer"),
            Some(Active {
                state: State::Failed(code, detail),
                ..
            }) => failure(code, &detail),
            Some(active) => self.finish(id, active),
        };
        TransferResult {
            transfer_id: id,
            outcome,
        }
    }

    fn finish(&self, id: Uuid, active: Active) -> TransferOutcome {
        let State::Receiving {
            file,
            hasher,
            received,
            ..
        } = active.state
        else {
            return failure(TransferFailureCode::Aborted, "unknown transfer");
        };
        let part = self.store.part_path(id);
        let digest: [u8; 32] = hasher.finalize().into();
        if received != active.size || digest != active.sha256 {
            drop(file);
            remove(&part);
            let detail = "the stored bytes do not match the announced size and hash";
            self.rejected(id, TransferFailureCode::ChecksumMismatch, detail);
            return failure(TransferFailureCode::ChecksumMismatch, detail);
        }
        let stored = file
            .sync_all()
            .and_then(|()| {
                drop(file);
                std::fs::rename(&part, self.store.bin_path(id))
            })
            .map_err(|err| tracing::warn!(%err, "could not store a transfer"));
        if stored.is_err() {
            remove(&part);
            let detail = "could not store the transfer";
            self.rejected(id, TransferFailureCode::Storage, detail);
            return failure(TransferFailureCode::Storage, detail);
        }
        self.store.register_staged(id, active.kind, active.size);
        TransferOutcome::Stored
    }

    /// Drops arriving transfers that went quiet or ran too long, and deletes stored
    /// transfers nobody used.
    pub fn expire(&mut self, now: Timestamp) {
        let stale: Vec<Uuid> = self
            .active
            .iter()
            .filter(|(_, active)| {
                now.duration_since(active.last_activity) >= IDLE_LIMIT
                    || now.duration_since(active.started) >= TOTAL_LIMIT
            })
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if self
                .active
                .get(&id)
                .is_some_and(|active| matches!(active.state, State::Receiving { .. }))
            {
                self.rejected(id, TransferFailureCode::Aborted, "the transfer timed out");
            }
            self.drop_transfer(id);
        }
        self.store.expire_staged(now);
    }

    /// How many transfer records are retained, for tests of the bound.
    #[cfg(test)]
    pub(crate) fn record_count(&self) -> usize {
        self.active.len()
    }

    /// Drops every arriving transfer, as when the link session ends. Stored ones stay.
    pub fn reset(&mut self) {
        let ids: Vec<Uuid> = self.active.keys().copied().collect();
        for id in ids {
            self.drop_transfer(id);
        }
    }

    fn drop_transfer(&mut self, id: Uuid) {
        if let Some(active) = self.active.remove(&id)
            && matches!(active.state, State::Receiving { .. })
        {
            drop(active);
            remove(&self.store.part_path(id));
        }
    }

    /// Serves frames from the link until `shutdown` or until the link drops its sender.
    /// Each `transfer_end` is answered once, as the reply to that frame.
    pub async fn serve(
        mut self,
        mut frames: mpsc::Receiver<IncomingTransfer>,
        link: LinkHandle,
        shutdown: CancellationToken,
    ) {
        let clock = self.clock.clone();
        let mut next = clock
            .now()
            .saturating_add(SWEEP_EVERY)
            .unwrap_or(clock.now());
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = clock.sleep_until(next) => {
                    let now = clock.now();
                    self.expire(now);
                    next = now.saturating_add(SWEEP_EVERY).unwrap_or(now);
                }
                frame = frames.recv() => {
                    let Some(IncomingTransfer { id, frame }) = frame else { break };
                    match frame {
                        TransferFrame::Begin(begin) => self.begin(&begin),
                        TransferFrame::Chunk(chunk) => self.chunk(&chunk),
                        TransferFrame::Reset => self.reset(),
                        TransferFrame::End(end) => {
                            let result = self.end(&end);
                            if !link.send(Message::TransferResult(result), Some(id)) {
                                tracing::warn!("could not queue a transfer result");
                            }
                        }
                    }
                }
            }
        }
        self.reset();
    }
}

/// Deletes a file that may already be gone.
fn remove(path: &std::path::Path) {
    if let Err(err) = std::fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%err, "could not delete a transfer file");
    }
}

/// A failed outcome with the wire's open code type.
fn failure(code: TransferFailureCode, detail: &str) -> TransferOutcome {
    let raw = match code {
        TransferFailureCode::ChecksumMismatch => "checksum_mismatch",
        TransferFailureCode::TooLarge => "too_large",
        TransferFailureCode::Storage => "storage",
        TransferFailureCode::Aborted => "aborted",
    };
    TransferOutcome::Failed {
        code: Open::new(raw, Some(code)),
        detail: detail.to_owned(),
    }
}
