//! What the HTTP adapters share: the client, sending a request and mapping failures.

use std::time::Duration;

use domovoi_core::ports::LlmError;
use jiff::{SignedDuration, Timestamp};
use reqwest::header::{DATE, HeaderMap, RETRY_AFTER};
use reqwest::{RequestBuilder, StatusCode};
use secrecy::SecretString;
use serde::de::DeserializeOwned;

use crate::error::{LlmBuildError, excerpt};

/// How long establishing a connection may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A client with Domovoi's user agent, a connect timeout and `timeout` for the whole
/// request. TLS uses the process-wide rustls provider, which the binary installs.
pub(crate) fn client(timeout: SignedDuration) -> Result<reqwest::Client, LlmBuildError> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout.unsigned_abs())
        .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| LlmBuildError::Client(e.to_string()))
}

/// `path` under `base`, keeping any path `base` already has (`…/v1` + `chat/completions`).
pub(crate) fn endpoint(base: &url::Url, path: &str) -> String {
    format!("{}/{path}", base.as_str().trim_end_matches('/'))
}

/// Sends `request` and decodes a successful JSON answer, mapping every failure to an
/// [`LlmError`]. `secret` is removed from anything that ends up in an error.
pub(crate) async fn send<T: DeserializeOwned>(
    request: RequestBuilder,
    secret: Option<&SecretString>,
) -> Result<T, LlmError> {
    let response = request.send().await.map_err(|e| send_error(&e, secret))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes().await.map_err(|e| send_error(&e, secret))?;
    tracing::debug!(status = status.as_u16(), bytes = body.len(), "llm response");
    if !status.is_success() {
        return Err(status_error(status, &headers, &body, secret));
    }
    serde_json::from_slice(&body).map_err(|e| {
        LlmError::InvalidResponse(format!(
            "{e}: {}",
            excerpt(&String::from_utf8_lossy(&body), secret)
        ))
    })
}

/// A transport failure: no connection, a timeout or a broken body.
fn send_error(err: &reqwest::Error, secret: Option<&SecretString>) -> LlmError {
    let text = excerpt(&without_url(err), secret);
    if err.is_timeout() {
        LlmError::Unavailable(format!("timed out: {text}"))
    } else {
        LlmError::Unavailable(text)
    }
}

fn without_url(err: &reqwest::Error) -> String {
    // The URL is the configured base URL, which may carry credentials in its query.
    let mut err = err.to_string();
    if let Some(start) = err.find(" for url (")
        && let Some(end) = err[start..].find(')')
    {
        err.replace_range(start..=start + end, "");
    }
    err
}

/// A non-success status.
pub(crate) fn status_error(
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
    secret: Option<&SecretString>,
) -> LlmError {
    let detail = || {
        let text = excerpt(&String::from_utf8_lossy(body), secret);
        if text.is_empty() {
            format!("HTTP {}", status.as_u16())
        } else {
            format!("HTTP {}: {text}", status.as_u16())
        }
    };
    match status.as_u16() {
        401 | 403 => LlmError::Auth,
        429 => LlmError::RateLimited {
            retry_after: retry_after(headers),
        },
        408 | 529 | 500..=599 => LlmError::Unavailable(detail()),
        _ => LlmError::InvalidResponse(detail()),
    }
}

/// The wait a `retry-after` header asks for: delay seconds, or an HTTP date measured
/// against the response's own `date` header so the local clock plays no part. A date
/// already passed gives zero.
pub(crate) fn retry_after(headers: &HeaderMap) -> Option<SignedDuration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u32>() {
        return Some(SignedDuration::from_secs(i64::from(seconds)));
    }
    let at = http_date(value)?;
    let now = http_date(headers.get(DATE)?.to_str().ok()?)?;
    let wait = at.duration_since(now);
    Some(if wait.is_negative() {
        SignedDuration::ZERO
    } else {
        wait
    })
}

fn http_date(value: &str) -> Option<Timestamp> {
    jiff::fmt::rfc2822::parse(value.trim())
        .ok()
        .map(|z| z.timestamp())
}
