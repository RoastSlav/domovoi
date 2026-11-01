//! The web UI: the built React app from `web/dist`, embedded with `rust-embed` behind the
//! `embed-ui` feature, with an `index.html` fallback for client-side routes. Without the
//! feature, a short page explains how to build the UI.

use axum::Json;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

use crate::api::error::ErrorBody;

/// The `Cache-Control` of hashed files under `/assets/`, whose names change with their
/// content.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The `Cache-Control` of `index.html` and other unhashed files.
pub const NO_CACHE: &str = "no-cache";

/// The built web UI from `web/dist`.
#[cfg(feature = "embed-ui")]
#[derive(Embed)]
#[folder = "../../web/dist"]
pub struct WebDist;

/// What the hub serves at `/` when built without the web UI.
pub const NO_UI_PAGE: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Domovoi</title></head>
<body>
<main>
<h1>Domovoi</h1>
<p>This hub was built without the web UI. The API is available under <code>/api/v1</code>.</p>
<p>To include the UI, run <code>npm ci &amp;&amp; npm run build</code> in <code>web/</code>, then build the hub with <code>--features embed-ui</code>.</p>
</main>
</body>
</html>
"#;

/// Serves everything no other route matched: the web UI, or the page explaining how to
/// build it.
pub async fn handler(method: Method, uri: Uri) -> Response {
    #[cfg(feature = "embed-ui")]
    {
        serve::<WebDist>(&method, &uri)
    }
    #[cfg(not(feature = "embed-ui"))]
    {
        no_ui(&method, &uri)
    }
}

/// Whether `path` belongs to the server rather than the web UI, so it is never answered
/// with `index.html`.
fn server_path(path: &str) -> bool {
    ["/api", "/hooks"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

fn not_found() -> Response {
    let body = ErrorBody {
        error: "not_found".to_owned(),
        message: "not found".to_owned(),
    };
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

/// The response for paths the UI never serves, if `method` and `path` are such a request.
fn refuse(method: &Method, path: &str) -> Option<Response> {
    if server_path(path) {
        return Some(not_found());
    }
    if method != Method::GET && method != Method::HEAD {
        return Some(StatusCode::METHOD_NOT_ALLOWED.into_response());
    }
    None
}

fn no_ui(method: &Method, uri: &Uri) -> Response {
    if let Some(refused) = refuse(method, uri.path()) {
        return refused;
    }
    with_cache(Html(NO_UI_PAGE).into_response(), NO_CACHE)
}

fn with_cache(mut response: Response, cache: &'static str) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

fn file<A: Embed>(path: &str) -> Option<Response> {
    let file = A::get(path)?;
    let mime = HeaderValue::from_str(file.metadata.mimetype()).ok()?;
    let cache = if path.starts_with("assets/") {
        IMMUTABLE
    } else {
        NO_CACHE
    };
    let mut response = file.data.into_owned().into_response();
    response.headers_mut().insert(header::CONTENT_TYPE, mime);
    Some(with_cache(response, cache))
}

/// Serves `uri` from the files of `A`: the file itself when it exists, a 404 for missing
/// assets and server paths, and `index.html` for everything else, so client routes load
/// the app.
pub fn serve<A: Embed>(method: &Method, uri: &Uri) -> Response {
    let path = uri.path();
    if let Some(refused) = refuse(method, path) {
        return refused;
    }
    let relative = path.trim_start_matches('/');
    if !relative.is_empty()
        && let Some(found) = file::<A>(relative)
    {
        return found;
    }
    if relative.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    file::<A>("index.html").unwrap_or_else(|| no_ui(method, uri))
}
