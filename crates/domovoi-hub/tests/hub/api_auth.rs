use axum::http::{Method, StatusCode, header};
use jiff::SignedDuration;
use serde_json::json;

use crate::api_harness::{PASSWORD, USERNAME, api, login_of};

#[tokio::test]
async fn setup_token_is_single_use_and_creates_admin() {
    let a = api().await;
    let token = a.setup_token().await;
    assert_eq!(
        a.state.auth().open_setup().await.unwrap(),
        None,
        "one token at a time while it is valid"
    );
    let setup = |token: &str, password: &str| json!({ "token": token, "username": USERNAME, "password": password });
    let wrong = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(setup("nope", PASSWORD)),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let weak = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(setup(&token, "short")),
        )
        .await;
    assert_eq!(weak.status, StatusCode::BAD_REQUEST);

    let created = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(setup(&token, PASSWORD)),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.text);
    assert_eq!(created.body["user"]["username"], USERNAME);
    let login = login_of(&created);
    assert_eq!(
        a.get("/api/v1/session", &login).await.status,
        StatusCode::OK
    );

    let again = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(setup(&token, PASSWORD)),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT, "the token is spent");
    let stored = domovoi_core::ports::UserRepo::get_by_username(&*a.store, USERNAME)
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored
            .password_hash
            .starts_with("$argon2id$v=19$m=19456,t=2,p=1$")
    );
}

#[tokio::test]
async fn setup_token_expires_after_a_day() {
    let a = api().await;
    let token = a.setup_token().await;
    a.advance(SignedDuration::from_hours(25));
    let late = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(json!({ "token": token, "username": USERNAME, "password": PASSWORD })),
        )
        .await;
    assert_eq!(late.status, StatusCode::FORBIDDEN);
    assert!(
        a.state.auth().open_setup().await.unwrap().is_some(),
        "a new link is issued"
    );
}

#[tokio::test]
async fn setup_is_closed_once_a_user_exists() {
    let a = api().await;
    a.admin().await;
    assert_eq!(a.state.auth().open_setup().await.unwrap(), None);
    let reply = a
        .send(
            Method::POST,
            "/api/v1/auth/setup",
            None,
            Some(json!({ "token": "x", "username": "second", "password": PASSWORD })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_success_sets_hardened_cookie() {
    let a = api().await;
    a.admin().await;
    let reply = a.login(PASSWORD).await;
    assert_eq!(reply.status, StatusCode::OK);
    let cookie = reply.headers[header::SET_COOKIE].to_str().unwrap();
    for part in [
        "domovoi_session=",
        "HttpOnly",
        "SameSite=Strict",
        "Path=/",
        "Secure",
        "Max-Age=604800",
    ] {
        assert!(cookie.contains(part), "{part} missing from {cookie}");
    }
    let login = login_of(&reply);
    let session = a.get("/api/v1/session", &login).await;
    assert_eq!(session.status, StatusCode::OK);
    assert_eq!(session.body["csrf_token"], login.csrf.as_str());

    let out = a.post("/api/v1/auth/logout", &login, json!({})).await;
    assert_eq!(out.status, StatusCode::NO_CONTENT);
    assert!(
        out.headers[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        a.get("/api/v1/session", &login).await.status,
        StatusCode::UNAUTHORIZED,
        "logout deletes the session"
    );
}

#[tokio::test]
async fn login_rate_limit_locks_out() {
    let a = api().await;
    a.admin().await;
    for _ in 0..5 {
        let reply = a.login("wrong password!").await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
        assert_eq!(reply.body["message"], "wrong username or password");
    }
    let locked = a.login(PASSWORD).await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "even the right password"
    );
    assert_eq!(locked.headers[header::RETRY_AFTER], "60");

    // Failing again after the lockout doubles it, up to 15 minutes.
    for expected in ["120", "240", "480", "900", "900"] {
        a.advance(SignedDuration::from_secs(1000));
        for _ in 0..5 {
            a.login("wrong password!").await;
        }
        assert_eq!(
            a.login(PASSWORD).await.headers[header::RETRY_AFTER],
            expected
        );
    }

    let unknown = a
        .send(
            Method::POST,
            "/api/v1/auth/login",
            None,
            Some(json!({ "username": "nobody", "password": PASSWORD })),
        )
        .await;
    assert_eq!(
        unknown.status,
        StatusCode::TOO_MANY_REQUESTS,
        "limits are per address"
    );

    // A successful login forgets the address.
    a.advance(SignedDuration::from_secs(1000));
    assert_eq!(a.login(PASSWORD).await.status, StatusCode::OK);
    a.login("wrong password!").await;
    assert_eq!(a.login(PASSWORD).await.status, StatusCode::OK);
}

#[tokio::test]
async fn unauthenticated_requests_get_401() {
    let a = api().await;
    a.admin().await;
    for uri in [
        "/api/v1/session",
        "/api/v1/incidents",
        &format!("/api/v1/incidents/{}", a.incident.id()),
        "/api/v1/actions",
        "/api/v1/hosts",
        "/api/v1/hosts/nas/skills",
        "/api/v1/notes",
        "/api/v1/settings",
        "/api/v1/advisories",
        "/api/v1/digest/preview",
        "/api/v1/events",
    ] {
        let reply = a.send(Method::GET, uri, None, None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(reply.body["error"], "unauthorized");
    }
    let ack = a
        .send(
            Method::POST,
            &format!("/api/v1/incidents/{}/ack", a.incident.id()),
            None,
            None,
        )
        .await;
    assert_eq!(ack.status, StatusCode::UNAUTHORIZED);
    assert_eq!(a.send(Method::GET, "/healthz", None, None).await.text, "ok");
}

#[tokio::test]
async fn sessions_expire_when_idle_and_after_a_week() {
    let a = api().await;
    let login = a.admin().await;
    for _ in 0..3 {
        a.advance(SignedDuration::from_hours(11));
        assert_eq!(
            a.get("/api/v1/session", &login).await.status,
            StatusCode::OK
        );
    }
    a.advance(SignedDuration::from_hours(13));
    assert_eq!(
        a.get("/api/v1/session", &login).await.status,
        StatusCode::UNAUTHORIZED,
        "idle for more than 12 h"
    );

    let login = login_of(&a.login(PASSWORD).await);
    for _ in 0..16 {
        a.advance(SignedDuration::from_hours(11));
        a.get("/api/v1/session", &login).await;
    }
    assert_eq!(
        a.get("/api/v1/session", &login).await.status,
        StatusCode::UNAUTHORIZED,
        "a week after login, however active"
    );
}

#[tokio::test]
async fn setup_required_until_first_user() {
    let a = api().await;
    let before = a.send(Method::GET, "/api/v1/session", None, None).await;
    assert_eq!(before.status, StatusCode::UNAUTHORIZED);
    assert_eq!(before.body["error"], "setup_required");
    let stale = a
        .send(
            Method::GET,
            "/api/v1/incidents",
            Some(&crate::api_harness::Login {
                cookie: "domovoi_session=gone".to_owned(),
                csrf: String::new(),
            }),
            None,
        )
        .await;
    assert_eq!(
        stale.body["error"], "setup_required",
        "even with a stale cookie"
    );

    a.admin().await;
    let after = a.send(Method::GET, "/api/v1/session", None, None).await;
    assert_eq!(after.status, StatusCode::UNAUTHORIZED);
    assert_eq!(after.body["error"], "unauthorized");
}
