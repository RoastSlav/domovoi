//! Contract for notification state.

use domovoi_core::ports::NotifyStateRepo;
use domovoi_core::ports::{MessageRef, ThreadRef};
use domovoi_core::{ChatRefs, HeldItem, IncidentId, NotifyState, Severity};

use super::ContractResult;
use crate::build::at;

fn held(line: &str) -> NotifyState {
    NotifyState {
        held: Some(HeldItem {
            line: line.to_owned(),
            severity: Severity::Warning,
            held_at: at(1),
            resolved: false,
        }),
        ..NotifyState::default()
    }
}

/// Exercises storing, replacing, listing held and forgetting notification state on an
/// empty `repo`.
pub async fn notify_state_repo_contract(repo: &dyn NotifyStateRepo) -> ContractResult {
    let (a, b, c) = (IncidentId::new(), IncidentId::new(), IncidentId::new());
    assert_eq!(repo.get(a).await?, None);

    let mut posted = NotifyState::default();
    posted.messages.insert(
        "discord".to_owned(),
        ChatRefs {
            message: MessageRef {
                platform: "discord".to_owned(),
                id: "m1".to_owned(),
            },
            thread: Some(ThreadRef {
                platform: "discord".to_owned(),
                id: "t1".to_owned(),
            }),
        },
    );
    posted.first_alert_at = Some(at(5));
    posted.folds_since_reply = 3;
    repo.put(a, &posted, at(5)).await?;
    assert_eq!(repo.get(a).await?, Some(posted.clone()), "round trip");

    repo.put(b, &held("disk filling"), at(10)).await?;
    repo.put(c, &held("unit failed"), at(20)).await?;
    let listed: Vec<IncidentId> = repo
        .list_held()
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(listed, [c, b], "only held, most recently updated first");

    // Releasing replaces the state; it is no longer held.
    repo.put(b, &NotifyState::default(), at(30)).await?;
    let listed: Vec<IncidentId> = repo
        .list_held()
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(listed, [c]);

    repo.delete(c).await?;
    assert_eq!(repo.get(c).await?, None);
    repo.delete(c).await?;
    assert!(repo.list_held().await?.is_empty());
    Ok(())
}
