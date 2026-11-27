//! The knowledge API: facts, management paths, relations and entities.

use axum::http::{Method, StatusCode};
use domovoi_core::Actor;
use domovoi_core::knowledge::{FactSource, Predicate, SubjectRef};
use domovoi_engine::knowledge::{FactDraft, KnowledgeDeps, KnowledgeService};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, PASSWORD, api};

fn host_ref(a: &Api) -> String {
    format!("host:{}", a.nas.id)
}

async fn assert_fact(a: &Api, login: &Login, body: Value) -> crate::api_harness::Reply {
    a.post("/api/v1/knowledge/facts", login, body).await
}

fn role(a: &Api, value: &str) -> Value {
    json!({ "subject": host_ref(a), "predicate": "role", "value": value })
}

fn path_body(a: &Api, cidr: &str, password: Option<&str>) -> Value {
    json!({
        "subject": host_ref(a), "predicate": "management_path",
        "value": { "cidr": cidr }, "password": password,
    })
}

/// A service over the harness store, to write facts as a system component would.
fn system_service(a: &Api) -> KnowledgeService {
    KnowledgeService::new(KnowledgeDeps {
        uow: a.store.clone(),
        read: a.store.clone(),
        coverage: a.coverage.clone(),
        clock: a.clock.clone(),
    })
}

#[tokio::test]
async fn knowledge_endpoints_require_a_session() {
    let a = api().await;
    let id = "0190a4a0-0000-7000-8000-000000000000";
    let routes = [
        (Method::GET, "/api/v1/knowledge/facts".to_owned()),
        (Method::GET, format!("/api/v1/knowledge/facts/{id}")),
        (Method::POST, "/api/v1/knowledge/facts".to_owned()),
        (Method::POST, format!("/api/v1/knowledge/facts/{id}/accept")),
        (
            Method::POST,
            format!("/api/v1/knowledge/facts/{id}/retract"),
        ),
        (Method::POST, format!("/api/v1/knowledge/facts/{id}/verify")),
        (
            Method::POST,
            format!("/api/v1/knowledge/facts/{id}/confirm"),
        ),
        (
            Method::GET,
            "/api/v1/knowledge/hosts/nas/management-paths".to_owned(),
        ),
        (Method::GET, "/api/v1/knowledge/relations".to_owned()),
        (Method::POST, "/api/v1/knowledge/relations".to_owned()),
        (
            Method::POST,
            format!("/api/v1/knowledge/relations/{id}/retract"),
        ),
        (Method::GET, "/api/v1/knowledge/entities".to_owned()),
        (Method::POST, "/api/v1/knowledge/entities".to_owned()),
        (Method::PUT, format!("/api/v1/knowledge/entities/{id}")),
        (
            Method::DELETE,
            format!("/api/v1/knowledge/entities/{id}?expected_version=1"),
        ),
    ];
    for (method, uri) in routes {
        let reply = a.send(method.clone(), &uri, None, Some(json!({}))).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn owner_asserts_lists_and_retracts_a_fact() {
    let a = api().await;
    let login = a.admin().await;
    let reply = assert_fact(&a, &login, role(&a, "media server")).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.text);
    assert_eq!(reply.body["source"], "user");
    assert!(reply.body["actor"].as_str().unwrap().starts_with("user:"));
    let id = reply.body["id"].as_str().unwrap().to_owned();

    let list = a.get("/api/v1/knowledge/facts?host=nas", &login).await;
    assert_eq!(list.body["items"].as_array().unwrap().len(), 1);

    let version = reply.body["version"].clone();
    let retract = a
        .post(
            &format!("/api/v1/knowledge/facts/{id}/retract"),
            &login,
            json!({ "expected_version": version }),
        )
        .await;
    assert_eq!(retract.status, StatusCode::OK, "{}", retract.text);
    assert_eq!(retract.body["status"], "retracted");

    let list = a.get("/api/v1/knowledge/facts?host=nas", &login).await;
    assert!(list.body["items"].as_array().unwrap().is_empty());
    let list = a
        .get("/api/v1/knowledge/facts?host=nas&status=retracted", &login)
        .await;
    assert_eq!(list.body["items"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_api_refuses_a_smuggled_source_or_actor() {
    let a = api().await;
    let login = a.admin().await;
    for extra in ["source", "actor"] {
        let mut body = role(&a, "media server");
        body[extra] = json!("inferred");
        let reply = assert_fact(&a, &login, body).await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{extra}: {}",
            reply.text
        );
    }
    let list = a.get("/api/v1/knowledge/facts", &login).await;
    assert!(list.body["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn list_filters_by_host_predicate_status_and_source() {
    let a = api().await;
    let login = a.admin().await;
    for i in 0..3 {
        let body = json!({
            "subject": host_ref(&a), "predicate": "owner_note", "value": format!("note {i}"),
        });
        assert_eq!(
            assert_fact(&a, &login, body).await.status,
            StatusCode::CREATED
        );
    }
    assert_eq!(
        assert_fact(&a, &login, role(&a, "nas")).await.status,
        StatusCode::CREATED
    );
    let system = system_service(&a);
    system
        .record(
            FactDraft {
                subject: SubjectRef::Host(a.nas.id),
                predicate: Predicate::BackupLocation,
                value: json!("offsite"),
                source: FactSource::Discovered,
                observed_at: None,
                valid_until: None,
                evidence: None,
            },
            &Actor::System("profile".into()),
        )
        .await
        .unwrap();

    let by = |q: &str| {
        let login = &login;
        let a = &a;
        let uri = format!("/api/v1/knowledge/facts?{q}");
        async move { a.get(&uri, login).await.body }
    };
    assert_eq!(
        by("predicate=owner_note").await["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        by("source=discovered").await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        by("status=current&host=nas").await["items"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        by("status=stale").await["items"].as_array().unwrap().len(),
        0
    );

    let first = by("predicate=owner_note&limit=2").await;
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let second = by(&format!("predicate=owner_note&limit=2&cursor={cursor}")).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());

    for bad in [
        "predicate=nope",
        "source=nope",
        "status=nope",
        "subject=nope",
    ] {
        let reply = a
            .get(&format!("/api/v1/knowledge/facts?{bad}"), &login)
            .await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let reply = a.get("/api/v1/knowledge/facts?host=ghost", &login).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_finds_live_facts_and_never_retracted_ones() {
    let a = api().await;
    let login = a.admin().await;
    let kept = assert_fact(&a, &login, role(&a, "media server")).await;
    assert_eq!(kept.status, StatusCode::CREATED);
    let note = json!({
        "subject": host_ref(&a), "predicate": "owner_note", "value": "old media box",
    });
    let gone = assert_fact(&a, &login, note).await;
    let id = gone.body["id"].as_str().unwrap();
    let retract = a
        .post(
            &format!("/api/v1/knowledge/facts/{id}/retract"),
            &login,
            json!({ "expected_version": gone.body["version"] }),
        )
        .await;
    assert_eq!(retract.status, StatusCode::OK);

    let hits = a.get("/api/v1/knowledge/facts?q=media", &login).await;
    assert_eq!(hits.status, StatusCode::OK, "{}", hits.text);
    let items = hits.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["predicate"], "role");
    assert!(hits.body["next_cursor"].is_null());
}

#[tokio::test]
async fn safety_critical_fact_writes_need_the_password() {
    let a = api().await;
    let login = a.admin().await;
    let none = assert_fact(&a, &login, path_body(&a, "192.168.1.0/24", None)).await;
    assert_eq!(none.status, StatusCode::FORBIDDEN, "{}", none.text);
    let wrong = assert_fact(&a, &login, path_body(&a, "192.168.1.0/24", Some("nope"))).await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let listed = a.get("/api/v1/knowledge/facts", &login).await;
    assert!(listed.body["items"].as_array().unwrap().is_empty());

    let ok = assert_fact(&a, &login, path_body(&a, "192.168.1.0/24", Some(PASSWORD))).await;
    assert_eq!(ok.status, StatusCode::CREATED, "{}", ok.text);
    // An ordinary predicate needs none.
    assert_eq!(
        assert_fact(&a, &login, role(&a, "nas")).await.status,
        StatusCode::CREATED
    );
    // Retracting a path only removes trust, so it needs none either.
    let id = ok.body["id"].as_str().unwrap();
    let retract = a
        .post(
            &format!("/api/v1/knowledge/facts/{id}/retract"),
            &login,
            json!({ "expected_version": ok.body["version"] }),
        )
        .await;
    assert_eq!(retract.status, StatusCode::OK, "{}", retract.text);
}

#[tokio::test]
async fn confirm_needs_the_password_and_verified_coverage() {
    let a = api().await;
    let login = a.admin().await;
    let fact = assert_fact(&a, &login, path_body(&a, "192.168.1.0/24", Some(PASSWORD))).await;
    let id = fact.body["id"].as_str().unwrap().to_owned();
    let uri = format!("/api/v1/knowledge/facts/{id}/confirm");
    let version = fact.body["version"].clone();

    let blocked = a
        .post(
            &uri,
            &login,
            json!({ "expected_version": version, "password": PASSWORD }),
        )
        .await;
    assert_eq!(blocked.status, StatusCode::CONFLICT, "{}", blocked.text);
    assert!(
        blocked.body["message"]
            .as_str()
            .unwrap()
            .contains("cannot be verified")
    );
    let paths = a
        .get("/api/v1/knowledge/hosts/nas/management-paths", &login)
        .await;
    assert_eq!(paths.body["coverage_available"], false);
    assert_eq!(paths.body["paths"][0]["state"], "unconfirmed");

    // 38f: the row keeps saying why, for every later reader of the list.
    let refusal = &paths.body["paths"][0]["last_refusal"];
    assert_eq!(refusal["reason"], blocked.body["message"], "{}", paths.text);
    assert!(refusal["at"].as_str().is_some());

    a.coverage.cover(&["192.168.1.0/24"]);
    let wrong = a
        .post(
            &uri,
            &login,
            json!({ "expected_version": version, "password": "nope" }),
        )
        .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let missing = a
        .post(&uri, &login, json!({ "expected_version": version }))
        .await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN);
    let paths = a
        .get("/api/v1/knowledge/hosts/nas/management-paths", &login)
        .await;
    assert_eq!(paths.body["paths"][0]["state"], "unconfirmed");

    let done = a
        .post(
            &uri,
            &login,
            json!({ "expected_version": version, "password": PASSWORD }),
        )
        .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.text);
    assert_eq!(done.body["state"], "confirmed");
    assert!(done.body["confirmation"]["confirmed_by"].as_str().is_some());
    assert!(done.body["confirmation"].get("evidence").is_none());
    assert!(!done.text.contains("fake-evidence"));
    let paths = a
        .get("/api/v1/knowledge/hosts/nas/management-paths", &login)
        .await;
    assert_eq!(paths.body["paths"][0]["state"], "confirmed");
    assert!(paths.body["paths"][0]["last_refusal"].is_null());
}

#[tokio::test]
async fn confirming_a_fact_that_is_not_a_management_path_says_so_without_asking_for_a_password() {
    let a = api().await;
    let login = a.admin().await;
    let fact = assert_fact(&a, &login, role(&a, "media server")).await;
    let id = fact.body["id"].as_str().unwrap().to_owned();

    // No password sent: the answer is "not a management path", not "enter your password".
    let reply = a
        .post(
            &format!("/api/v1/knowledge/facts/{id}/confirm"),
            &login,
            json!({ "expected_version": fact.body["version"] }),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply.body["message"]
            .as_str()
            .unwrap()
            .contains("only a management path"),
        "{}",
        reply.text
    );
}

#[tokio::test]
async fn a_discovered_path_must_be_accepted_before_it_can_be_confirmed() {
    let a = api().await;
    let login = a.admin().await;
    a.coverage.cover(&["10.0.0.0/8"]);
    let recorded = system_service(&a)
        .record(
            FactDraft {
                subject: SubjectRef::Host(a.nas.id),
                predicate: Predicate::ManagementPath,
                value: json!({ "cidr": "10.0.0.0/8" }),
                source: FactSource::Discovered,
                observed_at: None,
                valid_until: None,
                evidence: None,
            },
            &Actor::System("profile".into()),
        )
        .await
        .unwrap();
    let id = recorded.fact.value.id.to_string();
    let confirm = format!("/api/v1/knowledge/facts/{id}/confirm");
    let early = a
        .post(
            &confirm,
            &login,
            json!({ "expected_version": recorded.fact.version, "password": PASSWORD }),
        )
        .await;
    assert_eq!(early.status, StatusCode::CONFLICT, "{}", early.text);
    assert!(early.body["message"].as_str().unwrap().contains("accept"));

    let accepted = a
        .post(
            &format!("/api/v1/knowledge/facts/{id}/accept"),
            &login,
            json!({ "expected_version": recorded.fact.version, "password": PASSWORD }),
        )
        .await;
    assert_eq!(accepted.status, StatusCode::OK, "{}", accepted.text);
    assert_eq!(accepted.body["source"], "user");
    let new_id = accepted.body["id"].as_str().unwrap();
    let done = a
        .post(
            &format!("/api/v1/knowledge/facts/{new_id}/confirm"),
            &login,
            json!({ "expected_version": accepted.body["version"], "password": PASSWORD }),
        )
        .await;
    assert_eq!(done.status, StatusCode::OK, "{}", done.text);
}

#[tokio::test]
async fn a_stale_version_is_a_409() {
    let a = api().await;
    let login = a.admin().await;
    let fact = assert_fact(&a, &login, role(&a, "nas")).await;
    let id = fact.body["id"].as_str().unwrap();
    let uri = format!("/api/v1/knowledge/facts/{id}/verify");
    let body = json!({ "expected_version": fact.body["version"] });
    assert_eq!(
        a.post(&uri, &login, body.clone()).await.status,
        StatusCode::OK
    );
    let again = a.post(&uri, &login, body).await;
    assert_eq!(again.status, StatusCode::CONFLICT, "{}", again.text);
    let unknown = a
        .post(
            "/api/v1/knowledge/facts/not-an-id/verify",
            &login,
            json!({ "expected_version": 1 }),
        )
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn audit_entries_name_the_fact_but_never_its_value() {
    let a = api().await;
    let login = a.admin().await;
    let reply = assert_fact(&a, &login, role(&a, "a very distinctive value")).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    let events = a.audit_events("knowledge.fact.asserted").await;
    assert_eq!(events.len(), 1);
    let text = format!("{:?}", events[0]);
    assert!(text.contains("role"), "{text}");
    assert!(text.contains(&host_ref(&a)), "{text}");
    assert!(!text.contains("distinctive"), "{text}");
}

#[tokio::test]
async fn relations_and_entities_round_trip() {
    let a = api().await;
    let login = a.admin().await;
    let entity = a
        .post(
            "/api/v1/knowledge/entities",
            &login,
            json!({ "kind": "vpn", "name": "Home VPN" }),
        )
        .await;
    assert_eq!(entity.status, StatusCode::CREATED, "{}", entity.text);
    let entity_id = entity.body["id"].as_str().unwrap().to_owned();
    let dup = a
        .post(
            "/api/v1/knowledge/entities",
            &login,
            json!({ "kind": "vpn", "name": "home vpn" }),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let relation = a
        .post(
            "/api/v1/knowledge/relations",
            &login,
            json!({ "from": host_ref(&a), "kind": "managed_from", "to": format!("entity:{entity_id}") }),
        )
        .await;
    assert_eq!(relation.status, StatusCode::CREATED, "{}", relation.text);
    assert_eq!(relation.body["source"], "user");
    let relation_id = relation.body["id"].as_str().unwrap().to_owned();
    let listed = a.get("/api/v1/knowledge/relations", &login).await;
    assert_eq!(listed.body["items"].as_array().unwrap().len(), 1);

    let in_use = a
        .send(
            Method::DELETE,
            &format!("/api/v1/knowledge/entities/{entity_id}?expected_version=1"),
            Some(&login),
            None,
        )
        .await;
    assert_eq!(in_use.status, StatusCode::CONFLICT, "{}", in_use.text);

    let renamed = a
        .send(
            Method::PUT,
            &format!("/api/v1/knowledge/entities/{entity_id}"),
            Some(&login),
            Some(json!({ "name": "Home VPN 2", "expected_version": 1 })),
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.text);
    assert_eq!(renamed.body["version"], 2);

    let retract = a
        .post(
            &format!("/api/v1/knowledge/relations/{relation_id}/retract"),
            &login,
            json!({ "expected_version": relation.body["version"] }),
        )
        .await;
    assert_eq!(retract.status, StatusCode::OK, "{}", retract.text);
    let listed = a.get("/api/v1/knowledge/relations", &login).await;
    assert!(listed.body["items"].as_array().unwrap().is_empty());

    let deleted = a
        .send(
            Method::DELETE,
            &format!("/api/v1/knowledge/entities/{entity_id}?expected_version=2"),
            Some(&login),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.text);
    let entities = a.get("/api/v1/knowledge/entities", &login).await;
    assert!(entities.body.as_array().unwrap().is_empty());
}

#[test]
fn the_openapi_document_lists_every_knowledge_route() {
    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    let mut found = Vec::new();
    for (path, item) in spec["paths"].as_object().unwrap() {
        if path.starts_with("/api/v1/knowledge") {
            for operation in item.as_object().unwrap().values() {
                found.push(operation["operationId"].as_str().unwrap().to_owned());
            }
        }
    }
    for id in [
        "list_facts",
        "get_fact",
        "assert_fact",
        "accept_fact",
        "retract_fact",
        "verify_fact",
        "list_management_paths",
        "confirm_management_path",
        "list_relations",
        "assert_relation",
        "retract_relation",
        "list_entities",
        "create_entity",
        "rename_entity",
        "delete_entity",
    ] {
        assert!(found.iter().any(|f| f == id), "missing {id}: {found:?}");
    }
}

#[tokio::test]
async fn a_relation_to_something_that_does_not_exist_is_refused() {
    let a = api().await;
    let login = a.admin().await;
    for (from, to, what) in [
        (
            host_ref(&a),
            "entity:01a2191a-0000-7000-8000-000000000000".to_owned(),
            "entity",
        ),
        (
            "host:01a2191a-0000-7000-8000-000000000001".to_owned(),
            host_ref(&a),
            "host",
        ),
    ] {
        let reply = a
            .post(
                "/api/v1/knowledge/relations",
                &login,
                json!({ "from": from, "kind": "managed_from", "to": to }),
            )
            .await;
        assert_eq!(
            reply.status,
            StatusCode::BAD_REQUEST,
            "{what}: {}",
            reply.text
        );
        assert!(reply.text.contains("does not exist"), "{}", reply.text);
    }
}

/// 38c: a value that is no network is refused before any password is asked for or checked.
#[tokio::test]
async fn an_invalid_network_is_refused_before_the_password() {
    let a = api().await;
    let login = a.admin().await;
    for password in [None, Some("nope")] {
        let reply = assert_fact(&a, &login, path_body(&a, "not a network", password)).await;
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    }
}
