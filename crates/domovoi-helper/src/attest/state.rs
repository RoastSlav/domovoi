//! The helper's persisted attestation state: one small file with a checksum.
//!
//! ```text
//! format=1
//! key_fingerprint=<64 hex>
//! generation=<u64 decimal>
//! config_hash=<64 hex>
//! route_state_hash=<64 hex>
//! boot_id=<8-4-4-4-12 hex>
//! checksum=<64 hex SHA-256 of every preceding byte, LF included>
//! ```
//!
//! The parser is strict: each line is LF-terminated, the keys come in this order, nothing
//! follows the checksum, and every value is canonical. Anything else is corruption, and a corrupt
//! state is never repaired by guessing.

use domovoi_core::attest::{BootId, Hash32, ZERO_HASH, parse_uint};
use domovoi_core::host::KeyFingerprint;

/// The state format number this build writes and the only one it reads.
pub const STATE_FORMAT: u32 = 1;

/// Why a state file is not usable. Both are `state_corrupt` on the wire: the bytes cannot be
/// trusted, whatever the cause.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateFault {
    /// The text is not the strict layout, or a value is not canonical.
    #[error("the state file is malformed: {0}")]
    Malformed(&'static str),
    /// The checksum does not match the bytes before it.
    #[error("the state file's checksum does not match")]
    Checksum,
}

/// What the helper last committed: the key it belongs to, the generation and the hashes that
/// generation was made from, and the boot it was last attested in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFile {
    /// The fingerprint of the key the sequence belongs to.
    pub key_fingerprint: KeyFingerprint,
    /// The last generation committed under that key; 0 until the first attestation.
    pub generation: u64,
    /// The configuration hash of that generation; all zero at generation 0.
    pub config_hash: Hash32,
    /// The route-state hash of that generation; all zero at generation 0.
    pub route_state_hash: Hash32,
    /// The boot the state was last written in.
    pub boot_id: BootId,
}

impl StateFile {
    /// The state `init`, `rotate-key` and `recover` write: generation 0 and zero hashes.
    #[must_use]
    pub fn fresh(key_fingerprint: KeyFingerprint, boot_id: BootId) -> Self {
        Self {
            key_fingerprint,
            generation: 0,
            config_hash: ZERO_HASH,
            route_state_hash: ZERO_HASH,
            boot_id,
        }
    }

    /// The exact bytes of the file, checksum included.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        let body = format!(
            "format={STATE_FORMAT}\nkey_fingerprint={}\ngeneration={}\nconfig_hash={}\nroute_state_hash={}\nboot_id={}\n",
            self.key_fingerprint,
            self.generation,
            self.config_hash,
            self.route_state_hash,
            self.boot_id
        );
        let checksum = Hash32::of(body.as_bytes());
        format!("{body}checksum={checksum}\n").into_bytes()
    }

    /// Parses the file strictly.
    ///
    /// # Errors
    ///
    /// [`StateFault`] for any deviation from the layout, a non-canonical value or a wrong
    /// checksum.
    pub fn parse(bytes: &[u8]) -> Result<Self, StateFault> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| StateFault::Malformed("not valid UTF-8"))?;
        if !text.ends_with('\n') {
            return Err(StateFault::Malformed("missing final LF"));
        }
        let mut offset = 0;
        let mut values: Vec<&str> = Vec::with_capacity(7);
        let mut preceding = 0;
        for key in [
            "format",
            "key_fingerprint",
            "generation",
            "config_hash",
            "route_state_hash",
            "boot_id",
            "checksum",
        ] {
            let rest = &text[offset..];
            let line_end = rest
                .find('\n')
                .ok_or(StateFault::Malformed("a line is missing"))?;
            let line = &rest[..line_end];
            let value = line
                .strip_prefix(key)
                .and_then(|r| r.strip_prefix('='))
                .ok_or(StateFault::Malformed("unexpected key or order"))?;
            if key == "checksum" {
                preceding = offset;
            }
            values.push(value);
            offset += line_end + 1;
        }
        if offset != text.len() {
            return Err(StateFault::Malformed("data after the checksum"));
        }
        let hash = |text: &str| {
            text.parse::<Hash32>()
                .map_err(|_| StateFault::Malformed("a hash is not 64 lowercase hex"))
        };
        if values[0] != "1" {
            return Err(StateFault::Malformed("unsupported format"));
        }
        let fingerprint = hash(values[1])?;
        let generation = parse_uint(values[2])
            .map_err(|_| StateFault::Malformed("generation is not canonical"))?;
        let state = Self {
            key_fingerprint: fingerprint.into(),
            generation,
            config_hash: hash(values[3])?,
            route_state_hash: hash(values[4])?,
            boot_id: BootId::parse(values[5])
                .map_err(|_| StateFault::Malformed("boot_id is not canonical"))?,
        };
        if hash(values[6])? != Hash32::of(&bytes[..preceding]) {
            return Err(StateFault::Checksum);
        }
        Ok(state)
    }
}

/// How the state file is unusable when it is read from disk.
#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateReadFault {
    /// No state file exists.
    Missing,
    /// The file exists and cannot be read or fails the strict parse.
    Corrupt(StateFault),
}

/// Reads and parses `state` in `dir`, without following a symlink.
///
/// # Errors
///
/// [`StateReadFault::Missing`] when there is no file and [`StateReadFault::Corrupt`] otherwise.
#[cfg(unix)]
pub fn load_state(dir: &std::path::Path) -> Result<StateFile, StateReadFault> {
    use std::io::Read as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    /// A state file is a few hundred bytes; a larger one is not a state file.
    const MAX_STATE_BYTES: u64 = 4096;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(dir.join(super::STATE_FILE));
    let file = match file {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(StateReadFault::Missing);
        }
        Err(_) => return Err(StateReadFault::Corrupt(StateFault::Malformed("unreadable"))),
    };
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StateReadFault::Corrupt(StateFault::Malformed("unreadable")))?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(StateReadFault::Corrupt(StateFault::Malformed("too large")));
    }
    StateFile::parse(&bytes).map_err(StateReadFault::Corrupt)
}
