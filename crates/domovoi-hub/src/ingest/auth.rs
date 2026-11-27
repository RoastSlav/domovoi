//! Constant-time webhook authentication.

use axum::http::{HeaderMap, HeaderValue, header};

use crate::auth::bearer::verify_bearer;
use hmac::{Hmac, KeyInit, Mac};
use secrecy::{ExposeSecret, SecretString};
use sha2::Sha256;

/// The header carrying `sha256=<hex>` over the raw body, for `auth = "hmac"`.
pub const SIGNATURE_HEADER: &str = "x-domovoi-signature";

pub(super) fn authorized(
    config: &domovoi_config::hub::WebhookConfig,
    headers: &HeaderMap,
    body: &[u8],
) -> bool {
    let secret = config.secret.value();
    match config.auth {
        domovoi_config::hub::WebhookAuth::Hmac => {
            verify_hmac(secret, body, headers.get(SIGNATURE_HEADER))
        }
        domovoi_config::hub::WebhookAuth::Bearer => {
            verify_bearer(secret, headers.get(header::AUTHORIZATION))
        }
    }
}

/// Why a request that failed [`authorized`] failed, as a sentence for the log. It never
/// repeats what the sender supplied.
pub(super) fn failure_reason(
    config: &domovoi_config::hub::WebhookConfig,
    headers: &HeaderMap,
) -> &'static str {
    match config.auth {
        domovoi_config::hub::WebhookAuth::Hmac => match headers.get(SIGNATURE_HEADER) {
            None => "the request has no x-domovoi-signature header",
            Some(_) => "the signature does not match the body and the hook secret",
        },
        domovoi_config::hub::WebhookAuth::Bearer => match headers.get(header::AUTHORIZATION) {
            None => "the request has no Authorization header",
            Some(_) => "the bearer token does not match the hook secret",
        },
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
