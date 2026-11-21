//! The pure half of a file transfer: the frames, the checks on the bytes that go into them
//! and the deadline. Nothing here talks to an agent.

use domovoi_core::install::MAX_CHUNK_BYTES;
use domovoi_core::package::ArchiveDigest;
use domovoi_core::{TransferFrame, TransferKind};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The slowest transfer rate the deadline allows for, in bytes per second.
const SLOWEST_BYTES_PER_SEC: u64 = 256 * 1024;

/// The longest any transfer may take.
const LONGEST: SignedDuration = SignedDuration::from_mins(15);

/// The time a transfer of `size` bytes may take from `start`: 60 seconds plus the size at
/// 256 KiB per second, at most 15 minutes.
pub(super) fn transfer_deadline(start: Timestamp, size: u64) -> Timestamp {
    let secs = 60 + size / SLOWEST_BYTES_PER_SEC;
    let allowed = SignedDuration::from_secs(i64::try_from(secs).unwrap_or(i64::MAX)).min(LONGEST);
    start.checked_add(allowed).unwrap_or(start)
}

/// The `Begin` frame of a transfer.
pub(super) fn begin(
    transfer: Uuid,
    kind: TransferKind,
    size: u64,
    sha256: [u8; 32],
) -> TransferFrame {
    TransferFrame::Begin {
        transfer,
        kind,
        size,
        sha256,
    }
}

/// Why the bytes of a transfer are not the ones that were announced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(super) enum Mismatch {
    /// A chunk was empty, larger than a frame allows, or followed a short chunk.
    #[error("the archive was read in chunks the transfer cannot carry")]
    ChunkShape,
    /// More bytes came than the index says the archive has.
    #[error("the archive on disk is larger than its index says")]
    TooLong,
    /// Fewer bytes came than the index says.
    #[error("the archive on disk is shorter than its index says")]
    TooShort,
    /// The bytes do not hash to the announced digest.
    #[error("the archive on disk does not match its recorded digest")]
    Digest,
}

/// Numbers the chunks of one transfer and hashes the bytes that go into them, so the totals
/// can be checked against what `Begin` announced before `End` is sent.
pub(super) struct Chunker {
    transfer: Uuid,
    expected_size: u64,
    seen: u64,
    seq: u64,
    short_seen: bool,
    hasher: Sha256,
}

impl Chunker {
    /// A chunker for the transfer `transfer` that announced `expected_size` bytes.
    pub(super) fn new(transfer: Uuid, expected_size: u64) -> Self {
        Self {
            transfer,
            expected_size,
            seen: 0,
            seq: 0,
            short_seen: false,
            hasher: Sha256::new(),
        }
    }

    /// Takes the next chunk of the file and returns its frame. Only the last chunk may be
    /// shorter than a full frame.
    pub(super) fn push(&mut self, bytes: Vec<u8>) -> Result<TransferFrame, Mismatch> {
        if bytes.is_empty() || bytes.len() > MAX_CHUNK_BYTES || self.short_seen {
            return Err(Mismatch::ChunkShape);
        }
        self.short_seen = bytes.len() < MAX_CHUNK_BYTES;
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        self.seen = self.seen.saturating_add(len);
        if self.seen > self.expected_size {
            return Err(Mismatch::TooLong);
        }
        self.hasher.update(&bytes);
        let seq = self.seq;
        self.seq += 1;
        Ok(TransferFrame::Chunk {
            transfer: self.transfer,
            seq,
            bytes,
        })
    }

    /// Checks the totals against the announced size and digest. Call before `End`.
    pub(super) fn finish(self, expected: &ArchiveDigest) -> Result<(), Mismatch> {
        if self.seen < self.expected_size {
            return Err(Mismatch::TooShort);
        }
        let actual = ArchiveDigest::from_bytes(self.hasher.finalize().into());
        if &actual == expected {
            Ok(())
        } else {
            Err(Mismatch::Digest)
        }
    }
}

/// The SHA-256 of `bytes`, as a transfer announces it.
pub(super) fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Cuts `bytes` into frame-sized pieces.
pub(super) fn pieces(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes.chunks(MAX_CHUNK_BYTES).map(<[u8]>::to_vec).collect()
}
