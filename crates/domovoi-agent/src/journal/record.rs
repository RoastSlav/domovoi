//! Record framing and the five lifecycle records.

use domovoi_proto::messages::{AuthorizationKind, CapabilityResult};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::error::JournalError;

/// Most bytes one record payload may carry: 1 MiB.
pub const MAX_PAYLOAD: usize = 1 << 20;

/// One durable journal record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Record {
    /// The first record of every epoch, linking the epoch to the record file.
    JournalCreated {
        /// The record schema version.
        format: u32,
        /// The new epoch.
        epoch: Uuid,
        /// Wall time at creation.
        wall_time: Timestamp,
        /// The boot id at creation.
        boot_id: String,
        /// Monotonic time at creation.
        mono_ns: u64,
    },
    /// An execution the agent accepted. Durable before the reply and the spawn.
    Accepted {
        /// The record schema version.
        format: u32,
        /// The execution.
        execution_id: Uuid,
        /// Contract id.
        contract: String,
        /// Contract version.
        version: u32,
        /// Canonical target.
        target: String,
        /// The plan hash the approval bound to, as lowercase hex.
        plan_hash: String,
        /// Whether a human or a policy authorized it.
        authorization_kind: AuthorizationKind,
        /// Wall time at acceptance.
        wall_time: Timestamp,
        /// The boot id at acceptance.
        boot_id: String,
        /// Monotonic time at acceptance.
        mono_ns: u64,
        /// Whether the contract mutates the host.
        mutating: bool,
    },
    /// The child started. Durable immediately after spawn.
    Started {
        /// The record schema version.
        format: u32,
        /// The execution.
        execution_id: Uuid,
        /// The child's pid.
        pid: u32,
        /// `/proc/<pid>/stat` field 22, in clock ticks since boot.
        proc_start_time: u64,
    },
    /// The child completed. Durable before the reply.
    Finished {
        /// The record schema version.
        format: u32,
        /// The execution.
        execution_id: Uuid,
        /// The durable result.
        result: CapabilityResult,
    },
    /// A restart found the recorded child alive and killed its process group.
    Abandoned {
        /// The record schema version.
        format: u32,
        /// The execution.
        execution_id: Uuid,
        /// The pid that was killed.
        pid: u32,
        /// Why it was abandoned.
        reason: String,
    },
}

impl Record {
    /// The record schema version this build writes.
    pub const FORMAT: u32 = 1;

    /// The record's format field.
    #[must_use]
    pub fn format(&self) -> u32 {
        match self {
            Self::JournalCreated { format, .. }
            | Self::Accepted { format, .. }
            | Self::Started { format, .. }
            | Self::Finished { format, .. }
            | Self::Abandoned { format, .. } => *format,
        }
    }

    /// The execution this record is about, if any.
    #[must_use]
    pub fn execution_id(&self) -> Option<Uuid> {
        match self {
            Self::JournalCreated { .. } => None,
            Self::Accepted { execution_id, .. }
            | Self::Started { execution_id, .. }
            | Self::Finished { execution_id, .. }
            | Self::Abandoned { execution_id, .. } => Some(*execution_id),
        }
    }
}

/// A journal file's complete records and the byte length they occupy.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRecords {
    /// The complete records, oldest first.
    pub records: Vec<Record>,
    /// The length of the complete frames; anything after it was a torn tail.
    pub valid_len: u64,
}

/// Frames one record as `u32_be(len) || sha256(payload)[32] || payload`.
pub fn frame(record: &Record) -> Result<Vec<u8>, JournalError> {
    let payload = serde_json::to_vec(record)
        .map_err(|err| JournalError::Corrupt(format!("could not encode a record: {err}")))?;
    if payload.len() > MAX_PAYLOAD {
        return Err(JournalError::Corrupt(
            "a record is larger than 1 MiB".to_owned(),
        ));
    }
    let len = u32::try_from(payload.len())
        .map_err(|_| JournalError::Corrupt("a record is larger than 4 GiB".to_owned()))?;
    let mut out = Vec::with_capacity(4 + 32 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&Sha256::digest(&payload));
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Parses a journal file, discarding only a physically incomplete final frame.
///
/// A complete frame with a bad checksum, invalid JSON, an unknown kind or an unsupported
/// format is corruption, never a torn tail.
pub fn parse(bytes: &[u8], metadata_epoch: Uuid) -> Result<ParsedRecords, JournalError> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let remaining = bytes.len() - offset;
        if remaining < 4 {
            break;
        }
        let len = u32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .map_err(|_| JournalError::Corrupt("bad frame length".to_owned()))?,
        ) as usize;
        if len > MAX_PAYLOAD {
            return Err(JournalError::Corrupt(format!(
                "a frame declares {len} bytes, more than 1 MiB"
            )));
        }
        if remaining < 4 + 32 {
            break;
        }
        if remaining < 4 + 32 + len {
            break;
        }
        let checksum = &bytes[offset + 4..offset + 4 + 32];
        let payload = &bytes[offset + 4 + 32..offset + 4 + 32 + len];
        if Sha256::digest(payload).as_slice() != checksum {
            return Err(JournalError::Corrupt(
                "a complete record has a bad checksum".to_owned(),
            ));
        }
        let record: Record = serde_json::from_slice(payload)
            .map_err(|err| JournalError::Corrupt(format!("a record is not valid JSON: {err}")))?;
        if record.format() != Record::FORMAT {
            return Err(JournalError::Corrupt(format!(
                "a record uses unsupported format {}",
                record.format()
            )));
        }
        records.push(record);
        offset += 4 + 32 + len;
    }
    validate(&records, metadata_epoch)?;
    Ok(ParsedRecords {
        records,
        valid_len: offset as u64,
    })
}

fn validate(records: &[Record], metadata_epoch: Uuid) -> Result<(), JournalError> {
    let Some(Record::JournalCreated { epoch, .. }) = records.first() else {
        return Err(JournalError::Corrupt(
            "the journal does not start with journal_created".to_owned(),
        ));
    };
    if *epoch != metadata_epoch {
        return Err(JournalError::Corrupt(
            "journal_created names an epoch other than the metadata's".to_owned(),
        ));
    }
    if records
        .iter()
        .filter(|r| matches!(r, Record::JournalCreated { .. }))
        .count()
        != 1
    {
        return Err(JournalError::Corrupt(
            "the journal has more than one journal_created record".to_owned(),
        ));
    }
    Ok(())
}
