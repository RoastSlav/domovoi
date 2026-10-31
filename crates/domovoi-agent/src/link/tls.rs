//! TLS to the hub: the certificate pin, the enrollment recorder and channel binding.
//!
//! The hub's certificate is self-signed, so names and CAs mean nothing here. Trust is the
//! SHA-256 of the end-entity certificate, pinned at enrollment. Handshake signatures are
//! still verified with the crypto provider, so a peer must hold the certificate's key.

use std::sync::{Arc, Mutex, PoisonError};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, Error, SignatureScheme};
use sha2::{Digest, Sha256};

/// The RFC 9266 exporter label used to bind the challenge signature to one TLS session.
pub const CHANNEL_BINDING_LABEL: &[u8] = b"EXPORTER-Channel-Binding";

/// SHA-256 of a DER certificate.
#[must_use]
pub fn fingerprint(cert: &[u8]) -> [u8; 32] {
    Sha256::digest(cert).into()
}

/// The crypto provider every agent TLS connection uses.
#[must_use]
pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn algorithms() -> WebPkiSupportedAlgorithms {
    rustls::crypto::ring::default_provider().signature_verification_algorithms
}

/// The error both verifiers return for a certificate they refuse.
fn rejected() -> Error {
    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

/// Whether `err` is a TLS failure caused by refusing the hub's certificate.
#[must_use]
pub fn is_rejected_certificate(err: &std::io::Error) -> bool {
    err.get_ref()
        .and_then(|inner| inner.downcast_ref::<Error>())
        .is_some_and(|e| *e == rejected())
}

/// Accepts only the end-entity certificate whose SHA-256 is pinned.
#[derive(Debug)]
pub struct PinnedVerifier {
    pin: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedVerifier {
    /// A verifier for the certificate with SHA-256 `pin`.
    #[must_use]
    pub fn new(pin: [u8; 32]) -> Self {
        Self {
            pin,
            algorithms: algorithms(),
        }
    }
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if fingerprint(end_entity.as_ref()) == self.pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rejected())
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Accepts the hub's certificate on first contact and records its SHA-256.
///
/// With an expected fingerprint it refuses any other certificate during the handshake, so
/// nothing is sent to an impostor.
#[derive(Debug)]
pub struct RecordingVerifier {
    expected: Option<[u8; 32]>,
    seen: Mutex<Option<[u8; 32]>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl RecordingVerifier {
    /// A recorder that, when `expected` is set, accepts only that fingerprint.
    #[must_use]
    pub fn new(expected: Option<[u8; 32]>) -> Self {
        Self {
            expected,
            seen: Mutex::new(None),
            algorithms: algorithms(),
        }
    }

    /// The fingerprint of the certificate the hub presented.
    #[must_use]
    pub fn seen(&self) -> Option<[u8; 32]> {
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let seen = fingerprint(end_entity.as_ref());
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = Some(seen);
        match self.expected {
            Some(expected) if expected != seen => Err(rejected()),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// A TLS 1.3 client configuration that trusts only what `verifier` accepts.
pub fn client_config(verifier: Arc<dyn ServerCertVerifier>) -> Result<ClientConfig, Error> {
    Ok(ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The RFC 9266 channel binding of a TLS session, as hex.
pub fn channel_binding(conn: &rustls::ClientConnection) -> Result<String, Error> {
    let binding = conn.export_keying_material([0u8; 32], CHANNEL_BINDING_LABEL, None)?;
    Ok(hex::encode(binding))
}
