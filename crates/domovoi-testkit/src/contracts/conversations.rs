//! Contract for [`ConversationRepo`] and the conversation writes of the unit of work.

use domovoi_core::conversation::{
    Conversation, ConversationScope, Message, MessageFailure, MessageRole, MessageStatus,
    MessageTool,
};
use domovoi_core::ports::{
    AggregateRef, CommitError, ConversationFilter, ConversationRepo, ConversationWrite,
    Expectation, Expected, MessageGuard, Observed, Page, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{ConversationId, HostId, OperationId, TurnId};

use super::ContractResult;
use crate::build::at;

fn conversation(scope: ConversationScope, secs: i64) -> Conversation {
    Conversation {
        id: ConversationId::new(),
        title: "Why is it slow?".to_owned(),
        scope,
        operation: None,
        last_ordinal: 0,
        created_at: at(secs),
        updated_at: at(secs),
    }
}

async fn commit(uow: &dyn UnitOfWork, writes: Vec<ConversationWrite>) -> Result<(), CommitError> {
    let mut tx = Transaction::new();
    for write in writes {
        tx = tx.write(StateWrite::Conversation(write));
    }
    uow.commit(tx).await.map(|_| ())
}

fn create(c: &Conversation) -> ConversationWrite {
    ConversationWrite::Conversation {
        conversation: c.clone(),
        expected: Expected::New,
    }
}

fn is_status_conflict(result: Result<(), CommitError>, found: Observed) -> bool {
    matches!(
        result,
        Err(CommitError::Conflict { found: f, expected: Expectation::Status(_), .. }) if f == found
    )
}

/// A question and its pending answer on `c`, at ordinals `first` and `first + 1`.
fn exchange(c: &Conversation, first: u32, secs: i64) -> (Message, Message) {
    let turn = TurnId::new();
    (
        Message::user(c.id, first, turn, format!("question {first}"), at(secs)),
        Message::pending_answer(c.id, first + 1, turn, at(secs)),
    )
}

fn message_write(message: &Message, guard: MessageGuard) -> ConversationWrite {
    ConversationWrite::Message {
        message: message.clone(),
        guard,
    }
}

/// Exercises creation, listing, paging, messages, guards, versions and deletion on an empty
/// store that is both the unit of work and the conversation repository.
pub async fn conversation_repo_contract<S>(store: &S) -> ContractResult
where
    S: UnitOfWork + ConversationRepo,
{
    let host = HostId::new();
    let operation = OperationId::new();
    let global = conversation(ConversationScope::Global, 10);
    let mut on_host = conversation(ConversationScope::Host(host), 20);
    on_host.operation = Some(operation);
    commit(store, vec![create(&global)]).await?;
    commit(store, vec![create(&on_host)]).await?;
    assert!(
        commit(store, vec![create(&global)]).await.is_err(),
        "New over existing"
    );

    let got = store.get(global.id).await?.map(|v| (v.value, v.version));
    assert_eq!(got, Some((global.clone(), 1)));
    assert_eq!(store.get(ConversationId::new()).await?, None);

    // Listing: newest updated first, filters, paging.
    let all = store
        .list(&ConversationFilter::default(), &Page::first(10))
        .await?;
    let ids: Vec<_> = all.items.iter().map(|r| r.conversation.id).collect();
    assert_eq!(ids, vec![on_host.id, global.id]);
    assert!(
        all.items
            .iter()
            .all(|r| r.message_count == 0 && r.version == 1)
    );
    let by_host = ConversationFilter {
        host: Some(host),
        operation: None,
    };
    assert_eq!(store.list(&by_host, &Page::first(10)).await?.items.len(), 1);
    let by_op = ConversationFilter {
        host: None,
        operation: Some(operation),
    };
    assert_eq!(
        store.list(&by_op, &Page::first(10)).await?.items[0]
            .conversation
            .id,
        on_host.id
    );
    let first = store
        .list(&ConversationFilter::default(), &Page::first(1))
        .await?;
    assert_eq!(first.items.len(), 1);
    let cursor = first
        .next_cursor
        .ok_or_else(|| super::ContractError::Setup("no cursor".into()))?;
    let second = store
        .list(
            &ConversationFilter::default(),
            &Page {
                cursor: Some(cursor),
                limit: 1,
            },
        )
        .await?;
    assert_eq!(second.items[0].conversation.id, global.id);
    assert_eq!(second.next_cursor, None);

    // A question: the conversation and both messages together.
    let (user, pending) = exchange(&global, 1, 30);
    let mut after_send = global.clone();
    after_send.last_ordinal = 2;
    after_send.updated_at = at(30);
    commit(
        store,
        vec![
            ConversationWrite::Conversation {
                conversation: after_send.clone(),
                expected: Expected::Version(1),
            },
            message_write(&user, MessageGuard::New),
            message_write(&pending, MessageGuard::New),
        ],
    )
    .await?;
    assert_eq!(
        store.get(global.id).await?.map(|v| v.version),
        Some(2),
        "version bump"
    );
    assert_eq!(
        store.messages(global.id).await?,
        vec![user.clone(), pending.clone()]
    );
    assert_eq!(store.message(pending.id).await?, Some(pending.clone()));
    assert_eq!(
        store
            .by_turn(global.id, user.turn, MessageRole::Assistant)
            .await?,
        Some(pending.clone())
    );
    assert_eq!(
        store
            .by_turn(global.id, TurnId::new(), MessageRole::User)
            .await?,
        None
    );
    assert_eq!(store.pending().await?, vec![pending.clone()]);
    let row = store
        .list(&ConversationFilter::default(), &Page::first(10))
        .await?;
    assert_eq!(
        row.items[0].conversation.id, global.id,
        "updated_at moved it first"
    );
    assert_eq!(row.items[0].message_count, 2);

    // Stale conversation version conflicts.
    let stale = commit(
        store,
        vec![ConversationWrite::Conversation {
            conversation: after_send.clone(),
            expected: Expected::Version(1),
        }],
    )
    .await;
    assert!(matches!(stale, Err(CommitError::Conflict { .. })));

    // A second pending answer in the same conversation is refused.
    let (_, other_pending) = exchange(&global, 3, 31);
    let second_pending = commit(
        store,
        vec![message_write(&other_pending, MessageGuard::New)],
    )
    .await;
    assert!(
        matches!(second_pending, Err(CommitError::Constraint(_))),
        "one pending per conversation"
    );

    // Settle: the guard holds once; a late settle conflicts with the stored status.
    let mut done = pending.clone();
    done.status = MessageStatus::Complete;
    done.text = "Two restarts.".to_owned();
    done.tools = vec![MessageTool {
        tool: "journal".to_owned(),
        failed: false,
    }];
    done.completed_at = Some(at(40));
    commit(
        store,
        vec![message_write(
            &done,
            MessageGuard::Status(MessageStatus::Pending),
        )],
    )
    .await?;
    assert_eq!(store.message(done.id).await?, Some(done.clone()));
    let late = commit(
        store,
        vec![message_write(
            &done,
            MessageGuard::Status(MessageStatus::Pending),
        )],
    )
    .await;
    assert!(is_status_conflict(
        late,
        Observed::Status("complete".to_owned())
    ));
    assert!(store.pending().await?.is_empty());

    // A failed message keeps its failure and is never part of the history.
    let (user2, pending2) = exchange(&global, 3, 50);
    commit(
        store,
        vec![
            message_write(&user2, MessageGuard::New),
            message_write(&pending2, MessageGuard::New),
        ],
    )
    .await?;
    let mut failed = pending2.clone();
    failed.status = MessageStatus::Failed;
    failed.failure = Some(MessageFailure::BudgetSpent);
    failed.failure_detail = Some("spent".to_owned());
    failed.completed_at = Some(at(51));
    commit(
        store,
        vec![message_write(
            &failed,
            MessageGuard::Status(MessageStatus::Pending),
        )],
    )
    .await?;
    assert_eq!(store.message(failed.id).await?, Some(failed.clone()));

    // History: only Complete messages, oldest first, `before` and `limit` honoured.
    let history = store.recent_complete(global.id, 10, 10).await?;
    assert_eq!(history, vec![user.clone(), done.clone(), user2.clone()]);
    assert_eq!(
        store.recent_complete(global.id, 3, 10).await?,
        vec![user.clone(), done.clone()]
    );
    assert_eq!(
        store.recent_complete(global.id, 10, 2).await?,
        vec![done.clone(), user2.clone()]
    );

    // Ordinals and turn/role pairs are unique.
    let mut duplicate = Message::user(global.id, 1, TurnId::new(), "again".to_owned(), at(60));
    assert!(matches!(
        commit(store, vec![message_write(&duplicate, MessageGuard::New)]).await,
        Err(CommitError::Constraint(_))
    ));
    duplicate.ordinal = 9;
    duplicate.turn = user.turn;
    assert!(matches!(
        commit(store, vec![message_write(&duplicate, MessageGuard::New)]).await,
        Err(CommitError::Constraint(_))
    ));

    // A message needs its conversation.
    let orphan = Message::user(
        ConversationId::new(),
        1,
        TurnId::new(),
        "x".to_owned(),
        at(61),
    );
    assert!(
        commit(store, vec![message_write(&orphan, MessageGuard::New)])
            .await
            .is_err()
    );

    // Delete: a stale version conflicts; the right one removes the messages too.
    let stale_delete = commit(
        store,
        vec![ConversationWrite::Delete {
            id: global.id,
            expected: 1,
        }],
    )
    .await;
    assert!(matches!(stale_delete, Err(CommitError::Conflict { .. })));
    commit(
        store,
        vec![ConversationWrite::Delete {
            id: global.id,
            expected: 2,
        }],
    )
    .await?;
    assert_eq!(store.get(global.id).await?, None);
    assert!(store.messages(global.id).await?.is_empty());
    assert_eq!(store.message(user.id).await?, None);
    let gone = commit(
        store,
        vec![ConversationWrite::Delete {
            id: global.id,
            expected: 2,
        }],
    )
    .await;
    assert!(matches!(
        gone,
        Err(CommitError::Conflict { aggregate, found: Observed::Missing, .. })
            if aggregate == AggregateRef::conversation(global.id)
    ));
    Ok(())
}
