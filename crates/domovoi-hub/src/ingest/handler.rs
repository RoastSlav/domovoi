//! HTTP request handling and host resolution.

use std::collections::BTreeMap;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use domovoi_checks::sources::{ParseOptions, SourceFormat, SourceSignal, parse};
use domovoi_config::hub::{WebhookConfig, WebhookKind};
use domovoi_core::{Host, HostId, HostKind, HostName, SourceKind};

use super::IngestState;
use super::auth::authorized;

pub(super) async fn receive(
    State(state): State<IngestState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    let Some(webhook) = state.webhooks.get(&name) else {
        return StatusCode::NOT_FOUND;
    };
    let now = state.deps.clock.now();
    let allowed = webhook
        .limit
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take(now);
    if !allowed {
        return StatusCode::TOO_MANY_REQUESTS;
    }
    if !authorized(&webhook.config, &headers, &body) {
        return StatusCode::UNAUTHORIZED;
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
        Err(_) => return StatusCode::BAD_REQUEST,
    };
    let mut resolved = Vec::with_capacity(signals.len());
    for signal in signals {
        match resolve(&state, &webhook.config, signal, now).await {
            Ok(signal) => resolved.push(signal),
            Err(status) => return status,
        }
    }
    if resolved.is_empty() {
        return StatusCode::ACCEPTED;
    }
    match state.deps.sink.submit(resolved).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
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
    let name = signal
        .host
        .as_deref()
        .map(HostName::parse)
        .transpose()
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?
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
