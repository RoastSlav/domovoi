//! The database itself keeps the audit log append-only (INV-7).

use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page};
use domovoi_testkit::build::at;

use crate::common::Fresh;

#[tokio::test]
async fn audit_log_rejects_update_and_delete() {
    let fresh = Fresh::new().await;
    let event = AuditEvent {
        at: at(1),
        actor: "user:alice".to_owned(),
        kind: "action.approved".to_owned(),
        subject: "action:1".to_owned(),
        detail: serde_json::json!({ "plan": "abc" }),
    };
    assert_eq!(fresh.store.append(&event).await.unwrap(), 1);
    let pool = fresh.store.pool_for_tests();

    for sql in [
        "UPDATE audit_log SET actor = 'user:mallory'",
        "UPDATE audit_log SET detail = 'null' WHERE seq = 1",
        "DELETE FROM audit_log",
        "DELETE FROM audit_log WHERE seq = 1",
    ] {
        let err = sqlx::query(sql).execute(pool).await.unwrap_err();
        assert!(
            err.to_string().contains("audit_log is append-only"),
            "{sql}: {err}"
        );
    }
    // Replacing a row is an update in disguise and is refused too.
    let replace = "INSERT OR REPLACE INTO audit_log (seq, at, actor, kind, subject, detail) \
        VALUES (1, 0, 'x', 'x', 'x', 'null')";
    let err = sqlx::query(replace).execute(pool).await.unwrap_err();
    assert!(
        err.to_string().contains("audit_log is append-only"),
        "{err}"
    );

    let page = fresh
        .store
        .list(&AuditFilter::default(), &Page::first(10))
        .await
        .unwrap();
    assert_eq!(page.items, vec![event]);
}
