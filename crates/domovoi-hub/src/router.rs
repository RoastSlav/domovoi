//! The hub's HTTP router: the API, webhooks and health behind the shared middleware, and
//! the OpenAPI document generated from the same routes.

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::sensitive_headers::SetSensitiveHeadersLayer;
use tower_http::trace::TraceLayer;

use crate::api::{self, ApiState};
use crate::auth::csrf;
use crate::ingest::{self, IngestState};
use crate::middleware;

/// The full HTTP surface: `/api/v1`, `/healthz`, `/metrics` and, with `ingest`,
/// `/hooks/{name}`. The CSRF guard covers the API; webhooks authenticate themselves.
pub fn router(api: ApiState, ingest: Option<IngestState>) -> Router {
    let (routes, _) = api::routes().split_for_parts();
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
    let [csp, nosniff, referrer, permissions] = middleware::security_headers();
    app.layer(axum::middleware::from_fn(middleware::timeout))
        .layer(
            ServiceBuilder::new()
                .layer(SetSensitiveHeadersLayer::new(middleware::SENSITIVE))
                .layer(TraceLayer::new_for_http())
                .layer(CompressionLayer::new()),
        )
        .layer(csp)
        .layer(nosniff)
        .layer(referrer)
        .layer(permissions)
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
