//! The policy API: drafts, previews, publication with step-up, recorded decisions and the
//! simulator. Nothing here enforces anything; these tests check what is stored, audited and
//! answered.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Method, StatusCode};
use domovoi_core::policy_store::{ShadowFilter, ShadowStatus};
use domovoi_core::ports::{
    AuditFilter, CommitError, CommitReceipt, JournalRead, Page, PolicyRepo, Transaction, UnitOfWork,
};
use domovoi_engine::policy::Rule;
use domovoi_hub::api::dto_policy_rule::RuleDto;
use domovoi_hub::api::{ApiDeps, ApiState};
use domovoi_hub::router::router;
use domovoi_hub::services::{PolicyDeps, PolicyService};
use serde_json::{Value, json};

use crate::api_harness::{Api, Login, PASSWORD, api, peer};
use crate::policy_env::{CONTRACT, record_step};

const PLEX_ALLOWS_NAS_DENIES: &str = "incomparable_conflict:allow-plex:deny-nas";

fn rule(id: &str, outcome: &str) -> Value {
    json!({
        "id": id, "enabled": true, "shadow": false, "locked": false,
        "host": { "kind": "all" }, "capability": { "kind": "any" },
        "targets": { "kind": "any" }, "conditions": [], "outcome": outcome, "note": "",
    })
}

fn plex(id: &str, outcome: &str) -> Value {
    let mut value = rule(id, outcome);
    value["capability"] = json!({ "kind": "contract", "contract": CONTRACT });
    value
}

fn locked_deny(id: &str, note: &str) -> Value {
    let mut value = rule(id, "deny");
    value["locked"] = json!(true);
    value["note"] = json!(note);
    value
}

/// Two rules neither of which is more specific, with differing outcomes: one blocking finding.
fn conflicting(a: &Api) -> Vec<Value> {
    let mut deny = rule("deny-nas", "deny");
    deny["host"] = json!({ "kind": "host", "host_id": a.nas.id.to_string() });
    vec![plex("allow-plex", "allow"), deny]
}

fn engine_rules(values: &[Value]) -> Vec<Rule> {
    values
        .iter()
        .map(|v| Rule::try_from(serde_json::from_value::<RuleDto>(v.clone()).unwrap()).unwrap())
        .collect()
}

async fn put_draft(
    a: &Api,
    login: &Login,
    rules: &[Value],
    extra: Value,
) -> crate::api_harness::Reply {
    let mut body = json!({ "rules": rules });
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        body[key] = value;
    }
    a.send(Method::PUT, "/api/v1/policy/draft", Some(login), Some(body))
        .await
}

async fn preview(a: &Api, login: &Login, rules: &[Value], base: Option<u64>) -> Value {
    let body = json!({ "rules": rules, "base_revision": base });
    let reply = a
        .send(
            Method::POST,
            "/api/v1/policy/preview",
            Some(login),
            Some(body),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body
}

/// Saves `rules` as the draft (no locked rule) and returns its revision.
async fn save(a: &Api, login: &Login, rules: &[Value]) -> u64 {
    let reply = put_draft(a, login, rules, json!({})).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    reply.body["draft"]["revision"].as_u64().unwrap()
}

/// Previews the draft's rules against the active version and publishes revision `revision`.
async fn publish(
    a: &Api,
    login: &Login,
    rules: &[Value],
    revision: u64,
) -> crate::api_harness::Reply {
    let seen = preview(a, login, rules, None).await;
    let body = json!({
        "revision": revision,
        "preview_hash": seen["preview_hash"],
        "acknowledged": seen["blocking"],
        "password": PASSWORD,
    });
    a.send(
        Method::POST,
        "/api/v1/policy/draft/publish",
        Some(login),
        Some(body),
    )
    .await
}

async fn audit_kinds(a: &Api) -> Vec<String> {
    a.audit
        .list(&AuditFilter::default(), &Page::first(200))
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|event| event.kind)
        .filter(|kind| kind.starts_with("policy."))
        .collect()
}

#[tokio::test]
async fn policy_routes_require_a_session() {
    let a = api().await;
    let id = "0190aaaa-0000-7000-8000-000000000001";
    let routes = [
        (Method::GET, "/api/v1/policy".to_owned()),
        (Method::GET, "/api/v1/policy/versions".to_owned()),
        (Method::GET, "/api/v1/policy/versions/1".to_owned()),
        (Method::GET, "/api/v1/policy/draft".to_owned()),
        (Method::PUT, "/api/v1/policy/draft".to_owned()),
        (Method::DELETE, "/api/v1/policy/draft?revision=1".to_owned()),
        (Method::POST, "/api/v1/policy/preview".to_owned()),
        (Method::POST, "/api/v1/policy/draft/publish".to_owned()),
        (Method::POST, "/api/v1/policy/simulate".to_owned()),
        (Method::GET, "/api/v1/policy/shadow".to_owned()),
        (Method::GET, format!("/api/v1/policy/shadow/{id}")),
    ];
    a.admin().await;
    for (method, uri) in routes {
        let reply = a.send(method.clone(), &uri, None, Some(json!({}))).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn draft_lifecycle_over_http() {
    let a = api().await;
    let login = a.admin().await;
    assert_eq!(
        a.get("/api/v1/policy/draft", &login).await.status,
        StatusCode::NOT_FOUND
    );
    let created = put_draft(&a, &login, &[plex("one", "deny")], json!({})).await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.text);
    assert_eq!(created.body["draft"]["revision"], 1);
    let again = put_draft(&a, &login, &[plex("one", "deny")], json!({})).await;
    assert_eq!(
        again.status,
        StatusCode::CONFLICT,
        "a second draft is refused"
    );
    let replaced = put_draft(
        &a,
        &login,
        &[plex("one", "deny"), plex("two", "deny")],
        json!({ "base_revision": 1 }),
    )
    .await;
    assert_eq!(replaced.body["draft"]["revision"], 2, "{}", replaced.text);
    let stale = put_draft(
        &a,
        &login,
        &[plex("x", "deny")],
        json!({ "base_revision": 1 }),
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    let read = a.get("/api/v1/policy/draft", &login).await;
    assert_eq!(read.body["rules"].as_array().unwrap().len(), 2);
    let wrong = a
        .send(
            Method::DELETE,
            "/api/v1/policy/draft?revision=1",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(wrong.status, StatusCode::CONFLICT);
    let gone = a
        .send(
            Method::DELETE,
            "/api/v1/policy/draft?revision=2",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NO_CONTENT);
    assert_eq!(
        a.get("/api/v1/policy/draft", &login).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        audit_kinds(&a)
            .await
            .contains(&"policy.draft_discarded".to_owned())
    );
}

#[tokio::test]
async fn saving_a_draft_without_locked_changes_needs_no_password() {
    let a = api().await;
    let login = a.admin().await;
    let reply = put_draft(&a, &login, &[plex("one", "allow")], json!({})).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(a.audit_events("policy.draft_saved").await.len(), 1);
    assert!(a.audit_events("policy.lock_changed").await.is_empty());
}

#[tokio::test]
async fn locked_rule_change_needs_password_and_preview_hash() {
    let a = api().await;
    let login = a.admin().await;
    let rules = [locked_deny("no-prune", "first")];
    let none = put_draft(&a, &login, &rules, json!({})).await;
    assert_eq!(none.status, StatusCode::FORBIDDEN, "{}", none.text);
    assert!(a.get("/api/v1/policy/draft", &login).await.status == StatusCode::NOT_FOUND);
    assert!(
        audit_kinds(&a).await.is_empty(),
        "a refused password writes nothing"
    );
    let no_hash = put_draft(&a, &login, &rules, json!({ "password": PASSWORD })).await;
    assert_eq!(no_hash.status, StatusCode::CONFLICT, "{}", no_hash.text);
    let seen = preview(&a, &login, &rules, None).await;
    assert_eq!(
        seen["locked_changes"],
        json!([{ "rule": "no-prune", "change": "created" }])
    );
    let ok = put_draft(
        &a,
        &login,
        &rules,
        json!({ "password": PASSWORD, "preview_hash": seen["preview_hash"] }),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(a.audit_events("policy.lock_changed").await.len(), 1);
    // Editing, unlocking and removing it behave the same.
    let mut unlocked = rule("no-prune", "deny");
    unlocked["note"] = json!("first");
    for (next, change) in [
        (vec![locked_deny("no-prune", "second")], "edited"),
        (vec![unlocked], "unlocked"),
    ] {
        let denied = put_draft(&a, &login, &next, json!({ "base_revision": 1 })).await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN, "{change}");
    }
    let removing = preview(&a, &login, &[], Some(1)).await;
    assert_eq!(removing["locked_changes"][0]["change"], "removed");
    let edited = preview(&a, &login, &[locked_deny("no-prune", "second")], Some(1)).await;
    assert_eq!(edited["locked_changes"][0]["change"], "edited");
}

#[tokio::test]
async fn wrong_password_is_forbidden_and_lockout_is_429() {
    let a = api().await;
    let login = a.admin().await;
    let rules = [locked_deny("no-prune", "x")];
    let seen = preview(&a, &login, &rules, None).await;
    let body =
        |password: &str| json!({ "password": password, "preview_hash": seen["preview_hash"] });
    for _ in 0..5 {
        let wrong = put_draft(&a, &login, &rules, body("not the password")).await;
        assert_eq!(wrong.status, StatusCode::FORBIDDEN);
        assert_eq!(wrong.body["message"], "wrong password");
    }
    let locked = put_draft(&a, &login, &rules, body(PASSWORD)).await;
    assert_eq!(
        locked.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        locked.text
    );
}

#[tokio::test]
async fn publish_always_needs_the_password() {
    let a = api().await;
    let login = a.admin().await;
    let rules = [plex("one", "deny")];
    let revision = save(&a, &login, &rules).await;
    let seen = preview(&a, &login, &rules, None).await;
    let body = json!({ "revision": revision, "preview_hash": seen["preview_hash"] });
    let reply = a
        .send(
            Method::POST,
            "/api/v1/policy/draft/publish",
            Some(&login),
            Some(body),
        )
        .await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
    assert_eq!(
        reply.body["message"],
        "enter your password to change the policy"
    );
    assert!(
        a.get("/api/v1/policy/versions", &login).await.body["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn publish_requires_exact_acknowledgements() {
    let a = api().await;
    let login = a.admin().await;
    let rules = conflicting(&a);
    let revision = save(&a, &login, &rules).await;
    let seen = preview(&a, &login, &rules, None).await;
    assert_eq!(seen["blocking"], json!([PLEX_ALLOWS_NAS_DENIES]));
    let send = |acknowledged: Value| {
        let body = json!({
            "revision": revision, "preview_hash": seen["preview_hash"],
            "acknowledged": acknowledged, "password": PASSWORD,
        });
        a.send(
            Method::POST,
            "/api/v1/policy/draft/publish",
            Some(&login),
            Some(body),
        )
    };
    let none = send(json!([])).await;
    assert_eq!(none.status, StatusCode::CONFLICT, "{}", none.text);
    assert!(
        none.body["message"]
            .as_str()
            .unwrap()
            .contains(PLEX_ALLOWS_NAS_DENIES)
    );
    let extra = send(json!([PLEX_ALLOWS_NAS_DENIES, "equal_scope_conflict:x:y"])).await;
    assert_eq!(extra.status, StatusCode::BAD_REQUEST, "{}", extra.text);
    let exact = send(json!([PLEX_ALLOWS_NAS_DENIES])).await;
    assert_eq!(exact.status, StatusCode::OK, "{}", exact.text);
    let version = a.get("/api/v1/policy/versions/1", &login).await;
    let ack = &version.body["acknowledgements"][0];
    assert_eq!(ack["finding_key"], PLEX_ALLOWS_NAS_DENIES);
    assert_eq!(ack["kind"], "incomparable_conflict");
    assert!(
        ack["acknowledged_by"]
            .as_str()
            .unwrap()
            .starts_with("user:")
    );
    assert!(ack["acknowledged_at"].as_str().is_some());
}

#[tokio::test]
async fn publish_with_stale_preview_hash_conflicts() {
    let a = api().await;
    let login = a.admin().await;
    let first = [plex("one", "deny")];
    let revision = save(&a, &login, &first).await;
    let seen = preview(&a, &login, &first, None).await;
    let next = put_draft(
        &a,
        &login,
        &[plex("one", "deny"), plex("two", "deny")],
        json!({ "base_revision": revision }),
    )
    .await;
    let body = json!({
        "revision": next.body["draft"]["revision"], "preview_hash": seen["preview_hash"],
        "acknowledged": [], "password": PASSWORD,
    });
    let reply = a
        .send(
            Method::POST,
            "/api/v1/policy/draft/publish",
            Some(&login),
            Some(body),
        )
        .await;
    assert_eq!(reply.status, StatusCode::CONFLICT, "{}", reply.text);
}

#[tokio::test]
async fn publish_records_journal_and_audit_in_one_commit() {
    let a = api().await;
    let login = a.admin().await;
    let mut noted = plex("one", "deny");
    noted["note"] = json!("an owner note that must stay out of the journal");
    let rules = [noted];
    let revision = save(&a, &login, &rules).await;
    let published = publish(&a, &login, &rules, revision).await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.text);
    let rows: Vec<_> = a
        .store
        .entries_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.kind == "policy.published")
        .collect();
    assert_eq!(rows.len(), 1);
    let payload = rows[0].payload.clone();
    assert_eq!(payload["version"], 1);
    assert_eq!(payload["rules"], 1);
    assert_eq!(payload["rules_hash"].as_str().unwrap().len(), 64);
    assert!(!payload.to_string().contains("owner note"));
    assert_eq!(a.audit_events("policy.published").await.len(), 1);
    assert!(
        !a.audit_events("policy.published").await[0]
            .detail
            .to_string()
            .contains("owner note")
    );
}

/// A store that refuses every commit, as one whose audit append failed would.
struct RefusingUow;

#[async_trait]
impl UnitOfWork for RefusingUow {
    async fn commit(&self, _tx: Transaction) -> Result<CommitReceipt, CommitError> {
        Err(CommitError::Backend("the audit append failed".to_owned()))
    }
}

#[tokio::test]
async fn publish_that_cannot_commit_publishes_nothing() {
    let a = api().await;
    let login = a.admin().await;
    let rules = [plex("one", "deny")];
    let revision = save(&a, &login, &rules).await;
    let seen = preview(&a, &login, &rules, None).await;
    let mut deps: ApiDeps = (**a.state.deps()).clone();
    deps.services.policy = PolicyService::new(PolicyDeps {
        repo: a.store.clone(),
        uow: Arc::new(RefusingUow),
        env: a.policy_env.clone(),
        auth: a.state.auth().clone(),
        clock: a.clock.clone(),
        shadow_enabled: true,
    });
    let app =
        router(ApiState::new(deps, a.state.auth().clone()), None).layer(MockConnectInfo(peer()));
    let body = json!({
        "revision": revision, "preview_hash": seen["preview_hash"],
        "acknowledged": [], "password": PASSWORD,
    });
    let reply = a
        .send_to(
            app,
            Method::POST,
            "/api/v1/policy/draft/publish",
            Some(&login),
            Some(body),
        )
        .await;
    assert_eq!(
        reply.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        reply.text
    );
    assert!(a.store.active().await.unwrap().is_none());
    assert!(
        audit_kinds(&a)
            .await
            .iter()
            .all(|kind| kind != "policy.published")
    );
}

#[tokio::test]
async fn publish_twice_assigns_numbers_and_keeps_the_old_version_readable() {
    let a = api().await;
    let login = a.admin().await;
    let first = [plex("one", "deny")];
    let revision = save(&a, &login, &first).await;
    assert_eq!(
        publish(&a, &login, &first, revision).await.status,
        StatusCode::OK
    );
    let one = a.get("/api/v1/policy/versions/1", &login).await.body;
    let second = [plex("one", "deny"), plex("two", "deny")];
    let revision = save(&a, &login, &second).await;
    let published = publish(&a, &login, &second, revision).await;
    assert_eq!(published.body["version"]["number"], 2, "{}", published.text);
    assert_eq!(a.get("/api/v1/policy/versions/1", &login).await.body, one);
    let overview = a.get("/api/v1/policy", &login).await.body;
    assert_eq!(overview["active"]["number"], 2);
    let listed = a.get("/api/v1/policy/versions", &login).await.body;
    assert_eq!(listed["items"][0]["number"], 2);
    assert_eq!(listed["items"][1]["number"], 1);
}

#[tokio::test]
async fn publish_refuses_package_contract_and_unknown_contract_rules() {
    let a = api().await;
    let login = a.admin().await;
    let mut package = rule("pkg", "deny");
    package["capability"] = json!({ "kind": "contract", "contract": "agent.package.install" });
    let reply = put_draft(&a, &login, &[package], json!({})).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert_eq!(reply.body["issues"][0]["path"], "rules.pkg");
    let mut unknown = rule("ghost", "deny");
    unknown["capability"] =
        json!({ "kind": "contract", "contract": "skill.nobody_announces_this" });
    let revision = save(&a, &login, &[unknown.clone()]).await;
    let seen = preview(&a, &login, &[unknown.clone()], None).await;
    assert!(!seen["validation"].as_array().unwrap().is_empty());
    assert!(seen["report"].is_null());
    let body = json!({
        "revision": revision, "preview_hash": seen["preview_hash"],
        "acknowledged": [], "password": PASSWORD,
    });
    let reply = a
        .send(
            Method::POST,
            "/api/v1/policy/draft/publish",
            Some(&login),
            Some(body),
        )
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
}

#[tokio::test]
async fn preview_reports_conflicts_locks_and_a_bounded_simulation() {
    let a = api().await;
    let login = a.admin().await;
    for _ in 0..3 {
        record_step(&a, &[], 0).await;
    }
    let mut rules = conflicting(&a);
    rules.push(locked_deny("no-prune", "x"));
    let seen = preview(&a, &login, &rules, None).await;
    // The locked deny-all makes `allow-plex` ineffective, which is a finding of its own.
    assert_eq!(
        seen["blocking"],
        json!([
            PLEX_ALLOWS_NAS_DENIES,
            "ineffective_under_lock:no-prune:allow-plex"
        ])
    );
    assert_eq!(seen["locked_changes"][0]["rule"], "no-prune");
    assert_eq!(seen["simulation"]["evaluated"], 3);
    assert_eq!(seen["simulation"]["newly_denied"], 3);
    assert_eq!(seen["notes"].as_array().unwrap().len(), 3);
    // The hash is the one a draft save of the same rules expects.
    let saved = put_draft(
        &a,
        &login,
        &rules,
        json!({ "password": PASSWORD, "preview_hash": seen["preview_hash"] }),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
}

#[tokio::test]
async fn simulate_reproduces_recorded_decisions_for_the_recorded_version() {
    let a = api().await;
    let login = a.admin().await;
    let mut shadow = plex("shadow-allow", "allow");
    shadow["shadow"] = json!(true);
    let rules = [plex("deny-plex", "deny"), shadow];
    let revision = save(&a, &login, &rules).await;
    assert_eq!(
        publish(&a, &login, &rules, revision).await.status,
        StatusCode::OK
    );
    for _ in 0..2 {
        record_step(&a, &engine_rules(&rules), 1).await;
    }
    let body = json!({ "rules": { "source": "version", "number": 1 }, "selection": "both" });
    let reply = a.post("/api/v1/policy/simulate", &login, body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let items = reply.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    for item in items {
        assert_eq!(item["enforced"]["comparison"], "same");
        assert_eq!(item["shadow"]["comparison"], "same");
        assert_eq!(item["recorded_policy_version"], 1);
    }
    assert_eq!(reply.body["summary"]["changed"], 0);
}

#[tokio::test]
async fn simulate_draft_shows_what_would_change() {
    let a = api().await;
    let login = a.admin().await;
    record_step(&a, &[], 0).await;
    save(&a, &login, &[plex("deny-plex", "deny")]).await;
    let body = json!({ "rules": { "source": "draft" }, "selection": "enforced" });
    let reply = a.post("/api/v1/policy/simulate", &login, body).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["items"][0]["enforced"]["comparison"], "changed");
    assert_eq!(reply.body["summary"]["newly_denied"], 1);
    assert_eq!(reply.body["label"], "draft revision 1");
    let gone = a
        .post(
            "/api/v1/policy/simulate",
            &login,
            json!({ "rules": { "source": "version", "number": 9 } }),
        )
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn simulate_never_writes() {
    let a = api().await;
    let login = a.admin().await;
    record_step(&a, &[], 0).await;
    save(&a, &login, &[plex("deny-plex", "deny")]).await;
    let before = (
        audit_kinds(&a).await.len(),
        a.store.head_seq().await.unwrap(),
        a.store.shadow_state().await.unwrap().total_evaluated,
        a.store
            .shadow_records(&ShadowFilter::default(), &Page::first(100))
            .await
            .unwrap()
            .items
            .len(),
    );
    for inputs in ["recorded", "current"] {
        let body = json!({ "rules": { "source": "draft" }, "inputs": inputs });
        assert_eq!(
            a.post("/api/v1/policy/simulate", &login, body).await.status,
            StatusCode::OK
        );
    }
    let after = (
        audit_kinds(&a).await.len(),
        a.store.head_seq().await.unwrap(),
        a.store.shadow_state().await.unwrap().total_evaluated,
        a.store
            .shadow_records(&ShadowFilter::default(), &Page::first(100))
            .await
            .unwrap()
            .items
            .len(),
    );
    assert_eq!(before, after);
}

#[tokio::test]
async fn shadow_endpoints_page_and_filter_and_404() {
    let a = api().await;
    let login = a.admin().await;
    let first = record_step(&a, &[], 0).await;
    a.advance(jiff::SignedDuration::from_secs(1));
    record_step(&a, &[], 0).await;
    let page = a.get("/api/v1/policy/shadow?limit=1", &login).await;
    assert_eq!(
        page.body["items"].as_array().unwrap().len(),
        1,
        "{}",
        page.text
    );
    let cursor = page.body["next_cursor"].as_str().unwrap().to_owned();
    let next = a
        .get(
            &format!("/api/v1/policy/shadow?limit=1&cursor={cursor}"),
            &login,
        )
        .await;
    assert_eq!(next.body["items"][0]["execution_id"], first.to_string());
    let filtered = a
        .get(
            &format!(
                "/api/v1/policy/shadow?status=evaluated&host={}&contract={CONTRACT}",
                a.nas.id
            ),
            &login,
        )
        .await;
    assert_eq!(filtered.body["items"].as_array().unwrap().len(), 2);
    let none = a.get("/api/v1/policy/shadow?status=skipped", &login).await;
    assert!(none.body["items"].as_array().unwrap().is_empty());
    let bad = a.get("/api/v1/policy/shadow?status=nonsense", &login).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let detail = a
        .get(&format!("/api/v1/policy/shadow/{first}"), &login)
        .await;
    assert_eq!(detail.status, StatusCode::OK, "{}", detail.text);
    assert_eq!(detail.body["enforced_outcome"], "require_approval");
    assert!(detail.body["gaps"].as_array().unwrap().len() >= 2);
    assert!(detail.body["inputs"]["contract"]["id"] == CONTRACT);
    let missing = a
        .get(
            "/api/v1/policy/shadow/0190aaaa-0000-7000-8000-000000000009",
            &login,
        )
        .await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let rows = a
        .store
        .shadow_records(
            &ShadowFilter {
                status: Some(ShadowStatus::Evaluated),
                ..ShadowFilter::default()
            },
            &Page::first(10),
        )
        .await
        .unwrap();
    assert_eq!(rows.items.len(), 2);
}

#[tokio::test]
async fn overview_reports_standing_notes_and_zone() {
    let a = api().await;
    let login = a.admin().await;
    let overview = a.get("/api/v1/policy", &login).await;
    assert_eq!(overview.status, StatusCode::OK, "{}", overview.text);
    assert_eq!(overview.body["shadow_enabled"], true);
    assert_eq!(overview.body["time_window_zone"], "UTC");
    assert_eq!(overview.body["notes"].as_array().unwrap().len(), 3);
    assert!(overview.body["active"].is_null());
    assert!(overview.body["draft"].is_null());
    assert_eq!(overview.body["shadow"]["total_evaluated"], 0);
}

#[test]
fn policy_openapi_covers_the_eleven_routes() {
    let spec = domovoi_hub::router::openapi();
    let expected = [
        ("/api/v1/policy", "get", "policy_overview"),
        ("/api/v1/policy/versions", "get", "policy_versions"),
        ("/api/v1/policy/versions/{number}", "get", "policy_version"),
        ("/api/v1/policy/draft", "get", "policy_draft"),
        ("/api/v1/policy/draft", "put", "policy_save_draft"),
        ("/api/v1/policy/draft", "delete", "policy_discard_draft"),
        ("/api/v1/policy/preview", "post", "policy_preview"),
        ("/api/v1/policy/draft/publish", "post", "policy_publish"),
        ("/api/v1/policy/simulate", "post", "policy_simulate"),
        ("/api/v1/policy/shadow", "get", "policy_shadow_list"),
        (
            "/api/v1/policy/shadow/{execution_id}",
            "get",
            "policy_shadow_detail",
        ),
    ];
    for (path, method, id) in expected {
        let item = spec
            .paths
            .paths
            .get(path)
            .unwrap_or_else(|| panic!("{path}"));
        let op = match method {
            "get" => item.get.as_ref(),
            "put" => item.put.as_ref(),
            "post" => item.post.as_ref(),
            _ => item.delete.as_ref(),
        }
        .unwrap_or_else(|| panic!("{method} {path}"));
        assert_eq!(op.operation_id.as_deref(), Some(id));
    }
    let schemas = spec.components.unwrap().schemas;
    assert!(schemas.keys().all(|name| !name.contains("Permit")));
}
