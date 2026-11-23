//! Helper persistent state: key file, `state` file format and checksum, atomic write, the
//! `flock` on `state.lock`, and the explicit root actions `init`, `recover`, `rotate-key`.
//! All paths are caller supplied; the harness only ever passes temporary directories.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;

use crate::hash::{HASH_SENTINEL, parse_hex32, sha256};
use crate::keys::{fingerprint, generate_key, public_bytes};

/// Private key file name.
pub const KEY_FILE: &str = "attest.key";
/// State file name.
pub const STATE_FILE: &str = "state";
/// Lock file name.
pub const LOCK_FILE: &str = "state.lock";
/// Temporary state file name.
pub const TMP_FILE: &str = "state.tmp";

/// The persisted generation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperState {
    /// Fingerprint of the key this sequence belongs to.
    pub key_fingerprint: String,
    /// Generation.
    pub generation: u64,
    /// Last persisted config hash.
    pub config_hash: [u8; 32],
    /// Last persisted route hash.
    pub route_state_hash: [u8; 32],
    /// Boot id at the last write.
    pub boot_id: String,
}

impl HelperState {
    /// Exact file bytes: six LF lines, then `checksum=` over all preceding bytes.
    #[must_use]
    pub fn render(&self) -> Vec<u8> {
        let body = format!(
            "format=1\nkey_fingerprint={}\ngeneration={}\nconfig_hash={}\nroute_state_hash={}\nboot_id={}\n",
            self.key_fingerprint,
            self.generation,
            hex::encode(self.config_hash),
            hex::encode(self.route_state_hash),
            self.boot_id
        );
        let sum = hex::encode(sha256(body.as_bytes()));
        format!("{body}checksum={sum}\n").into_bytes()
    }

    /// Strict parse. Any deviation is corruption.
    ///
    /// # Errors
    /// A description of the first problem.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "not utf-8".to_owned())?;
        let body_end = text.rfind("checksum=").ok_or("missing checksum")?;
        let (body, tail) = text.split_at(body_end);
        let sum = tail.strip_prefix("checksum=").and_then(|t| t.strip_suffix('\n')).ok_or("bad checksum line")?;
        if parse_hex32(sum).is_none() || hex::encode(sha256(body.as_bytes())) != sum {
            return Err("checksum mismatch".into());
        }
        let keys = ["format", "key_fingerprint", "generation", "config_hash", "route_state_hash", "boot_id"];
        let lines: Vec<&str> = body.strip_suffix('\n').ok_or("missing final LF")?.split('\n').collect();
        if lines.len() != keys.len() {
            return Err("field count".into());
        }
        let mut vals = Vec::new();
        for (l, k) in lines.iter().zip(keys) {
            let v = l.strip_prefix(k).and_then(|r| r.strip_prefix('=')).ok_or_else(|| format!("expected key {k}"))?;
            vals.push(v);
        }
        if vals[0] != "1" {
            return Err("unknown format".into());
        }
        let g = vals[2];
        if g.is_empty() || !g.bytes().all(|b| b.is_ascii_digit()) || (g.len() > 1 && g.starts_with('0')) {
            return Err("generation".into());
        }
        Ok(Self {
            key_fingerprint: parse_hex32(vals[1]).map(|_| vals[1].to_owned()).ok_or("key_fingerprint")?,
            generation: g.parse().map_err(|_| "generation overflow")?,
            config_hash: parse_hex32(vals[3]).ok_or("config_hash")?,
            route_state_hash: parse_hex32(vals[4]).ok_or("route_state_hash")?,
            boot_id: vals[5].to_owned(),
        })
    }
}

/// Points of the atomic write at which a test hook may observe or block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// `state.tmp` written, not yet synced.
    TmpWritten,
    /// `state.tmp` fsynced.
    TmpSynced,
    /// Renamed over `state`.
    Renamed,
    /// Directory fsynced.
    DirSynced,
}

/// Atomic write: `state.tmp`, fsync, rename over `state`, fsync the directory. `hook` is called
/// after each step.
///
/// # Errors
/// Any I/O error; on error the previous `state` is untouched unless the failure came after the rename.
pub fn write_state_atomic(dir: &Path, st: &HelperState, hook: &mut dyn FnMut(Stage)) -> io::Result<()> {
    let tmp = dir.join(TMP_FILE);
    let mut f = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
    f.write_all(&st.render())?;
    hook(Stage::TmpWritten);
    f.sync_all()?;
    hook(Stage::TmpSynced);
    drop(f);
    fs::rename(&tmp, dir.join(STATE_FILE))?;
    hook(Stage::Renamed);
    File::open(dir)?.sync_all()?;
    hook(Stage::DirSynced);
    Ok(())
}

/// Why the helper is failing closed. `reason()` is the `incomplete_reason` token.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FailClosed {
    /// Neither key nor state.
    #[error("uninitialized")]
    Uninitialized,
    /// State present, key absent.
    #[error("key_missing")]
    KeyMissing,
    /// Key present but unreadable (not in the contract's list).
    #[error("key_corrupt")]
    KeyCorrupt,
    /// Key present, state absent.
    #[error("state_missing")]
    StateMissing,
    /// State fails checksum, parse or format.
    #[error("state_corrupt: {0}")]
    StateCorrupt(String),
    /// State belongs to another key.
    #[error("state_key_mismatch")]
    StateKeyMismatch,
}

impl FailClosed {
    /// The reason token.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::KeyMissing => "key_missing",
            Self::KeyCorrupt => "key_corrupt",
            Self::StateMissing => "state_missing",
            Self::StateCorrupt(_) => "state_corrupt",
            Self::StateKeyMismatch => "state_key_mismatch",
        }
    }
}

/// Why a root action was refused.
#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    /// `init` found a key or state.
    #[error("refusing to overwrite existing key or state")]
    Exists,
    /// `recover`/`rotate-key` found nothing to act on.
    #[error("nothing to retire; use init")]
    NothingToRetire,
    /// I/O.
    #[error("io: {0}")]
    Io(#[from] io::Error),
}

/// A locked view of the helper directory. Dropping it releases the `flock`.
pub struct Session {
    dir: PathBuf,
    _lock: File,
    /// The signing key, or why there is none.
    pub key: Result<SigningKey, FailClosed>,
    /// The state, or why it cannot be used.
    pub state: Result<HelperState, FailClosed>,
}

impl Session {
    /// The directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// First failure, key before state.
    #[must_use]
    pub fn failure(&self) -> Option<FailClosed> {
        self.key.as_ref().err().or(self.state.as_ref().err()).cloned()
    }

    /// Persist a new state through the atomic write.
    ///
    /// # Errors
    /// I/O.
    pub fn write(&self, st: &HelperState, hook: &mut dyn FnMut(Stage)) -> io::Result<()> {
        write_state_atomic(&self.dir, st, hook)
    }
}

/// The helper directory (a temporary directory in every harness test).
#[derive(Debug, Clone)]
pub struct HelperDir(pub PathBuf);

fn read_key(p: &Path) -> Result<SigningKey, FailClosed> {
    let b = fs::read(p).map_err(|_| FailClosed::KeyCorrupt)?;
    let seed: [u8; 32] = b.try_into().map_err(|_| FailClosed::KeyCorrupt)?;
    Ok(SigningKey::from_bytes(&seed))
}

fn write_new_private(p: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(p)?;
    f.write_all(bytes)?;
    f.sync_all()
}

impl HelperDir {
    fn ensure_dir(&self) -> io::Result<()> {
        if !self.0.exists() {
            fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.0)?;
        }
        Ok(())
    }

    fn lock_file(&self) -> io::Result<File> {
        OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600).open(self.0.join(LOCK_FILE))
    }

    /// Take `flock(LOCK_EX)` on `state.lock`, delete a leftover `state.tmp`, then read and
    /// validate key and state. The lock is held until the session drops.
    ///
    /// # Errors
    /// I/O while locking.
    pub fn begin(&self) -> io::Result<Session> {
        if !self.0.exists() {
            // Nothing exists yet: report uninitialized without creating anything.
            let lock = tempfile::tempfile()?;
            return Ok(Session {
                dir: self.0.clone(),
                _lock: lock,
                key: Err(FailClosed::Uninitialized),
                state: Err(FailClosed::Uninitialized),
            });
        }
        let lock = self.lock_file()?;
        lock.lock()?;
        let _ = fs::remove_file(self.0.join(TMP_FILE));
        Ok(self.load(lock))
    }

    fn load(&self, lock: File) -> Session {
        let kp = self.0.join(KEY_FILE);
        let sp = self.0.join(STATE_FILE);
        let (kx, sx) = (kp.exists(), sp.exists());
        let key = match (kx, sx) {
            (false, false) => Err(FailClosed::Uninitialized),
            (false, true) => Err(FailClosed::KeyMissing),
            (true, _) => read_key(&kp),
        };
        let state = if !sx {
            Err(if kx { FailClosed::StateMissing } else { FailClosed::Uninitialized })
        } else {
            match fs::read(&sp).map_err(|e| e.to_string()).and_then(|b| HelperState::parse(&b)) {
                Err(e) => Err(FailClosed::StateCorrupt(e)),
                Ok(st) => match &key {
                    Ok(k) if fingerprint(&public_bytes(k)) != st.key_fingerprint => Err(FailClosed::StateKeyMismatch),
                    _ => Ok(st),
                },
            }
        };
        Session { dir: self.0.clone(), _lock: lock, key, state }
    }

    /// `domovoi helper fingerprint`: derive the public key and fingerprint from the key file.
    ///
    /// # Errors
    /// [`FailClosed`] when the key is absent or unreadable.
    pub fn fingerprint(&self) -> Result<String, FailClosed> {
        read_key(&self.0.join(KEY_FILE)).map(|k| fingerprint(&public_bytes(&k)))
    }

    fn fresh_state(key: &SigningKey, hashes: Option<([u8; 32], [u8; 32])>, boot_id: &str) -> HelperState {
        let (config_hash, route_state_hash) = hashes.unwrap_or((HASH_SENTINEL, HASH_SENTINEL));
        HelperState {
            key_fingerprint: fingerprint(&public_bytes(key)),
            generation: 1,
            config_hash,
            route_state_hash,
            boot_id: boot_id.to_owned(),
        }
    }

    /// `domovoi helper init`: the only first-time path. Creates key and `generation=1` state only
    /// when neither exists. `hashes` are the current config and route hashes if the caller could
    /// compute them, otherwise the sentinel is written (see the report: effect on the first
    /// generation).
    ///
    /// # Errors
    /// [`ActionError::Exists`] when either file exists.
    pub fn init(&self, hashes: Option<([u8; 32], [u8; 32])>, boot_id: &str) -> Result<[u8; 32], ActionError> {
        self.ensure_dir()?;
        let lock = self.lock_file()?;
        lock.lock()?;
        if self.0.join(KEY_FILE).exists() || self.0.join(STATE_FILE).exists() {
            return Err(ActionError::Exists);
        }
        let key = generate_key();
        write_new_private(&self.0.join(KEY_FILE), &key.to_bytes())?;
        write_state_atomic(&self.0, &Self::fresh_state(&key, hashes, boot_id), &mut |_| {})?;
        Ok(public_bytes(&key))
    }

    fn replace(&self, ts: &str, hashes: Option<([u8; 32], [u8; 32])>, boot_id: &str, need_both: bool) -> Result<[u8; 32], ActionError> {
        self.ensure_dir()?;
        let lock = self.lock_file()?;
        lock.lock()?;
        let (kp, sp) = (self.0.join(KEY_FILE), self.0.join(STATE_FILE));
        let (kx, sx) = (kp.exists(), sp.exists());
        if (need_both && !(kx && sx)) || (!kx && !sx) {
            return Err(ActionError::NothingToRetire);
        }
        let retired = self.0.join("retired").join(ts);
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&retired)?;
        for (p, n) in [(&kp, KEY_FILE), (&sp, STATE_FILE)] {
            if p.exists() {
                fs::rename(p, retired.join(n))?;
            }
        }
        let _ = fs::remove_file(self.0.join(TMP_FILE));
        let key = generate_key();
        write_new_private(&kp, &key.to_bytes())?;
        write_state_atomic(&self.0, &Self::fresh_state(&key, hashes, boot_id), &mut |_| {})?;
        Ok(public_bytes(&key))
    }

    /// `domovoi helper recover`: retire whatever exists, new key, new state at `generation=1`.
    ///
    /// # Errors
    /// [`ActionError::NothingToRetire`] when neither file exists.
    pub fn recover(&self, ts: &str, hashes: Option<([u8; 32], [u8; 32])>, boot_id: &str) -> Result<[u8; 32], ActionError> {
        self.replace(ts, hashes, boot_id, false)
    }

    /// `domovoi helper rotate-key`: new key, new state at `generation=1`; the old counter is
    /// not carried over. Requires both files to exist (a harness reading).
    ///
    /// # Errors
    /// [`ActionError::NothingToRetire`] when a file is missing.
    pub fn rotate_key(&self, ts: &str, hashes: Option<([u8; 32], [u8; 32])>, boot_id: &str) -> Result<[u8; 32], ActionError> {
        self.replace(ts, hashes, boot_id, true)
    }
}
