//! Constant-time webhook authentication.

use axum::http::{HeaderMap, HeaderValue, header};
use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub(super) fn authorized(
    config: &domovoi_config::hub::WebhookConfig,
    headers: &HeaderMap,
    body: &[u8],
) -> bool {
    let secret = config.secret.value();
    match config.auth {
        domovoi_config::hub::WebhookAuth::Hmac => {
            verify_hmac(secret, body, headers.get("x-hub-signature-256"))
        }
        domovoi_config::hub::WebhookAuth::Bearer => {
            verify_bearer(secret, headers.get(header::AUTHORIZATION))
        }
    }
}

fn verify_hmac(secret: &SecretString, body: &[u8], header: Option<&HeaderValue>) -> bool {
    let Some(encoded) = header
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("sha256="))
        .and_then(|value| hex::decode(value).ok())
    else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.expose_secret().as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&encoded).is_ok()
}

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
