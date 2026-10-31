//! The hub's TLS identity: a self-signed certificate that agents pin at enrollment.
//!
//! Created once under `<data_dir>/tls/` and reused, because changing it would break
//! every enrolled agent's pin. The key file is readable by the hub user only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};
use sha2::{Digest, Sha256};

use super::error::AgentServerError;

/// The RFC 9266 exporter label; the agent signs the challenge over the same value.
pub const CHANNEL_BINDING_LABEL: &[u8] = b"EXPORTER-Channel-Binding";

/// The name every hub certificate carries besides the hub's own name.
pub const HUB_SAN: &str = "domovoi-hub";

/// How long a new certificate is valid.
const VALID_YEARS: i16 = 10;

const CERT_FILE: &str = "cert.der";
const KEY_FILE: &str = "key.der";

/// The hub's certificate and key.
pub struct TlsIdentity {
    cert: CertificateDer<'static>,
    key: Vec<u8>,
    fingerprint: [u8; 32],
}

impl std::fmt::Debug for TlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsIdentity")
            .field("fingerprint", &colon_hex(&self.fingerprint))
            .finish_non_exhaustive()
    }
}

impl TlsIdentity {
    /// Loads the identity from `dir`, or creates it there when neither file exists.
    pub fn load_or_create(
        dir: &Path,
        hub_name: &str,
        now: Timestamp,
    ) -> Result<Self, AgentServerError> {
        let err = |reason: String| AgentServerError::Tls {
            path: dir.display().to_string(),
            reason,
        };
        let cert_path = dir.join(CERT_FILE);
        let key_path = dir.join(KEY_FILE);
        match (cert_path.exists(), key_path.exists()) {
            (true, true) => {
                let cert = fs::read(&cert_path).map_err(|e| err(e.to_string()))?;
                let key = fs::read(&key_path).map_err(|e| err(e.to_string()))?;
                Ok(Self::from_der(cert, key))
            }
            (false, false) => {
                let (cert, key) = generate(hub_name, now).map_err(err)?;
                create_private_dir(dir).map_err(|e| err(e.to_string()))?;
                write_new(&cert_path, &cert, false).map_err(|e| err(e.to_string()))?;
                write_new(&key_path, &key, true).map_err(|e| err(e.to_string()))?;
                Ok(Self::from_der(cert, key))
            }
            _ => Err(err(format!(
                "only one of {CERT_FILE} and {KEY_FILE} exists; restore the other or remove both \
                 (removing both makes a new certificate, and every agent must enroll again)"
            ))),
        }
    }

    fn from_der(cert: Vec<u8>, key: Vec<u8>) -> Self {
        let fingerprint = Sha256::digest(&cert).into();
        Self {
            cert: CertificateDer::from(cert),
            key,
            fingerprint,
        }
    }

    /// SHA-256 of the DER certificate: what agents pin.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    /// The fingerprint as colon-separated hex, the form shown to people.
    #[must_use]
    pub fn fingerprint_text(&self) -> String {
        colon_hex(&self.fingerprint)
    }

    /// A TLS 1.3 server configuration presenting this certificate.
    pub fn server_config(&self) -> Result<ServerConfig, rustls::Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(vec![self.cert.clone()], key)
    }
}

/// The channel binding of one TLS session, hex-encoded. The agent computes the same value
/// on its side and signs the challenge over it, so a signature cannot be replayed on
/// another connection.
pub fn channel_binding(conn: &ServerConnection) -> Result<String, rustls::Error> {
    let binding = conn.export_keying_material([0u8; 32], CHANNEL_BINDING_LABEL, None)?;
    Ok(hex::encode(binding))
}

/// `bytes` as uppercase hex pairs joined by colons.
#[must_use]
pub fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn generate(hub_name: &str, now: Timestamp) -> Result<(Vec<u8>, Vec<u8>), String> {
    let key =
        rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(|e| e.to_string())?;
    let names = vec![hub_name.to_owned(), HUB_SAN.to_owned()];
    let mut params = rcgen::CertificateParams::new(names)
        .map_err(|e| format!("the hub name {hub_name:?} cannot be used in a certificate ({e})"))?;
    let mut subject = rcgen::DistinguishedName::new();
    subject.push(rcgen::DnType::CommonName, hub_name);
    params.distinguished_name = subject;
    let today = now.to_zoned(TimeZone::UTC).date();
    // Day 28 at most, so the expiry date exists in every month and year.
    let day = today.day().min(28);
    let month = today.month();
    let ymd = |year: i16| {
        rcgen::date_time_ymd(
            i32::from(year),
            u8::try_from(month).unwrap_or(1),
            u8::try_from(day).unwrap_or(1),
        )
    };
    params.not_before = ymd(today.year());
    params.not_after = ymd(today.year().saturating_add(VALID_YEARS));
    let cert = params.self_signed(&key).map_err(|e| e.to_string())?;
    Ok((cert.der().to_vec(), key.serialize_der()))
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn write_new(path: &PathBuf, bytes: &[u8], private: bool) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
