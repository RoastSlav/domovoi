use std::sync::Arc;

use axum::http::{Method, StatusCode};
use domovoi_core::Risk;
use domovoi_core::ports::{AuditFilter, AuditLog, ChatError, IncidentRepo, Page};
use domovoi_testkit::RecordingNotifier;
use domovoi_testkit::build::{open_incident, resolved_incident};
use futures::StreamExt;
use serde_json::{Value, json};

use crate::api_harness::{Login, api, api_with_notifier, api_with_triage};

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
    assert_eq!(
        detail.body["card"]["badge"],
        a.incident.severity().to_string(),
        "before triage the badge is the severity"
    );
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
        let csp = h["content-security-policy"].to_str().unwrap();
        assert!(csp.starts_with(domovoi_hub::csp::BASE_POLICY), "{csp}");
        assert!(csp.contains("script-src 'self'"), "{csp}");
        assert!(!csp.contains("unsafe-eval"), "{csp}");
        let scripts = csp.split("script-src").nth(1).unwrap();
        assert!(!scripts.contains("unsafe-inline"), "{csp}");
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["referrer-policy"], "same-origin");
        assert_eq!(h["permissions-policy"], "interest-cohort=()");
    }
}

#[tokio::test]
async fn the_inline_scripts_of_the_served_page_match_the_policy() {
    let a = api().await;
    let page = a.send(Method::GET, "/incidents", None, None).await;
    let csp = page.headers["content-security-policy"].to_str().unwrap();
    let hashes = domovoi_hub::csp::inline_script_hashes(&page.text);
    // The built page carries the theme script; without the UI there is nothing to allow.
    #[cfg(feature = "embed-ui")]
    assert_eq!(hashes.len(), 1, "{csp}");
    for hash in hashes {
        assert!(csp.contains(&hash), "{hash} missing from {csp}");
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

    let restart = domovoi_testkit::build::manifest("restart_container", Risk::Write).unwrap();
    let mut restart_summary = restart.summary();
    restart_summary.readme = Some("# Restart container\n\nRestarts one container.".to_string());
    a.link.set_skills(a.nas.id, vec![restart_summary]);
    let skills = a.get("/api/v1/hosts/nas/skills", &login).await;
    assert_eq!(skills.body[0]["name"], "restart_container");
    assert_eq!(skills.body[0]["timeout_secs"], 60);
    assert_eq!(
        skills.body[0]["manifest_hash"],
        restart.manifest_hash().to_string()
    );
    assert_eq!(
        skills.body[0]["readme"],
        "# Restart container\n\nRestarts one container."
    );
    assert!(skills.body[0]["verify"].is_null());
    assert_eq!(skills.body[0]["enabled"], true);

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
async fn host_logs_are_served_or_the_host_is_unknown() {
    let a = api().await;
    let login = a.admin().await;
    let logs = a
        .get("/api/v1/hosts/nas/logs?lines=2&level=warn", &login)
        .await;
    assert_eq!(logs.status, StatusCode::OK, "{}", logs.text);
    assert_eq!(logs.body.as_array().unwrap().len(), 0);
    assert_eq!(
        a.get("/api/v1/hosts/nowhere/logs", &login).await.status,
        StatusCode::NOT_FOUND
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

#[tokio::test]
async fn digest_send_tells_a_refused_send_from_a_missing_notifier() {
    let notifier = Arc::new(RecordingNotifier::new("discord-gateway"));
    let a = api_with_notifier(notifier.clone()).await;
    let login = a.admin().await;

    notifier.fail_next(ChatError::Rejected(
        "discord 403: Missing Access".to_owned(),
    ));
    let refused = a.post("/api/v1/digest/send", &login, json!({})).await;
    assert_eq!(refused.status, StatusCode::BAD_GATEWAY, "{}", refused.text);
    assert_eq!(refused.body["error"], "delivery_failed");
    assert!(
        refused.body["message"]
            .as_str()
            .unwrap()
            .contains("Missing Access")
    );

    let sent = a.post("/api/v1/digest/send", &login, json!({})).await;
    assert_eq!(sent.status, StatusCode::OK, "{}", sent.text);
    assert_eq!(notifier.sent().len(), 1);
}

#[test]
fn openapi_snapshot() {
    let mut spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    spec["info"]["version"] = json!("0.0.0");
    insta::assert_json_snapshot!("openapi", spec);
}

#[tokio::test]
async fn event_stream_ends_when_the_server_shuts_down() {
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    let a = api().await;
    let login = a.admin().await;
    let shutdown = tokio_util::sync::CancellationToken::new();
    let app = domovoi_hub::router::router(a.state.clone().with_shutdown(shutdown.clone()), None)
        .layer(MockConnectInfo(crate::api_harness::peer()));
    let response = app
        .oneshot(
            Request::get("/api/v1/events")
                .header(header::COOKIE, &login.cookie)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(
        futures::FutureExt::now_or_never(body.next()).is_none(),
        "the stream stays open while the hub runs"
    );

    shutdown.cancel();
    // Ends instead of hanging the test: graceful shutdown no longer waits on open tabs.
    while body.next().await.is_some() {}
}

/// A report the scripted model gives triage.
const TRIAGE_REPORT: &str = r#"{
  "headline": "disk filling",
  "summary": "/srv grew by 8 GB in an hour.",
  "cause": "Unrotated logs.",
  "evidence": [],
  "verdict": "ACTION_NEEDED",
  "next_step": "Prune the logs.",
  "proposals": [],
  "details": "The logs under /srv are never rotated."
}"#;

#[tokio::test]
async fn retrying_triage_runs_the_model_again_and_reports_how_it_went() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    a.llm.push_text(TRIAGE_REPORT);

    let reply = a
        .post(
            &format!("/api/v1/incidents/{}/triage", a.incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["kind"], "report", "{}", reply.text);
    assert_eq!(a.llm.requests().len(), 1, "the model ran once");

    let detail = a
        .get(&format!("/api/v1/incidents/{}", a.incident.id()), &login)
        .await;
    assert_eq!(detail.body["triage"]["kind"], "report");
    assert_eq!(detail.body["report"]["headline"], "disk filling");
}

#[tokio::test]
async fn retrying_triage_without_a_model_is_503_and_unknown_incidents_are_404() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .post(
            &format!("/api/v1/incidents/{}/triage", a.incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        reply.text
    );

    let triaged = api_with_triage().await;
    let login = triaged.admin().await;
    let reply = triaged
        .post("/api/v1/incidents/not-an-id/triage", &login, json!({}))
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);
}

#[tokio::test]
async fn follow_up_history_is_served_and_clear_empties_it() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    a.llm.push_text("Because the config file is bad.");
    let first = a
        .post(
            &format!("/api/v1/incidents/{}/ask", a.incident.id()),
            &login,
            json!({ "question": "Why does nginx crash?" }),
        )
        .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);
    a.llm.push_text("Yes, it restarted.");
    let second = a
        .post(
            &format!("/api/v1/incidents/{}/ask", a.incident.id()),
            &login,
            json!({ "question": "Did the restart help?" }),
        )
        .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.text);

    let history = a
        .get(
            &format!("/api/v1/incidents/{}/followups", a.incident.id()),
            &login,
        )
        .await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.text);
    let turns = history.body["turns"].as_array().unwrap();
    assert_eq!(
        turns.len(),
        2,
        "both questions are remembered: {}",
        history.text
    );
    assert_eq!(turns[0]["question"], "Why does nginx crash?");
    assert_eq!(turns[0]["answer"], "Because the config file is bad.");
    assert!(
        turns[1]["asked_by"].as_str().unwrap().starts_with("user:"),
        "the asker is recorded",
    );

    let cleared = a
        .post(
            &format!("/api/v1/incidents/{}/followups/clear", a.incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::NO_CONTENT, "{}", cleared.text);
    let after = a
        .get(
            &format!("/api/v1/incidents/{}/followups", a.incident.id()),
            &login,
        )
        .await;
    assert_eq!(after.status, StatusCode::OK, "{}", after.text);
    assert_eq!(
        after.body["turns"].as_array().unwrap().len(),
        0,
        "after the clear nothing is remembered: {}",
        after.text,
    );

    let off = api().await;
    let login = off.admin().await;
    let unknown = off
        .get(
            &format!("/api/v1/incidents/{}/followups", off.incident.id()),
            &login,
        )
        .await;
    assert_eq!(
        unknown.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "with triage off there is no conversation to read: {}",
        unknown.text,
    );
    let cleared = off
        .post(
            &format!("/api/v1/incidents/{}/followups/clear", off.incident.id()),
            &login,
            json!({}),
        )
        .await;
    assert_eq!(
        cleared.status,
        StatusCode::NO_CONTENT,
        "a clear works even with triage off: {}",
        cleared.text,
    );
}

#[tokio::test]
async fn follow_up_history_for_an_unknown_incident_is_not_found() {
    let a = api_with_triage().await;
    let login = a.admin().await;
    let unknown = domovoi_core::IncidentId::new();
    let missing = a
        .get(&format!("/api/v1/incidents/{}/followups", unknown), &login)
        .await;
    assert_eq!(
        missing.status,
        StatusCode::NOT_FOUND,
        "an id that parses but was never stored is not a cleared conversation: {}",
        missing.text,
    );
}

#[tokio::test]
async fn digest_preview_says_when_the_next_send_is_due() {
    let a = api().await;
    let login = a.admin().await;
    let preview = a.get("/api/v1/digest/preview", &login).await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.text);
    // The clock is 12:00 UTC and the digest goes out at 09:00 UTC, so the next send is
    // tomorrow morning.
    assert_eq!(preview.body["next_send_at"], "2026-11-01T09:00:00Z");
}
