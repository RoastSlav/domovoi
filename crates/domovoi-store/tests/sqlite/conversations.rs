//! Operator conversations in SQLite: transactions, guards, cascade, journal and retention.

use domovoi_core::conversation::{Conversation, ConversationScope, Message, MessageStatus};
use domovoi_core::ports::{
    AggregateRef, CommitError, ConversationRepo, ConversationWrite, Expectation, Expected,
    JournalDraft, JournalKind, MessageGuard, Observed, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{Actor, ConversationId, OperationId, TurnId};
use domovoi_store::{PURGED_TABLES, RetentionPolicy, Store};
use domovoi_testkit::build::{T0, at};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use crate::common::Fresh;

fn days(n: i64) -> SignedDuration {
    SignedDuration::from_hours(24 * n)
}

fn conversation(updated: Timestamp) -> Conversation {
    Conversation {
        id: ConversationId::new(),
        title: "Why is it slow?".to_owned(),
        scope: ConversationScope::Global,
        operation: None,
        last_ordinal: 0,
        created_at: updated,
        updated_at: updated,
    }
}

fn write(w: ConversationWrite) -> StateWrite {
    StateWrite::Conversation(w)
}

fn create(c: &Conversation) -> StateWrite {
    write(ConversationWrite::Conversation {
        conversation: c.clone(),
        expected: Expected::New,
    })
}

fn put(m: &Message, guard: MessageGuard) -> StateWrite {
    write(ConversationWrite::Message {
        message: m.clone(),
        guard,
    })
}

fn journal(c: &Conversation, kind: JournalKind) -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::conversation(c.id),
        kind,
        payload_v: 1,
        payload: json!({"conversation": c.id.to_string()}),
        actor: Actor::System("operator".to_owned()),
        occurred_at: at(1),
    }
}

async fn seed(store: &Store, c: &Conversation) {
    store
        .commit(Transaction::new().write(create(c)))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_question_commits_both_messages_and_their_journal_rows_together() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let c = conversation(at(1));
    seed(store, &c).await;
    let turn = TurnId::new();
    let user = Message::user(c.id, 1, turn, "q".to_owned(), at(2));
    let answer = Message::pending_answer(c.id, 2, turn, at(2));
    let mut next = c.clone();
    next.last_ordinal = 2;
    let tx = Transaction::new()
        .write(write(ConversationWrite::Conversation {
            conversation: next.clone(),
            expected: Expected::Version(1),
        }))
        .write(put(&user, MessageGuard::New))
        .write(put(&answer, MessageGuard::New))
        .journal(journal(&c, JournalKind::ConversationMessage))
        .journal(journal(&c, JournalKind::ConversationMessage));
    let receipt = store.commit(tx).await.unwrap();
    assert_eq!(receipt.journal_seqs.len(), 2);
    assert_eq!(store.messages(c.id).await.unwrap().len(), 2);
    assert_eq!(
        store.get(c.id).await.unwrap().unwrap().value.last_ordinal,
        2
    );

    // A duplicate ordinal in the second message rolls back all of it.
    let c2 = conversation(at(3));
    seed(store, &c2).await;
    let turn = TurnId::new();
    let first = Message::user(c2.id, 1, turn, "q".to_owned(), at(4));
    let clash = Message::pending_answer(c2.id, 1, turn, at(4));
    let tx = Transaction::new()
        .write(put(&first, MessageGuard::New))
        .write(put(&clash, MessageGuard::New))
        .journal(journal(&c2, JournalKind::ConversationMessage));
    assert!(matches!(
        store.commit(tx).await,
        Err(CommitError::Constraint(_))
    ));
    assert!(store.messages(c2.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn commit_publishes_conversation_journal_seqs() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let c = conversation(at(1));
    let receipt = store
        .commit(
            Transaction::new()
                .write(create(&c))
                .journal(journal(&c, JournalKind::ConversationCreated)),
        )
        .await
        .unwrap();
    let (rows,): (i64,) = sqlx::query_as("SELECT count(*) FROM journal WHERE kind = ?")
        .bind("conversation.created")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(receipt.journal_seqs.len(), 1);
    assert_eq!(
        receipt.versions,
        vec![(AggregateRef::conversation(c.id), 1)]
    );
}

#[tokio::test]
async fn a_second_pending_message_in_one_conversation_is_refused() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let c = conversation(at(1));
    seed(store, &c).await;
    let a = Message::pending_answer(c.id, 1, TurnId::new(), at(2));
    let b = Message::pending_answer(c.id, 2, TurnId::new(), at(2));
    store
        .commit(Transaction::new().write(put(&a, MessageGuard::New)))
        .await
        .unwrap();
    let refused = store
        .commit(Transaction::new().write(put(&b, MessageGuard::New)))
        .await;
    assert!(matches!(refused, Err(CommitError::Constraint(_))));
}

#[tokio::test]
async fn a_late_settle_of_a_cancelled_message_conflicts() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let c = conversation(at(1));
    seed(store, &c).await;
    let pending = Message::pending_answer(c.id, 1, TurnId::new(), at(2));
    store
        .commit(Transaction::new().write(put(&pending, MessageGuard::New)))
        .await
        .unwrap();
    let mut cancelled = pending.clone();
    cancelled.status = MessageStatus::Cancelled;
    store
        .commit(Transaction::new().write(put(
            &cancelled,
            MessageGuard::Status(MessageStatus::Pending),
        )))
        .await
        .unwrap();
    let mut late = pending.clone();
    late.status = MessageStatus::Complete;
    late.text = "late".to_owned();
    let result = store
        .commit(Transaction::new().write(put(&late, MessageGuard::Status(MessageStatus::Pending))))
        .await;
    assert!(matches!(
        result,
        Err(CommitError::Conflict {
            expected: Expectation::Status(_),
            found: Observed::Status(ref s),
            ..
        }) if s == "cancelled"
    ));
}

#[tokio::test]
async fn deleting_a_conversation_removes_its_messages() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let c = conversation(at(1));
    seed(store, &c).await;
    let m = Message::user(c.id, 1, TurnId::new(), "q".to_owned(), at(2));
    store
        .commit(Transaction::new().write(put(&m, MessageGuard::New)))
        .await
        .unwrap();
    store
        .commit(Transaction::new().write(write(ConversationWrite::Delete {
            id: c.id,
            expected: 1,
        })))
        .await
        .unwrap();
    let (left,): (i64,) = sqlx::query_as("SELECT count(*) FROM messages")
        .fetch_one(store.pool_for_tests())
        .await
        .unwrap();
    assert_eq!(left, 0, "foreign keys are on, so the cascade ran");
}

async fn seed_with_message(
    store: &Store,
    updated: Timestamp,
    operation: Option<OperationId>,
    status_pending: bool,
) -> Conversation {
    let mut c = conversation(updated);
    c.operation = operation;
    seed(store, &c).await;
    let turn = TurnId::new();
    let message = if status_pending {
        Message::pending_answer(c.id, 1, turn, updated)
    } else {
        Message::user(c.id, 1, turn, "q".to_owned(), updated)
    };
    store
        .commit(Transaction::new().write(put(&message, MessageGuard::New)))
        .await
        .unwrap();
    c
}

#[tokio::test]
async fn purge_deletes_old_messages_and_keeps_operation_links() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let now = T0 + days(100);
    let old = now - days(60);
    let plain = seed_with_message(store, old, None, false).await;
    let linked = seed_with_message(store, old, Some(OperationId::new()), false).await;
    let recent = seed_with_message(store, now - days(1), None, false).await;
    let pending = seed_with_message(store, old, None, true).await;
    let policy = RetentionPolicy {
        conversations: days(30),
        ..RetentionPolicy::default()
    };

    let report = store.purge(now, &policy).await.unwrap();
    assert_eq!(report.operator_messages, 2, "plain and linked, not pending");
    assert_eq!(report.operator_conversations, 1, "only the plain one");
    assert!(store.get(plain.id).await.unwrap().is_none());
    let kept = store.get(linked.id).await.unwrap();
    assert!(kept.is_some(), "the operation link stays");
    assert!(store.messages(linked.id).await.unwrap().is_empty());
    assert_eq!(store.messages(recent.id).await.unwrap().len(), 1);
    assert_eq!(store.messages(pending.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn purge_deletes_operator_rows_in_the_same_transaction() {
    let fresh = Fresh::new().await;
    let store = &fresh.store;
    let now = T0 + days(100);
    let plain = seed_with_message(store, now - days(60), None, false).await;
    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, csrf_hash, created_at, expires_at) \
         VALUES (?, 'user:x', ?, ?, ?)",
    )
    .bind(vec![1_u8; 32])
    .bind(vec![2_u8; 32])
    .bind((now - days(2)).as_millisecond())
    .bind((now - days(1)).as_millisecond())
    .execute(store.pool_for_tests())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER boom BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'boom'); END",
    )
    .execute(store.pool_for_tests())
    .await
    .unwrap();
    let policy = RetentionPolicy {
        conversations: days(30),
        ..RetentionPolicy::default()
    };
    assert!(store.purge(now, &policy).await.is_err());
    assert_eq!(store.messages(plain.id).await.unwrap().len(), 1);
}

#[test]
fn purged_tables_list_names_the_operator_tables() {
    assert!(PURGED_TABLES.contains(&"messages"));
    assert!(PURGED_TABLES.contains(&"conversations"));
}
