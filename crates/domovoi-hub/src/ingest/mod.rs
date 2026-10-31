//! Signed webhook ingest for generic, Alertmanager and Uptime Kuma payloads.

mod auth;
mod handler;
mod limit;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_config::hub::WebhookConfig;
use domovoi_core::ports::{Clock, HostRepo, SignalSink};

use self::limit::TokenBucket;

/// Adapters shared by every configured webhook.
#[derive(Clone)]
pub struct IngestDeps {
    /// Time for signal timestamps and rate-limit refill.
    pub clock: Arc<dyn Clock>,
    /// Host lookup and creation.
    pub hosts: Arc<dyn HostRepo>,
    /// Where accepted signals go.
    pub sink: Arc<dyn SignalSink>,
}

struct Webhook {
    config: WebhookConfig,
    limit: Mutex<TokenBucket>,
}

/// The configured webhooks and their adapters.
#[derive(Clone)]
pub struct IngestState {
    webhooks: Arc<BTreeMap<String, Webhook>>,
    deps: IngestDeps,
}

impl IngestState {
    /// Builds independent rate limiters for `webhooks`.
    #[must_use]
    pub fn new(webhooks: BTreeMap<String, WebhookConfig>, deps: IngestDeps) -> Self {
        let now = deps.clock.now();
        let webhooks = webhooks
            .into_iter()
            .map(|(name, config)| {
                (
                    name,
                    Webhook {
                        config,
                        limit: Mutex::new(TokenBucket::new(now)),
                    },
                )
            })
            .collect();
        Self {
            webhooks: Arc::new(webhooks),
            deps,
        }
    }
}

/// Routes signed webhook posts. The composition root merges this into the web server.
pub fn router(state: IngestState) -> axum::Router {
    use axum::extract::DefaultBodyLimit;
    use axum::routing::post;
    use domovoi_checks::sources::MAX_BODY_BYTES;

    axum::Router::new()
        .route("/hooks/{name}", post(handler::receive))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}
