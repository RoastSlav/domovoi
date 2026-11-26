//! The audit log routes and the retention block of the settings answer.

use axum::http::StatusCode;
use domovoi_core::ports::AuditEvent;
use serde_json::{Value, json};

use crate::api_harness::{Api, api};

const KIND: &str = "test.audit.entry";

async fn append(a: &Api, detail: Value) -> u64 {
    let event = AuditEvent {
        at: a.now(),
        actor: "system:test".to_owned(),
        kind: KIND.to_owned(),
        subject: "subject:1".to_owned(),
        detail,
    };
    a.audit.append(&event).await.unwrap()
}

async fn exec(a: &Api, sql: &str) {
    sqlx::query(sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(a.store.pool_for_tests())
        .await
        .unwrap();
}

async fn count(a: &Api, table: &str) -> i64 {
    let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(a.store.pool_for_tests())
        .await
        .unwrap();
    row.0
}

fn items(body: &Value) -> Vec<Value> {
    body["items"].as_array().unwrap().clone()
}

#[tokio::test]
async fn audit_routes_require_a_session() {
    let a = api().await;
    for uri in ["/api/v1/audit", "/api/v1/audit/verification"] {
        let reply = a.send(axum::http::Method::GET, uri, None, None).await;
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
async fn audit_list_returns_rows_newest_first_with_hashes() {
    let a = api().await;
    let login = a.admin().await;
    let seqs = [
        append(&a, json!({"n": 1})).await,
        append(&a, json!({"n": 2})).await,
        append(&a, json!({"n": 3})).await,
    ];
    let reply = a.get(&format!("/api/v1/audit?kind={KIND}"), &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let rows = items(&reply.body);
    let got: Vec<u64> = rows.iter().map(|r| r["seq"].as_u64().unwrap()).collect();
    assert_eq!(got, [seqs[2], seqs[1], seqs[0]]);
    assert!(reply.body["next_cursor"].is_null());
    for row in &rows {
        assert!(row.get("history").is_none());
        let hash = row["hash"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        row["at"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .unwrap();
    }
}

#[tokio::test]
async fn audit_list_pages_with_a_cursor_and_a_limit() {
    let a = api().await;
    let login = a.admin().await;
    let mut seeded = Vec::new();
    for n in 0..5 {
        seeded.push(append(&a, json!({ "n": n })).await);
    }
    seeded.reverse();
    let mut uri = format!("/api/v1/audit?kind={KIND}&limit=2");
    let mut pages = Vec::new();
    loop {
        let reply = a.get(&uri, &login).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        pages.push(
            items(&reply.body)
                .iter()
                .map(|r| r["seq"].as_u64().unwrap())
                .collect::<Vec<_>>(),
        );
        match reply.body["next_cursor"].as_str() {
            Some(c) => uri = format!("/api/v1/audit?kind={KIND}&limit=2&cursor={c}"),
            None => break,
        }
    }
    assert_eq!(
        pages,
        [
            seeded[0..2].to_vec(),
            seeded[2..4].to_vec(),
            seeded[4..5].to_vec()
        ]
    );
}

#[tokio::test]
async fn audit_list_rejects_a_long_kind() {
    let a = api().await;
    let login = a.admin().await;
    let kind = "k".repeat(101);
    let long = a.get(&format!("/api/v1/audit?kind={kind}"), &login).await;
    assert_eq!(long.status, StatusCode::BAD_REQUEST, "{}", long.text);
    assert!(long.text.contains("kind is too long"));
}

#[tokio::test]
async fn audit_detail_is_redacted_cut_and_empty_for_null() {
    let a = api().await;
    let login = a.admin().await;
    let key = format!("sk-{}", "a".repeat(24));
    append(&a, json!({ "token": key })).await;
    append(&a, json!("x".repeat(6 * 1024))).await;
    append(&a, Value::Null).await;
    let reply = a.get(&format!("/api/v1/audit?kind={KIND}"), &login).await;
    let rows = items(&reply.body);
    let null_row = &rows[0];
    let long_row = &rows[1];
    let secret_row = &rows[2];
    assert_eq!(null_row["detail"], "");
    assert_eq!(long_row["detail_truncated"], true);
    assert!(long_row["detail"].as_str().unwrap().len() <= 4096);
    let secret = secret_row["detail"].as_str().unwrap();
    assert!(secret.contains("[REDACTED]"), "{secret}");
    assert!(!secret.contains(&key));
}

#[tokio::test]
async fn verification_of_a_fresh_install_reports_an_empty_chain() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/audit/verification", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let body = &reply.body;
    assert_eq!(body["scope"], "recent");
    assert_eq!(body["recent_limit"], 10_000);
    assert!(body.get("legacy").is_none());
    let status = body["chained"]["status"].as_str().unwrap();
    assert!(["empty", "intact"].contains(&status), "{status}");
    assert!(body["anchor"].get("legacy_last_id").is_none());
    assert_eq!(body["anchor"]["anchor_hash"].as_str().unwrap().len(), 64);
    assert_eq!(body["checked_at"], a.now().to_string());
}

#[tokio::test]
async fn full_verification_checks_every_row() {
    let a = api().await;
    let login = a.admin().await;
    append(&a, json!({})).await;
    let newest = append(&a, json!({})).await;
    let reply = a.get("/api/v1/audit/verification?scope=full", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    assert_eq!(reply.body["scope"], "full");
    assert_eq!(reply.body["chained"]["status"], "intact");
    assert_eq!(reply.body["head"]["seq"], newest);
    assert_eq!(
        reply.body["chained"]["head_hash"],
        reply.body["head"]["hash"]
    );
}

#[tokio::test]
async fn a_tampered_chain_is_reported_as_broken_with_the_first_broken_entry() {
    let a = api().await;
    let login = a.admin().await;
    append(&a, json!({"n": 1})).await;
    let middle = append(&a, json!({"n": 2})).await;
    append(&a, json!({"n": 3})).await;
    exec(&a, "DROP TRIGGER audit_log_no_update").await;
    exec(
        &a,
        &format!("UPDATE audit_log SET detail = '{{\"n\":99}}' WHERE seq = {middle}"),
    )
    .await;
    exec(
        &a,
        "CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log BEGIN \
         SELECT RAISE(ABORT, 'audit_log is append-only'); END",
    )
    .await;
    let reply = a.get("/api/v1/audit/verification?scope=full", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let chained = &reply.body["chained"];
    assert_eq!(chained["status"], "broken");
    assert_eq!(chained["first_broken_seq"], middle);
    assert_eq!(chained["break_reason"], "hash_mismatch");
    assert_eq!(chained["break_text"], "a row was edited");
}

#[tokio::test]
async fn a_verification_error_is_a_500_with_no_status() {
    let a = api().await;
    let login = a.admin().await;
    exec(&a, "DROP TRIGGER audit_anchor_no_update").await;
    exec(&a, "UPDATE audit_anchor SET install_id = 'edited'").await;
    exec(
        &a,
        "CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN \
         SELECT RAISE(ABORT, 'audit_anchor is append-only'); END",
    )
    .await;
    let reply = a.get("/api/v1/audit/verification", &login).await;
    assert_eq!(
        reply.status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "{}",
        reply.text
    );
    assert!(reply.body.get("legacy").is_none() && reply.body.get("chained").is_none());
}

#[tokio::test]
async fn unknown_scope_is_a_400() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a
        .get("/api/v1/audit/verification?scope=everything", &login)
        .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{}", reply.text);
    assert!(
        reply
            .text
            .contains("unknown scope `everything`; use recent or full")
    );
}

#[tokio::test]
async fn reading_the_audit_log_appends_nothing() {
    let a = api().await;
    let login = a.admin().await;
    append(&a, json!({})).await;
    let before = (count(&a, "audit_log").await, count(&a, "journal").await);
    for uri in [
        "/api/v1/audit",
        "/api/v1/audit/verification",
        "/api/v1/audit/verification?scope=full",
    ] {
        assert_eq!(a.get(uri, &login).await.status, StatusCode::OK, "{uri}");
    }
    let after = (count(&a, "audit_log").await, count(&a, "journal").await);
    assert_eq!(before, after);
}

#[tokio::test]
async fn settings_report_the_nine_retention_periods_and_which_apply() {
    let a = api().await;
    let login = a.admin().await;
    let reply = a.get("/api/v1/settings", &login).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let retention = &reply.body["retention"];
    let entries = retention["entries"].as_array().unwrap();
    let keys: Vec<&str> = entries.iter().map(|e| e["key"].as_str().unwrap()).collect();
    assert_eq!(
        keys,
        [
            "signals_days",
            "incident_timeline_days",
            "conversations_days",
            "notifications_days",
            "triage_cache_days",
            "resolved_advisories_days",
            "operation_journal_days",
            "execution_outputs_days",
            "retracted_facts_days"
        ]
    );
    let by = |key: &str| entries.iter().find(|e| e["key"] == key).unwrap();
    assert_eq!(by("signals_days")["days"], 30);
    assert_eq!(by("incident_timeline_days")["days"], 90);
    assert_eq!(by("conversations_days")["days"], 180);
    for key in ["operation_journal_days", "execution_outputs_days"] {
        assert_eq!(by(key)["applies"], "later", "{key}");
        assert!(by(key)["applies_once"].is_string());
    }
    assert_eq!(by("signals_days")["applies"], "now");
    assert_eq!(by("retracted_facts_days")["applies"], "now");
    assert_eq!(retention["backups_keep"], 7);
    assert_eq!(retention["backups_keep_pre_migrate"], 3);
}

#[tokio::test]
async fn the_openapi_document_lists_the_audit_operations_and_the_retention_schema() {
    let doc: Value = serde_json::from_str(&domovoi_hub::router::openapi_json()).unwrap();
    assert_eq!(
        doc["paths"]["/api/v1/audit"]["get"]["operationId"],
        "list_audit"
    );
    assert_eq!(
        doc["paths"]["/api/v1/audit/verification"]["get"]["operationId"],
        "verify_audit"
    );
    assert_eq!(
        doc["components"]["schemas"]["AuditRowDto"]["properties"]["hash"]["type"],
        serde_json::json!(["string", "null"])
    );
    let required = doc["components"]["schemas"]["SettingsDto"]["required"]
        .as_array()
        .unwrap();
    assert!(required.iter().any(|r| r == "retention"));
}
