//! The summary, edit, similar-operations and search endpoints of past operations.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::http::{Method, StatusCode};
use domovoi_core::experience::{
    ExperienceHit, ExperienceQuery, ResolutionBrief, ResolutionSummary, SimilarHit, SimilarQuery,
    SummaryField, SummaryRevision, SummarySource,
};
use domovoi_core::ports::{
    ExperienceRead, HostRepo, OperationWrite, StateWrite, StoreError, SummaryFieldWrite,
    Transaction, UnitOfWork,
};
use domovoi_core::{Actor, CheckKey, Host, OperationId, OperationKind, PinnedSubject, TargetKey};
use domovoi_engine::operations::OperationSeed;
use jiff::SignedDuration;
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, api, api_with_operations};

const LABEL: &str = "historical evidence, not authorization";

fn tests() -> Actor {
    Actor::System("tests".to_owned())
}

async fn open_on(a: &Api, host: &Host, check: &str, target: &str, goal: &str) -> OperationId {
    let seed = OperationSeed {
        kind: OperationKind::Investigate,
        goal: goal.to_owned(),
        pinned: vec![PinnedSubject {
            host: host.id,
            check: CheckKey::parse(check).unwrap(),
            target: Some(TargetKey::parse(target).unwrap()),
        }],
        incidents: Vec::new(),
        parent: None,
        budgets: None,
        first_steps: Vec::new(),
        effects: Vec::new(),
        actor: tests(),
    };
    a.ops().create(seed).await.unwrap()
}

fn revision(
    a: &Api,
    op: OperationId,
    field: SummaryField,
    expected: u32,
    value: Value,
    source: SummarySource,
) -> OperationWrite {
    OperationWrite::SummaryField(SummaryFieldWrite {
        operation: op,
        field,
        expected_revision: expected,
        value,
        source,
        actor: "system:operations".to_owned(),
        at: a.now(),
        tags: (source == SummarySource::Code).then(Vec::new),
    })
}

/// An operation on `host` that ended and has a summary: five code fields and, where given,
/// the model's problem, cause and fix.
async fn finished_on(
    a: &Api,
    host: &Host,
    check: &str,
    target: &str,
    goal: &str,
    narrative: [Option<&str>; 3],
) -> OperationId {
    let op = open_on(a, host, check, target, goal).await;
    a.advance(SignedDuration::from_secs(10));
    a.ops()
        .cancel(op, tests(), "done".to_owned())
        .await
        .unwrap();
    let mut writes = vec![
        revision(
            a,
            op,
            SummaryField::Outcome,
            0,
            json!("succeeded"),
            SummarySource::Code,
        ),
        revision(
            a,
            op,
            SummaryField::Verified,
            0,
            json!(true),
            SummarySource::Code,
        ),
        revision(
            a,
            op,
            SummaryField::Subjects,
            0,
            json!([{ "host": host.id, "check": check, "target": target }]),
            SummarySource::Code,
        ),
        revision(
            a,
            op,
            SummaryField::ContractsExecuted,
            0,
            json!([]),
            SummarySource::Code,
        ),
        revision(
            a,
            op,
            SummaryField::Hosts,
            0,
            json!([host.id]),
            SummarySource::Code,
        ),
    ];
    for (field, text) in [
        SummaryField::Problem,
        SummaryField::Cause,
        SummaryField::Fix,
    ]
    .into_iter()
    .zip(narrative)
    {
        if let Some(text) = text {
            writes.push(revision(a, op, field, 0, json!(text), SummarySource::Model));
        }
    }
    let tx = writes.into_iter().fold(Transaction::new(), |tx, w| {
        tx.write(StateWrite::Operation(w))
    });
    UnitOfWork::commit(&*a.store, tx).await.unwrap();
    a.advance(SignedDuration::from_secs(10));
    op
}

async fn plex(a: &Api, goal: &str) -> OperationId {
    let nas = a.nas.clone();
    finished_on(
        a,
        &nas,
        "container.down",
        "plex",
        goal,
        [
            Some("plex was stopped"),
            Some("the disk filled up"),
            Some("restart plex"),
        ],
    )
    .await
}

async fn media(a: &Api) -> Host {
    let media = domovoi_testkit::build::host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &media).await.unwrap();
    media
}

fn summary_uri(op: OperationId) -> String {
    format!("/api/v1/operations/{op}/summary")
}

async fn edit(
    a: &Api,
    login: &Login,
    op: OperationId,
    field: &str,
    body: Value,
) -> crate::api_harness::Reply {
    a.post(&format!("{}/{field}", summary_uri(op)), login, body)
        .await
}

#[tokio::test]
async fn the_summary_endpoint_reports_not_ready_drafting_and_ready() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let open = open_on(&a, &a.nas.clone(), "container.down", "plex", "open one").await;
    let reply = a.get(&summary_uri(open), &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["state"], "not_ready");
    assert_eq!(reply.body["fields"], json!([]));

    a.ops()
        .cancel(open, tests(), "stop".to_owned())
        .await
        .unwrap();
    let drafting = a.get(&summary_uri(open), &login).await;
    assert_eq!(drafting.body["state"], "drafting");
    assert!(drafting.body["ended_at"].is_string());

    let done = plex(&a, "restart plex").await;
    let ready = a.get(&summary_uri(done), &login).await;
    assert_eq!(ready.body["state"], "ready");
    let unknown = a.get(&summary_uri(OperationId::new()), &login).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_summary_shows_every_field_with_its_source_and_history() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = plex(&a, "restart plex").await;
    let head = a.head_seq().await;
    let reply = a.get(&summary_uri(op), &login).await;
    let fields: Vec<&str> = reply.body["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["field"].as_str().unwrap())
        .collect();
    assert_eq!(
        fields,
        [
            "outcome",
            "verified",
            "subjects",
            "contracts_executed",
            "hosts",
            "problem",
            "cause",
            "fix"
        ]
    );
    for field in reply.body["fields"].as_array().unwrap() {
        let narrative = ["problem", "cause", "fix"].contains(&field["field"].as_str().unwrap());
        assert_eq!(field["editable"], narrative, "{field}");
        assert_eq!(field["source"], if narrative { "model" } else { "code" });
        assert_eq!(field["actor"], "system:operations");
        assert_eq!(field["revision"], 1);
        assert!(field["created_at"].as_str().unwrap().ends_with('Z'));
    }
    assert_eq!(reply.body["history"].as_array().unwrap().len(), 8);
    assert_eq!(reply.body["history_truncated"], false);
    assert!(reply.body["as_of_seq"].as_u64().unwrap() >= head);
    assert_eq!(reply.body["label"], LABEL);
}

#[tokio::test]
async fn editing_a_narrative_field_returns_the_new_revision_and_is_audited() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = plex(&a, "restart plex").await;
    let body = json!({ "value": "the disk was full", "expected_revision": 1 });
    let reply = edit(&a, &login, op, "cause", body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["field"]["source"], "user");
    assert_eq!(reply.body["field"]["revision"], 2);
    assert_eq!(reply.body["unchanged"], false);
    let audit = a.audit_events("operation.summary_edited").await;
    assert_eq!(audit.len(), 1);
    assert!(audit[0].actor.starts_with("user:"));
    assert!(!audit[0].detail.to_string().contains("the disk was full"));

    let again = a.get(&summary_uri(op), &login).await;
    let cause = again.body["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["field"] == "cause")
        .unwrap();
    assert_eq!(cause["value"], "the disk was full");
    assert_eq!(cause["revision"], 2);
    let repeat = json!({ "value": "the disk was full", "expected_revision": 2 });
    let same = edit(&a, &login, op, "cause", repeat).await;
    assert_eq!(same.body["unchanged"], true);
}

#[tokio::test]
async fn a_stale_edit_is_409_and_a_code_owned_edit_is_403_and_an_unknown_field_is_400() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = plex(&a, "restart plex").await;
    let ok = json!({ "value": "first", "expected_revision": 1 });
    assert_eq!(edit(&a, &login, op, "fix", ok).await.status, StatusCode::OK);
    let stale = json!({ "value": "second", "expected_revision": 1 });
    let stale = edit(&a, &login, op, "fix", stale).await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert!(
        stale.body["message"]
            .as_str()
            .unwrap()
            .contains("reload and try again")
    );

    let code = json!({ "value": "x", "expected_revision": 1 });
    let code = edit(&a, &login, op, "outcome", code).await;
    assert_eq!(code.status, StatusCode::FORBIDDEN);
    assert_eq!(
        code.body["message"],
        "this field is filled by Domovoi and cannot be edited"
    );
    let unknown = json!({ "value": "x", "expected_revision": 0 });
    let unknown = edit(&a, &login, op, "colour", unknown).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.body["message"], "that is not a summary field");
    let empty = json!({ "value": "  ", "expected_revision": 2 });
    assert_eq!(
        edit(&a, &login, op, "fix", empty).await.status,
        StatusCode::BAD_REQUEST
    );
    let long = json!({ "value": "x".repeat(1001), "expected_revision": 2 });
    assert_eq!(
        edit(&a, &login, op, "fix", long).await.status,
        StatusCode::BAD_REQUEST
    );
    let open = open_on(&a, &a.nas.clone(), "container.down", "plex", "still open").await;
    let open_edit = json!({ "value": "x", "expected_revision": 0 });
    let open_edit = edit(&a, &login, open, "fix", open_edit).await;
    assert_eq!(open_edit.status, StatusCode::CONFLICT);
    assert_eq!(open_edit.body["message"], "the operation has not finished");
}

#[tokio::test]
async fn editing_needs_a_session_and_the_csrf_guard() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = plex(&a, "restart plex").await;
    let uri = format!("{}/fix", summary_uri(op));
    let body = json!({ "value": "x", "expected_revision": 1 });
    let anonymous = a.send(Method::POST, &uri, None, Some(body.clone())).await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);
    let no_csrf = Login {
        cookie: login.cookie.clone(),
        csrf: String::new(),
    };
    let refused = a.send(Method::POST, &uri, Some(&no_csrf), Some(body)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert!(a.audit_events("operation.summary_edited").await.is_empty());
}

#[tokio::test]
async fn editing_is_503_when_orchestration_is_off_and_reads_are_empty() {
    let a = api().await;
    let login = a.admin().await;
    let uri = format!("{}/fix", summary_uri(OperationId::new()));
    let body = json!({ "value": "x", "expected_revision": 0 });
    let reply = a.post(&uri, &login, body).await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.body["message"], "orchestration is not enabled");

    let search = a
        .get("/api/v1/experience/search?scope=global", &login)
        .await;
    assert_eq!(search.status, StatusCode::OK, "{}", search.text);
    assert_eq!(search.body["items"], json!([]));
    let similar = format!("/api/v1/incidents/{}/similar-operations", a.incident.id());
    let similar = a.get(&similar, &login).await;
    assert_eq!(similar.status, StatusCode::OK);
    assert_eq!(similar.body["items"], json!([]));
}

#[tokio::test]
async fn similar_operations_of_an_operation_exclude_itself_and_stay_on_its_host() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let other_host = media(&a).await;
    let past = plex(&a, "restart plex after the disk filled").await;
    finished_on(
        &a,
        &other_host,
        "container.down",
        "plex",
        "restart plex on media",
        [Some("plex was stopped"), None, None],
    )
    .await;
    let now = plex(&a, "restart plex again").await;
    let reply = a
        .get(&format!("/api/v1/operations/{now}/similar"), &login)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let items = reply.body["items"].as_array().unwrap();
    let ids: Vec<&str> = items
        .iter()
        .map(|i| i["operation"]["operation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [past.to_string().as_str()]);
    let codes: Vec<&str> = items[0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"same_check") && codes.contains(&"same_target"));
    assert!(items[0]["score"].as_u64().unwrap() >= 12);
    assert_eq!(reply.body["label"], LABEL);
}

#[tokio::test]
async fn similar_operations_of_an_incident_use_its_check_target_and_title() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let nas = a.nas.clone();
    let fixed = finished_on(
        &a,
        &nas,
        "disk.usage",
        "/srv",
        "free space on /srv",
        [Some("the disk was full"), None, None],
    )
    .await;
    plex(&a, "restart plex").await;
    let uri = format!("/api/v1/incidents/{}/similar-operations", a.incident.id());
    let reply = a.get(&uri, &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let ids: Vec<&str> = reply.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["operation"]["operation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.first().copied(), Some(fixed.to_string().as_str()));
    let reasons = reply.body["items"][0]["reasons"].to_string();
    assert!(reasons.contains("same_check") && reasons.contains("same_target"));
    let missing = a
        .get(
            &format!(
                "/api/v1/incidents/{}/similar-operations",
                domovoi_core::IncidentId::new()
            ),
            &login,
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn owner_search_is_host_scoped_by_default() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let other_host = media(&a).await;
    let on_nas = plex(&a, "restart plex").await;
    finished_on(
        &a,
        &other_host,
        "container.down",
        "plex",
        "restart plex on media",
        [None; 3],
    )
    .await;

    let missing = a.get("/api/v1/experience/search?q=plex", &login).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        missing.body["message"],
        "choose a host, or ask for a global search"
    );
    let reply = a
        .get("/api/v1/experience/search?q=plex&host=nas", &login)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["scope"], "host");
    assert_eq!(reply.body["host"]["name"], "nas");
    let ids: Vec<&str> = reply.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["operation"]["operation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [on_nas.to_string().as_str()]);
    assert_eq!(reply.body["items"][0]["match_pass"], "all_words");
    let unknown = a
        .get("/api/v1/experience/search?q=plex&host=nowhere", &login)
        .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    let long = format!("/api/v1/experience/search?host=nas&q={}", "a".repeat(201));
    assert_eq!(a.get(&long, &login).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn global_search_needs_scope_global_and_no_host() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let other_host = media(&a).await;
    plex(&a, "restart plex").await;
    finished_on(
        &a,
        &other_host,
        "container.down",
        "plex",
        "restart plex on media",
        [None; 3],
    )
    .await;

    let reply = a
        .get("/api/v1/experience/search?q=plex&scope=global", &login)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["scope"], "global");
    assert!(reply.body["host"].is_null());
    let items = reply.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let names: Vec<&str> = items
        .iter()
        .flat_map(|i| i["operation"]["hosts"].as_array().unwrap())
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"nas") && names.contains(&"media"));
    let both = a
        .get("/api/v1/experience/search?scope=global&host=nas", &login)
        .await;
    assert_eq!(both.status, StatusCode::BAD_REQUEST);
    let odd = a
        .get("/api/v1/experience/search?scope=everywhere", &login)
        .await;
    assert_eq!(odd.status, StatusCode::BAD_REQUEST);
    let newest = a
        .get("/api/v1/experience/search?scope=global", &login)
        .await;
    assert_eq!(newest.body["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn every_past_operation_response_carries_the_label() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let op = plex(&a, "restart plex").await;
    let now = plex(&a, "restart plex again").await;
    for uri in [
        summary_uri(op),
        format!("/api/v1/operations/{now}/similar"),
        format!("/api/v1/incidents/{}/similar-operations", a.incident.id()),
        "/api/v1/experience/search?host=nas".to_owned(),
        "/api/v1/experience/search?scope=global".to_owned(),
    ] {
        let reply = a.get(&uri, &login).await;
        assert_eq!(reply.status, StatusCode::OK, "{uri}: {}", reply.text);
        assert_eq!(reply.body["label"], LABEL, "{uri}");
    }
}

/// A store whose brief read always fails; every other read goes to the real store.
struct NoBriefs(Arc<domovoi_store::Store>);

#[async_trait]
impl ExperienceRead for NoBriefs {
    async fn summary(&self, op: OperationId) -> Result<Option<ResolutionSummary>, StoreError> {
        self.0.summary(op).await
    }
    async fn revisions(&self, op: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        self.0.revisions(op).await
    }
    async fn current(&self, op: OperationId) -> Result<Vec<SummaryRevision>, StoreError> {
        self.0.current(op).await
    }
    async fn briefs(
        &self,
        _: &[OperationId],
    ) -> Result<BTreeMap<OperationId, ResolutionBrief>, StoreError> {
        Err(StoreError::Backend("briefs are down".to_owned()))
    }
    async fn search(&self, q: &ExperienceQuery) -> Result<Vec<ExperienceHit>, StoreError> {
        self.0.search(q).await
    }
    async fn similar(&self, q: &SimilarQuery) -> Result<Vec<SimilarHit>, StoreError> {
        self.0.similar(q).await
    }
    async fn undrafted(&self, limit: u16) -> Result<Vec<OperationId>, StoreError> {
        self.0.undrafted(limit).await
    }
}

#[tokio::test]
async fn operation_list_rows_carry_the_resolution_brief_and_survive_a_failed_read() {
    let a = api_with_operations().await;
    let login = a.admin().await;
    let done = plex(&a, "restart plex").await;
    let open = open_on(&a, &a.nas.clone(), "container.down", "plex", "still open").await;

    let list = a.get("/api/v1/operations", &login).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text);
    let row = |id: OperationId| {
        list.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id.to_string())
            .unwrap()
            .clone()
    };
    assert_eq!(row(done)["resolution"]["outcome"], "succeeded");
    assert_eq!(row(done)["resolution"]["verified"], true);
    assert!(row(done)["resolution"]["ended_at"].is_string());
    assert!(row(open)["resolution"].is_null());
    let detail = a.get(&format!("/api/v1/operations/{done}"), &login).await;
    assert_eq!(
        detail.body["operation"]["resolution"]["outcome"],
        "succeeded"
    );

    let broken = a.with_experience(Arc::new(NoBriefs(a.store.clone())));
    let degraded = a
        .send_to(
            broken,
            Method::GET,
            "/api/v1/operations",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(degraded.status, StatusCode::OK, "{}", degraded.text);
    assert_eq!(degraded.body["items"].as_array().unwrap().len(), 2);
    for item in degraded.body["items"].as_array().unwrap() {
        assert!(item["resolution"].is_null());
    }
}

#[test]
fn the_openapi_document_lists_the_new_routes() {
    let spec: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    let ids = |path: &str, method: &str| spec["paths"][path][method]["operationId"].clone();
    assert_eq!(
        ids("/api/v1/operations/{id}/summary", "get"),
        "get_operation_summary"
    );
    assert_eq!(
        ids("/api/v1/operations/{id}/summary/{field}", "post"),
        "edit_operation_summary_field"
    );
    assert_eq!(
        ids("/api/v1/operations/{id}/similar", "get"),
        "get_similar_operations"
    );
    assert_eq!(
        ids("/api/v1/incidents/{id}/similar-operations", "get"),
        "get_similar_operations_for_incident"
    );
    assert_eq!(
        ids("/api/v1/experience/search", "get"),
        "search_experience_owner"
    );
}
