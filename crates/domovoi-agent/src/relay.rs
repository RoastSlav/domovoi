//! Loopback webhook relay: local services report through the agent's authenticated link.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::routing::post;
use domovoi_checks::sources::{MAX_BODY_BYTES, ParseOptions, SourceFormat, parse};
use domovoi_config::agent::LocalWebhooks;
use domovoi_core::ports::Clock;
use domovoi_core::{EvidenceValue, HostId, HostName, SourceKind};
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::spool::SharedSpool;

/// Why the local webhook relay could not start or serve.
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    /// Exposing the relay requires an explicit opt-in.
    #[error("local webhook bind {0} is not loopback; set allow_non_loopback = true to allow it")]
    NonLoopback(SocketAddr),
    /// The configured listener could not be opened.
    #[error("could not bind the local webhook relay: {0}")]
    Bind(#[source] std::io::Error),
    /// The HTTP listener stopped.
    #[error("the local webhook relay stopped: {0}")]
    Serve(#[source] std::io::Error),
}

/// A bound local webhook listener.
pub struct Relay {
    listener: TcpListener,
    token: Option<SecretString>,
    host_name: HostName,
}

impl Relay {
    /// Binds the configured address for the agent of `host_name`, refusing non-loopback
    /// exposure by default.
    pub async fn bind(config: &LocalWebhooks, host_name: HostName) -> Result<Self, RelayError> {
        if !config.bind.ip().is_loopback() && !config.allow_non_loopback {
            return Err(RelayError::NonLoopback(config.bind));
        }
        let listener = TcpListener::bind(config.bind)
            .await
            .map_err(RelayError::Bind)?;
        Ok(Self {
            listener,
            token: config.token.as_ref().map(|token| token.value().clone()),
            host_name,
        })
    }

    /// The actual listen address, including the assigned port when configured with port zero.
    pub fn local_addr(&self) -> Result<SocketAddr, RelayError> {
        self.listener.local_addr().map_err(RelayError::Bind)
    }

    /// Serves until `shutdown` is cancelled, spooling accepted signals for the existing link.
    pub async fn serve(
        self,
        spool: Arc<SharedSpool>,
        host: HostId,
        clock: Arc<dyn Clock>,
        shutdown: CancellationToken,
    ) -> Result<(), RelayError> {
        let state = RelayState {
            spool,
            host,
            clock,
            token: self.token,
            host_name: self.host_name,
        };
        let app = axum::Router::new()
            .route("/hooks/generic", post(generic))
            .route("/hooks/alertmanager", post(alertmanager))
            .route("/hooks/uptime_kuma", post(uptime_kuma))
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(state);
        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
            .map_err(RelayError::Serve)
    }
}

#[derive(Clone)]
struct RelayState {
    spool: Arc<SharedSpool>,
    host: HostId,
    clock: Arc<dyn Clock>,
    token: Option<SecretString>,
    host_name: HostName,
}

async fn generic(State(state): State<RelayState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    receive(state, SourceFormat::Generic, headers, body).await
}

async fn alertmanager(
    State(state): State<RelayState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    receive(state, SourceFormat::Alertmanager, headers, body).await
}

async fn uptime_kuma(
    State(state): State<RelayState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    receive(state, SourceFormat::UptimeKuma, headers, body).await
}

async fn receive(
    state: RelayState,
    format: SourceFormat,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if !authorized(state.token.as_ref(), headers.get(header::AUTHORIZATION)) {
        return StatusCode::UNAUTHORIZED;
    }
    let parsed = match parse(format, &body, ParseOptions::default()) {
        Ok(parsed) => parsed,
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    let now = state.clock.now();
    let signals = parsed
        .into_iter()
        .map(|mut source| {
            // Every relayed signal is about this host; a different name is kept as evidence.
            if let Some(reported) = source
                .host
                .take()
                .filter(|h| h.as_str() != state.host_name.as_str())
            {
                source
                    .evidence
                    .insert("reported_host".to_owned(), EvidenceValue::Text(reported));
            }
            source.into_signal(
                state.host,
                SourceKind::Webhook {
                    kind: format.name().to_owned(),
                },
                now,
            )
        })
        .collect::<Vec<_>>();
    if signals.is_empty() {
        return StatusCode::ACCEPTED;
    }
    match state.spool.push(signals).await {
        Ok(_) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn authorized(secret: Option<&SecretString>, header: Option<&HeaderValue>) -> bool {
    let Some(secret) = secret else {
        return true;
    };
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
