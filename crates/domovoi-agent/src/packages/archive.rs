//! Reading a `.tar.zst` package safely.
//!
//! One walker, [`walk`], serves both passes. The audit pass ([`audit`]) reads every entry,
//! enforces every limit and creates nothing. The extraction pass calls [`walk`] again with
//! a visitor that writes files. Both run each entry through [`check_entry`], so what the
//! audit accepted is exactly what extraction will accept.
//!
//! The decode pipeline is `file -> BufReader -> HashingReader -> zstd decoder ->
//! LimitedReader -> tar`. The decoder refuses any frame whose window exceeds 128 MiB
//! (set explicitly on the decoder), the limited reader stops a decompression bomb at a known
//! cap, and the hash covers every byte of the file including anything after the frame.
//! Entries are never unpacked by the `tar` crate: the visitor receives the bytes and writes
//! them itself, so modes, owners, links and extended attributes in the archive are ignored.

use std::cell::RefCell;
use std::io::{BufReader, Read};
use std::path::Path;

use domovoi_config::PackageLimits;
use domovoi_core::package::{
    ArchiveDigest, MAX_PACKAGE_MANIFEST_BYTES, PackageFailureCode, PackageName,
};
use domovoi_core::ports::Clock;
use jiff::Timestamp;
use ruzstd::decoding::{FrameDecoder, StreamingDecoder};
use sha2::{Digest as _, Sha256};

pub use super::entry::{EntryInfo, EntryKind, SeenPaths, check_entry};
use super::error::PackageError;

/// Bytes a tar header and its padding may add to each entry, on top of the content.
const TAR_OVERHEAD_PER_ENTRY: u64 = 1536;
/// Slack for the end-of-archive blocks.
const TAR_END_SLACK: u64 = 4096;
/// Largest zstd window the decoder accepts, in bytes.
const MAX_WINDOW_BYTES: u64 = 128 * 1024 * 1024;
/// Largest read from an entry between two clock checks.
const READ_STEP: usize = 64 * 1024;

fn fail(code: PackageFailureCode, detail: impl Into<String>) -> PackageError {
    PackageError::new(code, detail)
}

/// The clock limit for one pass.
struct Deadline<'a> {
    clock: &'a dyn Clock,
    at: Timestamp,
}

impl<'a> Deadline<'a> {
    fn new(clock: &'a dyn Clock, limits: &PackageLimits) -> Result<Self, PackageError> {
        let at = clock
            .now()
            .checked_add(limits.extraction_time.as_signed())
            .map_err(|_| {
                fail(
                    PackageFailureCode::Storage,
                    "the extraction deadline overflows",
                )
            })?;
        Ok(Self { clock, at })
    }

    fn check(&self) -> Result<(), PackageError> {
        if self.clock.now() >= self.at {
            return Err(fail(
                PackageFailureCode::LimitTime,
                "unpacking took longer than the time limit",
            ));
        }
        Ok(())
    }
}

/// Where a reader leaves the reason it failed, because `tar` hides the original error.
type Trip = RefCell<Option<PackageError>>;

fn tripped(trip: &Trip, error: PackageError) -> std::io::Error {
    *trip.borrow_mut() = Some(error);
    std::io::Error::other("package limit reached")
}

/// Hashes every byte it passes on.
struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if let Some(bytes) = buf.get(..read) {
            self.hasher.update(bytes);
        }
        Ok(read)
    }
}

/// Fails once more than `remaining` bytes have passed: the decompression-bomb stop.
struct LimitedReader<'a, R> {
    inner: R,
    remaining: u64,
    trip: &'a Trip,
}

impl<R: Read> Read for LimitedReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let room = usize::try_from(self.remaining.saturating_add(1)).unwrap_or(usize::MAX);
        let want = buf.len().min(room);
        let Some(window) = buf.get_mut(..want) else {
            return Ok(0);
        };
        let read = self.inner.read(window)?;
        let read_len = u64::try_from(read).unwrap_or(u64::MAX);
        if read_len > self.remaining {
            return Err(tripped(
                self.trip,
                fail(
                    PackageFailureCode::LimitUncompressed,
                    "the archive expands past its size limit",
                ),
            ));
        }
        self.remaining -= read_len;
        Ok(read)
    }
}

/// Checks the deadline before every read of at most 64 KiB.
struct EntryReader<'a, R> {
    inner: R,
    deadline: &'a Deadline<'a>,
    trip: &'a Trip,
}

impl<R: Read> Read for EntryReader<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Err(error) = self.deadline.check() {
            return Err(tripped(self.trip, error));
        }
        let want = buf.len().min(READ_STEP);
        match buf.get_mut(..want) {
            Some(window) => self.inner.read(window),
            None => Ok(0),
        }
    }
}

/// What a finished walk saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkSummary {
    /// Entries in the archive.
    pub entries: usize,
    /// Total declared bytes of regular files.
    pub uncompressed: u64,
}

/// Called for each accepted entry with a reader over its content (empty for directories).
/// The walker drains whatever the visitor leaves unread.
pub type Visitor<'a> = dyn FnMut(&EntryInfo, &mut dyn Read) -> Result<(), PackageError> + 'a;

fn damaged(trip: &Trip, error: &std::io::Error) -> PackageError {
    trip.borrow_mut().take().unwrap_or_else(|| {
        fail(
            PackageFailureCode::ArchiveInvalid,
            format!("the archive is damaged: {}", error.kind()),
        )
    })
}

/// Reads `reader` as a package archive for `name`, calling `visit` for every entry.
///
/// Fails with `digest_mismatch` unless every byte of the file hashes to `expected`.
///
/// # Errors
/// Any archive, limit or digest code, or the visitor's own error.
pub fn walk(
    reader: impl Read,
    name: &PackageName,
    limits: &PackageLimits,
    clock: &dyn Clock,
    expected: &ArchiveDigest,
    visit: &mut Visitor<'_>,
) -> Result<WalkSummary, PackageError> {
    let deadline = Deadline::new(clock, limits)?;
    let trip: Trip = RefCell::new(None);
    let mut hashing = HashingReader {
        inner: BufReader::new(reader),
        hasher: Sha256::new(),
    };
    let summary = decode_and_visit(&mut hashing, name, limits, &deadline, &trip, visit)?;
    let mut rest = [0_u8; 4096];
    let read = hashing
        .read(&mut rest)
        .map_err(|error| PackageError::io("reading the archive", &error))?;
    let extra = read != 0;
    if extra {
        return Err(fail(
            PackageFailureCode::ArchiveInvalid,
            "data follows the compressed archive",
        ));
    }
    let actual = ArchiveDigest::from_bytes(hashing.hasher.finalize().into());
    if &actual != expected {
        return Err(fail(
            PackageFailureCode::DigestMismatch,
            "the archive does not match the signed digest",
        ));
    }
    Ok(summary)
}

fn decode_and_visit<R: Read>(
    hashing: &mut HashingReader<R>,
    name: &PackageName,
    limits: &PackageLimits,
    deadline: &Deadline<'_>,
    trip: &Trip,
    visit: &mut Visitor<'_>,
) -> Result<WalkSummary, PackageError> {
    let mut frame = FrameDecoder::new();
    frame.set_max_window_size(MAX_WINDOW_BYTES);
    let decoder = StreamingDecoder::new_with_decoder(&mut *hashing, frame).map_err(|error| {
        fail(
            PackageFailureCode::ArchiveInvalid,
            format!("not a zstd archive: {error}"),
        )
    })?;
    let cap = limits
        .uncompressed_bytes
        .saturating_add(u64::from(limits.entries).saturating_mul(TAR_OVERHEAD_PER_ENTRY))
        .saturating_add(TAR_END_SLACK);
    let limited = LimitedReader {
        inner: decoder,
        remaining: cap,
        trip,
    };
    let mut archive = tar::Archive::new(limited);
    let summary = visit_entries(&mut archive, name, limits, deadline, trip, visit)?;
    let mut tail = archive.into_inner();
    let mut block = [0_u8; 8192];
    loop {
        let read = tail
            .read(&mut block)
            .map_err(|error| damaged(trip, &error))?;
        let Some(bytes) = block.get(..read).filter(|bytes| !bytes.is_empty()) else {
            break;
        };
        if bytes.iter().any(|byte| *byte != 0) {
            return Err(fail(
                PackageFailureCode::ArchiveInvalid,
                "data follows the end of the archive",
            ));
        }
    }
    Ok(summary)
}

fn visit_entries<D: Read>(
    archive: &mut tar::Archive<LimitedReader<'_, D>>,
    name: &PackageName,
    limits: &PackageLimits,
    deadline: &Deadline<'_>,
    trip: &Trip,
    visit: &mut Visitor<'_>,
) -> Result<WalkSummary, PackageError> {
    let mut seen = SeenPaths::default();
    let mut summary = WalkSummary {
        entries: 0,
        uncompressed: 0,
    };
    let entries = archive.entries().map_err(|error| damaged(trip, &error))?;
    for entry in entries {
        let mut entry = entry.map_err(|error| damaged(trip, &error))?;
        summary.entries += 1;
        if summary.entries > usize::try_from(limits.entries).unwrap_or(usize::MAX) {
            return Err(fail(
                PackageFailureCode::LimitEntries,
                format!("the archive has more than {} entries", limits.entries),
            ));
        }
        deadline.check()?;
        let kind = entry.header().entry_type();
        let mode = entry.header().mode().unwrap_or(0);
        let size = entry.size();
        let raw = entry.path_bytes().into_owned();
        let info = check_entry(name, limits, kind, &raw, size, mode, &mut seen)?;
        if let EntryKind::File { size, .. } = info.kind {
            summary.uncompressed = summary.uncompressed.saturating_add(size);
            if summary.uncompressed > limits.uncompressed_bytes {
                return Err(fail(
                    PackageFailureCode::LimitUncompressed,
                    format!(
                        "the files add up to more than {} bytes",
                        limits.uncompressed_bytes
                    ),
                ));
            }
        }
        let mut content = EntryReader {
            inner: &mut entry,
            deadline,
            trip,
        };
        let visited = match info.kind {
            EntryKind::Dir => visit(&info, &mut std::io::empty()),
            EntryKind::File { .. } => visit(&info, &mut content),
        };
        if let Err(error) = visited {
            return Err(trip.borrow_mut().take().unwrap_or(error));
        }
        std::io::copy(&mut content, &mut std::io::sink()).map_err(|error| damaged(trip, &error))?;
    }
    Ok(summary)
}

/// What the audit pass found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Audit {
    /// The bytes of `<name>/package.toml`.
    pub manifest_bytes: Vec<u8>,
    /// Entries in the archive.
    pub entries: usize,
    /// Total declared bytes of regular files.
    pub uncompressed: u64,
}

/// The audit pass over `reader`: every entry checked and read, nothing written.
///
/// # Errors
/// Any archive, limit or digest code; `manifest_invalid` when `package.toml` is missing or
/// larger than 64 KiB.
pub fn audit(
    reader: impl Read,
    name: &PackageName,
    limits: &PackageLimits,
    clock: &dyn Clock,
    expected: &ArchiveDigest,
) -> Result<Audit, PackageError> {
    let mut manifest: Option<Vec<u8>> = None;
    let limit = u64::try_from(MAX_PACKAGE_MANIFEST_BYTES).unwrap_or(u64::MAX);
    let summary = walk(
        reader,
        name,
        limits,
        clock,
        expected,
        &mut |info, content| {
            if info.rel == "package.toml" && matches!(info.kind, EntryKind::File { .. }) {
                let mut bytes = Vec::new();
                Read::take(&mut *content, limit.saturating_add(1)).read_to_end(&mut bytes)?;
                if bytes.len() > MAX_PACKAGE_MANIFEST_BYTES {
                    return Err(fail(
                        PackageFailureCode::ManifestInvalid,
                        "package.toml is too large",
                    ));
                }
                manifest = Some(bytes);
            }
            Ok(())
        },
    )?;
    let manifest_bytes = manifest.ok_or_else(|| {
        fail(
            PackageFailureCode::ManifestInvalid,
            "the archive has no package.toml",
        )
    })?;
    Ok(Audit {
        manifest_bytes,
        entries: summary.entries,
        uncompressed: summary.uncompressed,
    })
}

/// [`audit`] over the file at `path`; a file larger than `compressed_bytes` is `too_large`.
///
/// # Errors
/// As [`audit`], plus `too_large` and `storage`.
pub fn audit_file(
    path: &Path,
    name: &PackageName,
    limits: &PackageLimits,
    clock: &dyn Clock,
    expected: &ArchiveDigest,
) -> Result<Audit, PackageError> {
    let file = std::fs::File::open(path)
        .map_err(|error| PackageError::io("opening the archive", &error))?;
    let length = file
        .metadata()
        .map_err(|error| PackageError::io("reading the archive size", &error))?
        .len();
    if length > limits.compressed_bytes {
        return Err(fail(
            PackageFailureCode::TooLarge,
            format!(
                "the archive is larger than {} bytes",
                limits.compressed_bytes
            ),
        ));
    }
    audit(file, name, limits, clock, expected)
}
