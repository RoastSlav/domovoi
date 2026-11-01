//! Encryption at rest for secrets entered in the web UI (ADR 0017).
//!
//! ChaCha20-Poly1305 with a 32-byte key in `<data_dir>/secret.key`, which is never stored
//! in the database. Each ciphertext is a random 96-bit nonce followed by the sealed value,
//! and the secret's config path is the associated data, so a ciphertext copied to another
//! field does not open.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::{ExposeSecret, SecretString};

/// The key file's name inside the data directory.
pub const KEY_FILE: &str = "secret.key";

const KEY_LEN: usize = 32;

/// Why the key could not be loaded or a secret could not be sealed or opened.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// The key file could not be created or read.
    #[error("cannot use {path}: {source}")]
    Io {
        /// The key file.
        path: PathBuf,
        /// The operating-system error.
        source: std::io::Error,
    },
    /// Others can read the key file.
    #[error(
        "{path} is readable by group or others (mode {mode:o}); run `chmod 600 {path}` and start again"
    )]
    LoosePermissions {
        /// The key file.
        path: String,
        /// Its permission bits.
        mode: u32,
    },
    /// The key file is not 32 bytes.
    #[error("{0} is not a valid key; restore it from your backup or remove it to start over")]
    BadKey(String),
    /// The system random generator failed.
    #[error("no randomness available")]
    Random,
    /// The ciphertext is damaged, belongs to another field, or was sealed with another key.
    #[error("the stored secret cannot be decrypted")]
    Unreadable,
}

/// The data directory's secret key.
pub struct SecretKey {
    key: LessSafeKey,
    random: SystemRandom,
}

impl std::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretKey(..)")
    }
}

impl SecretKey {
    /// Loads `<data_dir>/secret.key`, creating it (mode 0600) with 32 random bytes on
    /// first start. Refuses a key file that group or others can read.
    pub fn load_or_create(data_dir: &Path) -> Result<Self, CryptoError> {
        let path = data_dir.join(KEY_FILE);
        let io = |source| CryptoError::Io {
            path: path.clone(),
            source,
        };
        let random = SystemRandom::new();
        let bytes = match read_key(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                std::fs::create_dir_all(data_dir).map_err(io)?;
                let mut bytes = [0u8; KEY_LEN];
                random.fill(&mut bytes).map_err(|_| CryptoError::Random)?;
                write_new_key(&path, &bytes).map_err(io)?;
                tracing::info!(path = %path.display(), "created the secret key");
                bytes.to_vec()
            }
            Err(err) => return Err(io(err)),
        };
        check_permissions(&path)?;
        let key = UnboundKey::new(&CHACHA20_POLY1305, &bytes)
            .map_err(|_| CryptoError::BadKey(path.display().to_string()))?;
        Ok(Self {
            key: LessSafeKey::new(key),
            random,
        })
    }

    /// Encrypts `plaintext` for the field at `path`.
    pub fn seal(&self, path: &str, plaintext: &SecretString) -> Result<Vec<u8>, CryptoError> {
        let mut nonce = [0u8; NONCE_LEN];
        self.random
            .fill(&mut nonce)
            .map_err(|_| CryptoError::Random)?;
        let mut sealed = plaintext.expose_secret().as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(path.as_bytes()),
                &mut sealed,
            )
            .map_err(|_| CryptoError::Unreadable)?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    /// Decrypts a value sealed for the field at `path`.
    pub fn open(&self, path: &str, ciphertext: &[u8]) -> Result<SecretString, CryptoError> {
        if ciphertext.len() < NONCE_LEN {
            return Err(CryptoError::Unreadable);
        }
        let (nonce, sealed) = ciphertext.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| CryptoError::Unreadable)?;
        let mut buffer = sealed.to_vec();
        let plain = self
            .key
            .open_in_place(nonce, Aad::from(path.as_bytes()), &mut buffer)
            .map_err(|_| CryptoError::Unreadable)?;
        let text = String::from_utf8(plain.to_vec()).map_err(|_| CryptoError::Unreadable)?;
        Ok(SecretString::from(text))
    }
}

fn read_key(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(KEY_LEN);
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() == KEY_LEN {
        Ok(bytes)
    } else {
        Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "the key file is not 32 bytes",
        ))
    }
}

fn write_new_key(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(unix)]
fn check_permissions(path: &Path) -> Result<(), CryptoError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .map_err(|source| CryptoError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions()
        .mode();
    if mode & 0o077 == 0 {
        Ok(())
    } else {
        Err(CryptoError::LoosePermissions {
            path: path.display().to_string(),
            mode: mode & 0o777,
        })
    }
}

#[cfg(not(unix))]
fn check_permissions(_path: &Path) -> Result<(), CryptoError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::{CryptoError, KEY_FILE, SecretKey};

    #[test]
    fn sealed_values_open_only_for_their_own_path_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let key = SecretKey::load_or_create(dir.path()).unwrap();
        let secret = SecretString::from("bot-token-value".to_owned());
        let sealed = key.seal("discord.bot_token", &secret).unwrap();
        assert!(
            !sealed
                .windows(b"bot-token-value".len())
                .any(|w| w == b"bot-token-value"),
            "the ciphertext does not contain the value"
        );
        let opened = key.open("discord.bot_token", &sealed).unwrap();
        assert_eq!(opened.expose_secret(), "bot-token-value");
        assert!(matches!(
            key.open("heartbeat.url", &sealed),
            Err(CryptoError::Unreadable)
        ));
        let other = tempfile::tempdir().unwrap();
        let other_key = SecretKey::load_or_create(other.path()).unwrap();
        assert!(other_key.open("discord.bot_token", &sealed).is_err());
        let again = SecretKey::load_or_create(dir.path()).unwrap();
        assert_eq!(
            again
                .open("discord.bot_token", &sealed)
                .unwrap()
                .expose_secret(),
            "bot-token-value",
            "the key file is reused, not replaced"
        );
        assert!(dir.path().join(KEY_FILE).exists());
    }

    #[cfg(unix)]
    #[test]
    fn secret_key_created_0600_and_loose_permissions_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        SecretKey::load_or_create(dir.path()).unwrap();
        let path = dir.path().join(KEY_FILE);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let err = SecretKey::load_or_create(dir.path()).unwrap_err();
        assert!(matches!(err, CryptoError::LoosePermissions { .. }), "{err}");
        assert!(err.to_string().contains("chmod 600"));
    }
}
