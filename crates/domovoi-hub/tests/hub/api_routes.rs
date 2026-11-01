use axum::http::{Method, StatusCode};
use domovoi_core::Risk;
use domovoi_core::ports::{AuditFilter, AuditLog, IncidentRepo, Page};
use domovoi_testkit::build::{open_incident, resolved_incident};
use futures::StreamExt;
use serde_json::{Value, json};

use crate::api_harness::{Login, api};

async fn incidents_page(a: &crate::api_harness::Api, login: &Login, query: &str) -> Value {
    let reply = a.get(&format!("/api/v1/incidents{query}"), login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

#[tokio::test]
async fn incident_list_pagination_and_filters() {
    let a = api().await;
    let login = a.admin().await;
    let other = domovoi_testkit::build::host("media").unwrap();
    domovoi_core::ports::HostRepo::upsert_host(&*a.store, &other)
        .await
        .unwrap();
    for (host, check) in [
        (a.nas.id, "cpu.load"),
        (a.nas.id, "mem.used"),
        (other.id, "disk.usage"),
    ] {
        let incident = open_incident(host, check, None).unwrap();
        IncidentRepo::insert(&*a.store, &incident).await.unwrap();
    }
    let done = resolved_incident(a.nas.id, "net.ping", None, a.now()).unwrap();
    IncidentRepo::insert(&*a.store, &done).await.unwrap();

    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let query = match &cursor {
            Some(c) => format!("?limit=2&cursor={c}"),
            None => "?limit=2".to_owned(),
        };
        let page = incidents_page(&a, &login, &query).await;
        let items = page["items"].as_array().unwrap();
        assert!(items.len() <= 2);
        seen.extend(items.iter().map(|i| i["id"].as_str().unwrap().to_owned()));
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 5, "every incident once");
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 5);

    let resolved = incidents_page(&a, &login, "?status=resolved").await;
    assert_eq!(resolved["items"].as_array().unwrap().len(), 1);
    assert_eq!(resolved["items"][0]["id"], done.id().to_string());
    let on_media = incidents_page(&a, &login, "?host=media").await;
    assert_eq!(on_media["items"][0]["check"], "disk.usage");
    assert_eq!(on_media["items"].as_array().unwrap().len(), 1);
    let critical = incidents_page(&a, &login, "?severity=critical").await;
    assert!(critical["items"].as_array().unwrap().is_empty());
    let warning = incidents_page(&a, &login, "?severity=warning&status=open").await;
    assert_eq!(warning["items"].as_array().unwrap().len(), 4);

    assert_eq!(
        a.get("/api/v1/incidents?status=sleeping", &login)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        a.get("/api/v1/incidents?host=nowhere", &login).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn incident_detail_ack_and_mute() {
    let a = api().await;
    let login = a.admin().await;
    let id = a.incident.id();
    let detail = a.get(&format!("/api/v1/incidents/{id}"), &login).await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    assert_eq!(detail.body["host"], "nas");
    assert_eq!(detail.body["card"]["headline"], a.incident.title());
    assert_eq!(detail.body["report"], Value::Null);

    let acked = a
        .post(&format!("/api/v1/incidents/{id}/ack"), &login, json!({}))
        .await;
    assert_eq!(acked.status, StatusCode::NO_CONTENT, "{}", acked.text);
    let muted = a
        .post(
            &format!("/api/v1/incidents/{id}/mute"),
            &login,
            json!({ "minutes": 60 }),
        )
        .await;
    assert_eq!(muted.status, StatusCode::NO_CONTENT, "{}", muted.text);
    let detail = a.get(&format!("/api/v1/incidents/{id}"), &login).await;
    assert_eq!(detail.body["incident"]["status"], "muted");
    let too_long = a
        .post(
            &format!("/api/v1/incidents/{id}/mute"),
            &login,
            json!({ "minutes": 0 }),
        )
        .await;
    assert_eq!(too_long.status, StatusCode::BAD_REQUEST);
    let ask = a
        .post(
            &format!("/api/v1/incidents/{id}/ask"),
            &login,
            json!({ "question": "why?" }),
        )
        .await;
    assert_eq!(
        ask.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "triage is off here"
    );
}

#[tokio::test]
async fn approve_and_deny_flow_through_approval_service() {
    let a = api().await;
    let login = a.admin().await;
    let first = a.propose(Risk::Write).await;
    let second = a.propose(Risk::Write).await;

    let approved = a
        .post(
            &format!("/api/v1/actions/{}/approve", first.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.text);
    assert_eq!(approved.body["status"], "approved");
    assert!(
        approved.body["approved_by"]
            .as_str()
            .unwrap()
            .starts_with("user:")
    );
    let again = a
        .post(
            &format!("/api/v1/actions/{}/approve", first.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);

    let denied = a
        .post(
            &format!("/api/v1/actions/{}/deny", second.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(denied.status, StatusCode::OK);
    assert_eq!(denied.body["status"], "denied");

    let filter = AuditFilter {
        kind: Some("action.approved".to_owned()),
        ..AuditFilter::default()
    };
    let audit = AuditLog::list(&*a.store, &filter, &Page::first(5))
        .await
        .unwrap()
        .items;
    assert_eq!(audit.len(), 1);
    assert!(audit[0].actor.starts_with("user:"));

    let listed = a.get("/api/v1/actions?status=denied", &login).await;
    assert_eq!(listed.body["items"][0]["id"], second.id().to_string());
    assert_eq!(
        a.post("/api/v1/actions/not-an-id/approve", &login, json!({}))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sse_stream_emits_incident_update() {
    let a = api().await;
    let login = a.admin().await;
    let req = axum::http::Request::builder()
        .uri("/api/v1/events")
        .header(axum::http::header::COOKIE, &login.cookie)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = tower::ServiceExt::oneshot(a.app.clone(), req)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[axum::http::header::CONTENT_TYPE],
        "text/event-stream"
    );
    let mut body = response.into_body().into_data_stream();

    let ack = a
        .post(
            &format!("/api/v1/incidents/{}/ack", a.incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(ack.status, StatusCode::NO_CONTENT);
    let frame = tokio::time::timeout(std::time::Duration::from_secs(10), body.next())
        .await
        .expect("an event arrives")
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&frame);
    assert!(text.starts_with("event: incident"), "{text}");
    assert!(text.contains("\"event\":\"incident_acked\""), "{text}");
    assert!(text.contains(&a.incident.id().to_string()));
}

#[tokio::test]
async fn security_headers_present_on_api_and_healthz() {
    let a = api().await;
    let login = a.admin().await;
    let replies = [
        a.send(Method::GET, "/healthz", None, None).await,
        a.send(Method::GET, "/api/v1/session", None, None).await,
        a.get("/api/v1/incidents", &login).await,
        a.send(Method::GET, "/metrics", None, None).await,
        a.send(Method::GET, "/incidents", None, None).await,
        a.send(Method::GET, "/api/v1/nope", None, None).await,
    ];
    for reply in replies {
        let h = &reply.headers;
        assert_eq!(
            h["content-security-policy"],
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'"
        );
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["referrer-policy"], "same-origin");
        assert_eq!(h["permissions-policy"], "interest-cohort=()");
    }
}

#[tokio::test]
async fn hosts_enrollment_skills_and_notes() {
    let a = api().await;
    let login = a.admin().await;
    let hosts = a.get("/api/v1/hosts", &login).await;
    assert_eq!(hosts.body[0]["name"], "nas");
    assert_eq!(hosts.body[0]["connected"], false);

    let invite = a
        .post("/api/v1/hosts/backup-1/enrollment", &login, json!({}))
        .await;
    assert_eq!(invite.status, StatusCode::OK, "{}", invite.text);
    assert_eq!(invite.body["token"], "enroll-token-shown-once");
    assert_eq!(
        a.post("/api/v1/hosts/Bad%20Name/enrollment", &login, json!({}))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let revoke = a.post("/api/v1/hosts/nas/revoke", &login, json!({})).await;
    assert_eq!(revoke.body["revoked"], false);

    a.link.set_skills(
        a.nas.id,
        vec![
            domovoi_testkit::build::manifest("restart_container", Risk::Write)
                .unwrap()
                .summary(),
        ],
    );
    let skills = a.get("/api/v1/hosts/nas/skills", &login).await;
    assert_eq!(skills.body[0]["name"], "restart_container");
    assert_eq!(skills.body[0]["timeout_secs"], 60);

    let created = a
        .post(
            "/api/v1/notes",
            &login,
            json!({ "host": "nas", "title": "Backups", "body_markdown": "restic runs at 02:00", "tags": ["backup"] }),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.text);
    let id = created.body["id"].as_str().unwrap().to_owned();
    let replaced = a
        .send(
            Method::PUT,
            &format!("/api/v1/notes/{id}"),
            Some(&login),
            Some(json!({ "host": "nas", "title": "Backups", "body_markdown": "restic runs at 03:00" })),
        )
        .await;
    assert_eq!(replaced.status, StatusCode::OK);
    let hits = a
        .get("/api/v1/notes/search?q=restic&host=nas", &login)
        .await;
    assert_eq!(hits.body[0]["note"]["id"], id.as_str());
    let listed = a.get("/api/v1/notes?host=nas", &login).await;
    assert_eq!(listed.body.as_array().unwrap().len(), 1);
    let deleted = a
        .send(
            Method::DELETE,
            &format!("/api/v1/notes/{id}"),
            Some(&login),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    assert_eq!(
        a.get(&format!("/api/v1/notes/{id}"), &login).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        a.post(
            "/api/v1/notes",
            &login,
            json!({ "title": "", "body_markdown": "" })
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn advisories_digest_and_metrics() {
    let a = api().await;
    let login = a.admin().await;
    assert_eq!(a.get("/api/v1/advisories", &login).await.body, json!([]));
    let preview = a.get("/api/v1/digest/preview", &login).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.text);
    assert!(preview.body["markdown"].as_str().unwrap().contains("nas"));
    assert_eq!(
        a.post("/api/v1/digest/send", &login, json!({}))
            .await
            .status,
        StatusCode::SERVICE_UNAVAILABLE,
        "no notifier configured"
    );
    assert_eq!(
        a.send(Method::GET, "/metrics", None, None).await.text,
        "domovoi_up 1\n"
    );
}

#[test]
fn openapi_snapshot() {
    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    insta::assert_json_snapshot!("openapi", spec);
}
