//! Constant-time check of a bearer token, shared by webhook ingest and `/metrics`.

use axum::http::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Whether `header` is `Bearer <secret>`, compared in constant time over digests.
pub(crate) fn verify_bearer(secret: &SecretString, header: Option<&HeaderValue>) -> bool {
    let Some(token) = header
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    let expected = Sha256::digest(secret.expose_secret().as_bytes());
    let actual = Sha256::digest(token.as_bytes());
    expected.as_slice().ct_eq(actual.as_slice()).into()
}
