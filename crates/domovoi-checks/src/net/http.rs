//! `probe.http`: a URL answers with an expected status and, over HTTPS, a certificate that
//! is not about to expire.
//!
//! - **Target:** the URL without user info, query or fragment, so secrets in a query string
//!   never reach a fingerprint.
//! - **Fires** (warning) when the request fails or times out after [`HTTP_TIMEOUT`], when
//!   the status is not in the expected set, or when the certificate expires within
//!   [`CERT_WARNING_DAYS`].
//! - **Evidence:** `status`, `latency_ms`, `cert_days_left` (HTTPS), `error`.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass};
use jiff::SignedDuration;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};
use url::Url;

use super::{ProbeError, cert, elapsed_ms};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, key, target};

/// How long a request may take.
pub const HTTP_TIMEOUT: SignedDuration = SignedDuration::from_secs(10);
/// A certificate expiring within this many days is a warning.
pub const CERT_WARNING_DAYS: f64 = 14.0;
/// Longest URL a probe accepts; it becomes the target.
pub const MAX_URL_LEN: usize = 128;

/// The URL as it appears in targets and summaries.
#[must_use]
pub fn display_url(url: &Url) -> String {
    let mut shown = url.clone();
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn tls_error(err: impl std::fmt::Display) -> ProbeError {
    ProbeError::Tls(err.to_string())
}

fn client(mut config: ClientConfig) -> Result<reqwest::Client, ProbeError> {
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .tls_backend_preconfigured(config)
        .tls_info(true)
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(concat!("domovoi-probe/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(tls_error)
}

/// The `probe.http` check for one URL.
pub struct HttpProbe {
    key: CheckKey,
    url: Url,
    shown: String,
    expect: BTreeSet<u16>,
    client: reqwest::Client,
}

impl HttpProbe {
    /// A probe of `url` that trusts the operating system's certificate store.
    pub fn new(url: Url, expect: BTreeSet<u16>) -> Result<Self, ProbeError> {
        let provider = provider();
        let verifier =
            rustls_platform_verifier::Verifier::new(provider.clone()).map_err(tls_error)?;
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Self::build(url, expect, client(config)?)
    }

    /// A probe of `url` that trusts only `roots`, for services behind a private CA.
    pub fn with_roots(
        url: Url,
        expect: BTreeSet<u16>,
        roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, ProbeError> {
        let mut store = RootCertStore::empty();
        for root in roots {
            store.add(root).map_err(tls_error)?;
        }
        let config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .with_root_certificates(store)
            .with_no_client_auth();
        Self::build(url, expect, client(config)?)
    }

    fn build(url: Url, expect: BTreeSet<u16>, client: reqwest::Client) -> Result<Self, ProbeError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ProbeError::InvalidTarget(format!(
                "{} is not an http or https URL",
                display_url(&url)
            )));
        }
        let shown = display_url(&url);
        if shown.len() > MAX_URL_LEN {
            return Err(ProbeError::InvalidTarget(format!(
                "URLs longer than {MAX_URL_LEN} characters cannot be probed"
            )));
        }
        if expect.is_empty() {
            return Err(ProbeError::InvalidTarget(
                "at least one expected status is needed".to_owned(),
            ));
        }
        Ok(Self {
            key: key("probe.http").map_err(|e| ProbeError::InvalidTarget(e.to_string()))?,
            url,
            shown,
            expect,
            client,
        })
    }
}

#[async_trait]
impl Check for HttpProbe {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let name = &self.shown;
        let start = ctx.clock.now();
        let deadline = start.checked_add(HTTP_TIMEOUT).unwrap_or(start);
        let result = tokio::select! {
            result = self.client.get(self.url.clone()).send() => result.map_err(|e| {
                // reqwest hides the cause (such as an expired certificate) in the source chain.
                let mut text = e.to_string();
                let mut source = std::error::Error::source(&e);
                while let Some(inner) = source {
                    text = format!("{text}: {inner}");
                    source = inner.source();
                }
                text
            }),
            () = ctx.clock.sleep_until(deadline) => {
                Err(format!("no answer within {}s", HTTP_TIMEOUT.as_secs()))
            }
        };
        let now = ctx.clock.now();
        let observation = match result {
            Err(error) => {
                Observation::firing(target(name), Severity::Warning, format!("{name} is down"))
                    .with_text("error", error)
            }
            Ok(response) => {
                let status = response.status().as_u16();
                let days_left = response
                    .extensions()
                    .get::<reqwest::tls::TlsInfo>()
                    .and_then(|info| info.peer_certificate())
                    .and_then(cert::not_after)
                    .map(|expiry| expiry.duration_since(now).as_secs_f64() / 86_400.0);
                let mut problems = Vec::new();
                if !self.expect.contains(&status) {
                    problems.push(format!("answered {status}"));
                }
                if let Some(days) = days_left.filter(|d| *d < CERT_WARNING_DAYS) {
                    problems.push(format!("certificate expires in {days:.0} days"));
                }
                let mut obs = if problems.is_empty() {
                    Observation::ok(target(name), format!("{name} is up"))
                } else {
                    Observation::firing(
                        target(name),
                        Severity::Warning,
                        format!("{name}: {}", problems.join("; ")),
                    )
                };
                obs = obs
                    .with("status", EvidenceValue::Int(i64::from(status)))
                    .with_number("latency_ms", elapsed_ms(start, now));
                if let Some(days) = days_left {
                    obs = obs.with_number("cert_days_left", days);
                }
                obs
            }
        };
        Ok(CheckReport {
            observations: vec![observation],
            facts: Vec::new(),
        })
    }
}
