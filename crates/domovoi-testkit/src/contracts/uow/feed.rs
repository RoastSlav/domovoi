//! Unit-of-work contracts about the journal as a change stream.

use domovoi_core::Actor;
use domovoi_core::IncidentId;
use domovoi_core::ports::{AggregateRef, JournalDraft, JournalKind, Transaction};
use futures::FutureExt;
use serde_json::json;

use super::super::ContractResult;
use super::UowSubject;
use crate::build::at;

fn row() -> JournalDraft {
    JournalDraft {
        aggregate: AggregateRef::incident(IncidentId::new()),
        kind: JournalKind::IncidentTimeline,
        payload_v: 1,
        payload: json!({"kind": "x", "summary": "y", "detail": null}),
        actor: Actor::System("test".to_owned()),
        occurred_at: at(1),
    }
}

/// The floor starts at zero, and `wait_after` is pending until the head passes its argument.
pub async fn floor_starts_at_zero_and_wait_after_follows_the_committed_head(
    subject: &dyn UowSubject,
) -> ContractResult {
    assert_eq!(subject.feed().floor_seq().await?, 0);
    let head = subject.feed().head_seq().await?;
    assert!(subject.feed().wait_after(head).now_or_never().is_none());
    subject
        .uow()
        .commit(Transaction::new().journal(row()))
        .await?;
    assert!(subject.feed().wait_after(head).now_or_never().is_some());
    let new_head = subject.feed().head_seq().await?;
    assert!(new_head > head);
    assert!(subject.feed().wait_after(new_head).now_or_never().is_none());
    assert!(subject.feed().wait_after(0).now_or_never().is_some());
    Ok(())
}

/// The head equals the highest sequence number the last commit returned.
pub async fn head_equals_the_highest_receipt_seq(subject: &dyn UowSubject) -> ContractResult {
    let receipt = subject
        .uow()
        .commit(Transaction::new().journal(row()).journal(row()))
        .await?;
    let highest = receipt.journal_seqs.iter().copied().max().unwrap_or(0);
    assert_eq!(subject.feed().head_seq().await?, highest);
    Ok(())
}
