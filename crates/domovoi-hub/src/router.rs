//! The hub's HTTP router: the API, webhooks, health and the web UI behind the shared middleware, and
//! the OpenAPI document generated from the same routes.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::sensitive_headers::SetSensitiveHeadersLayer;

use crate::api::{self, ApiState};
use crate::auth::csrf;
use crate::ingest::{self, IngestState};
use crate::{middleware, spa};

/// The full HTTP surface: `/api/v1`, `/healthz`, `/metrics`, with `ingest`
/// `/hooks/{name}`, and the web UI for every other path. The CSRF guard covers the API; webhooks authenticate themselves.
pub fn router(api: ApiState, ingest: Option<IngestState>) -> Router {
    let limit = api::packages::upload_limit(api.deps().config.packages.max_artifact_bytes);
    let (routes, _) = api::routes_with(limit).split_for_parts();
    let routes = routes
        .layer(axum::middleware::from_fn_with_state(
            api.auth().clone(),
            csrf::guard,
        ))
        .layer(DefaultBodyLimit::max(middleware::MAX_BODY_BYTES))
        .with_state(api);
    let mut app = routes;
    if let Some(ingest) = ingest {
        app = app.merge(ingest::router(ingest));
    }
    let app = app.fallback(spa::handler);
    let [csp, nosniff, referrer, permissions] = middleware::security_headers();
    app.layer(axum::middleware::from_fn(middleware::timeout))
        .layer(
            ServiceBuilder::new()
                .layer(SetSensitiveHeadersLayer::new(middleware::SENSITIVE))
                .layer(CompressionLayer::new()),
        )
        .layer(csp)
        .layer(nosniff)
        .layer(referrer)
        .layer(permissions)
        .layer(axum::middleware::from_fn(
            crate::middleware::request_log::log_requests,
        ))
}

/// The OpenAPI document of the API.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    api::openapi()
}

/// The OpenAPI document as pretty JSON with a trailing newline, as committed in
/// `web/src/api/openapi.json`.
#[must_use]
pub fn openapi_json() -> String {
    let mut json = openapi()
        .to_pretty_json()
        .unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"));
    json.push('\n');
    json
}
