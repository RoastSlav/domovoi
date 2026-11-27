//! HTTP request handling and host resolution.

use std::collections::BTreeMap;

use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use domovoi_checks::sources::{ParseOptions, SourceFormat, SourceSignal, parse};
use domovoi_config::hub::{WebhookConfig, WebhookKind};
use domovoi_core::{Host, HostId, HostKind, HostName, SourceKind};

use domovoi_core::log_gate::Gate;

use super::IngestState;
use super::auth::{authorized, failure_reason};

/// Logs a refused webhook request (one line per hook and status a while, with the number
/// held back) and returns the status to answer with.
fn refuse(
    state: &IngestState,
    hook: &str,
    source: &str,
    status: StatusCode,
    reason: &str,
) -> StatusCode {
    // An unknown name is whatever the sender typed: one key for all of them, so random
    // names can neither flood the log nor grow the gate.
    let key = if status == StatusCode::NOT_FOUND {
        "unknown-hook/404".to_owned()
    } else {
        format!("{hook}/{}", status.as_u16())
    };
    if let Gate::Log { suppressed } = state.gate.hit_now(&key) {
        // The hook name is part of the URL the sender chose: quoted and cut.
        let shown: String = hook.chars().take(64).collect();
        tracing::warn!(
            hook = ?shown,
            %source,
            status = status.as_u16(),
            reason,
            suppressed,
            "webhook request rejected"
        );
    }
    status
}

pub(super) async fn receive(
    State(state): State<IngestState>,
    Path(name): Path<String>,
    parts: Parts,
    body: Result<Bytes, BytesRejection>,
) -> StatusCode {
    let headers = &parts.headers;
    let source = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "unknown".to_owned(), |info| info.0.ip().to_string());
    let Some(webhook) = state.webhooks.get(&name) else {
        return refuse(
            &state,
            &name,
            &source,
            StatusCode::NOT_FOUND,
            "no webhook with this name is configured",
        );
    };
    let body = match body {
        Ok(body) => body,
        Err(rejection) => {
            let status = rejection.status();
            let reason = if status == StatusCode::PAYLOAD_TOO_LARGE {
                format!(
                    "the body is larger than the {} byte limit",
                    domovoi_checks::sources::MAX_BODY_BYTES
                )
            } else {
                "the body could not be read".to_owned()
            };
            return refuse(&state, &name, &source, status, &reason);
        }
    };
    // Authenticate before charging the bucket: the bucket protects the monitor's own alerts,
    // so traffic that cannot sign in must not be able to spend it (the body is already capped
    // at 1 MiB, which bounds what a bad request costs to check).
    if !authorized(&webhook.config, headers, &body) {
        let reason = failure_reason(&webhook.config, headers);
        return refuse(&state, &name, &source, StatusCode::UNAUTHORIZED, reason);
    }
    let now = state.deps.clock.now();
    let allowed = webhook
        .limit
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take(now);
    if !allowed {
        return refuse(
            &state,
            &name,
            &source,
            StatusCode::TOO_MANY_REQUESTS,
            "this webhook's rate limit is spent; alerts are refused until it refills",
        );
    }
    let format = format(webhook.config.kind);
    let signals = match parse(
        format,
        &body,
        ParseOptions {
            host_label: webhook.config.host_label.as_deref(),
        },
    ) {
        Ok(signals) => signals,
        Err(err) => {
            let reason = format!("the body is not a valid {} payload: {err}", format.name());
            return refuse(&state, &name, &source, StatusCode::BAD_REQUEST, &reason);
        }
    };
    let mut resolved = Vec::with_capacity(signals.len());
    for signal in signals {
        match resolve(&state, &webhook.config, signal, now).await {
            Ok(signal) => resolved.push(signal),
            Err(status) => {
                let reason = if status == StatusCode::UNPROCESSABLE_ENTITY {
                    "the alert names no usable host and the webhook has no default_host"
                } else {
                    "the host could not be looked up or created"
                };
                return refuse(&state, &name, &source, status, reason);
            }
        }
    }
    if resolved.is_empty() {
        return StatusCode::ACCEPTED;
    }
    match state.deps.sink.submit(resolved).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(err) => {
            let reason = format!("the signals could not be handed to the hub: {err}");
            refuse(
                &state,
                &name,
                &source,
                StatusCode::SERVICE_UNAVAILABLE,
                &reason,
            )
        }
    }
}

fn format(kind: WebhookKind) -> SourceFormat {
    match kind {
        WebhookKind::Generic => SourceFormat::Generic,
        WebhookKind::Alertmanager => SourceFormat::Alertmanager,
        WebhookKind::UptimeKuma => SourceFormat::UptimeKuma,
    }
}

async fn resolve(
    state: &IngestState,
    config: &WebhookConfig,
    signal: SourceSignal,
    now: jiff::Timestamp,
) -> Result<domovoi_core::Signal, StatusCode> {
    // A name Domovoi cannot use, or none at all, falls back to the webhook's default.
    let name = signal
        .host
        .as_deref()
        .and_then(|h| HostName::parse(h).ok())
        .or_else(|| config.default_host.clone())
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let host_id = match state.deps.hosts.get_host_by_name(&name).await {
        Ok(Some(host)) => host.id,
        Ok(None) => {
            let host = Host {
                id: HostId::new(),
                name,
                kind: HostKind::External,
                labels: BTreeMap::new(),
                created_at: now,
            };
            match state.deps.hosts.upsert_host(&host).await {
                Ok(()) => host.id,
                Err(error) => match state.deps.hosts.get_host_by_name(&host.name).await {
                    Ok(Some(existing)) => existing.id,
                    _ => {
                        tracing::warn!(%error, "could not create a webhook host");
                        return Err(StatusCode::SERVICE_UNAVAILABLE);
                    }
                },
            }
        }
        Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
    };
    Ok(return_signal(signal, host_id, format(config.kind), now))
}

fn return_signal(
    signal: SourceSignal,
    host: HostId,
    format: SourceFormat,
    now: jiff::Timestamp,
) -> domovoi_core::Signal {
    signal.into_signal(
        host,
        SourceKind::Webhook {
            kind: format.name().to_owned(),
        },
        now,
    )
}
