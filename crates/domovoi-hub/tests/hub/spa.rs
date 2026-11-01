use axum::http::{Method, StatusCode, Uri, header};
use axum::response::Response;
use domovoi_hub::spa::{self, IMMUTABLE, NO_CACHE};

#[derive(rust_embed::Embed)]
#[folder = "tests/fixtures/dist"]
struct Fixture;

fn get(path: &str) -> Response {
    spa::serve::<Fixture>(&Method::GET, &path.parse::<Uri>().unwrap())
}

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn header_of(response: &Response, name: header::HeaderName) -> &str {
    response.headers()[name].to_str().unwrap()
}

#[tokio::test]
async fn spa_fallback_serves_index_for_client_routes_and_not_for_api() {
    for path in [
        "/",
        "/incidents",
        "/incidents/0192abcd?status=open",
        "/approvals",
    ] {
        let response = get(path);
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(header_of(&response, header::CACHE_CONTROL), NO_CACHE);
        assert!(header_of(&response, header::CONTENT_TYPE).starts_with("text/html"));
        assert!(
            text(response).await.contains("<title>fixture</title>"),
            "{path}"
        );
    }
    for path in ["/api", "/api/v1/nope", "/hooks/x/y"] {
        let response = get(path);
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        let body = text(response).await;
        assert!(body.contains("\"not_found\""), "{path}: {body}");
        assert!(!body.contains("fixture"), "{path}");
    }
    let post = spa::serve::<Fixture>(&Method::POST, &"/incidents".parse().unwrap());
    assert_eq!(post.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn hashed_assets_are_immutable() {
    let response = get("/assets/app-abc123.js");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header_of(&response, header::CACHE_CONTROL), IMMUTABLE);
    assert!(header_of(&response, header::CONTENT_TYPE).contains("javascript"));
    assert_eq!(text(response).await, "console.log(\"fixture\");\n");

    let missing = get("/assets/app-gone.js");
    assert_eq!(
        missing.status(),
        StatusCode::NOT_FOUND,
        "a stale asset is not the app"
    );
}

#[cfg(not(feature = "embed-ui"))]
#[tokio::test]
async fn without_the_ui_the_hub_explains_how_to_build_it() {
    let response = spa::handler(Method::GET, "/incidents".parse().unwrap()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = text(response).await;
    assert!(body.contains("--features embed-ui"));
    assert!(!body.contains("<script"), "the page passes the CSP");
}

#[cfg(feature = "embed-ui")]
#[tokio::test]
async fn embedded_ui_serves_the_built_index() {
    let response = spa::handler(Method::GET, "/incidents".parse().unwrap()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(text(response).await.contains("<div id=\"root\">"));
}
