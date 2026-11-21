//! Unit tests of the transaction types.

use jiff::Timestamp;

use super::super::ClaimToken;
use super::*;

#[test]
fn aggregate_kinds_round_trip() {
    for kind in [
        AggregateKind::Incident,
        AggregateKind::Action,
        AggregateKind::Note,
        AggregateKind::Operation,
        AggregateKind::Policy,
        AggregateKind::Conversation,
    ] {
        assert_eq!(AggregateKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(AggregateKind::parse("host"), None);
    assert_eq!(
        AggregateKind::LEGACY,
        [
            AggregateKind::Incident,
            AggregateKind::Action,
            AggregateKind::Note
        ]
    );
}

#[test]
fn a_cancellation_alone_makes_a_transaction_non_empty() {
    let cancel = EffectCancel {
        aggregate: AggregateRef::operation(OperationId::new()),
        at: Timestamp::constant(1_700_000_000, 0),
    };
    assert!(!Transaction::new().cancel_effects(cancel).is_empty());
}

#[test]
fn aggregate_ref_displays_as_kind_and_id() {
    let id = IncidentId::from_uuid(uuid::Uuid::from_u128(7));
    assert_eq!(
        AggregateRef::incident(id).to_string(),
        format!("incident:{id}")
    );
}

#[test]
fn an_empty_transaction_reports_itself_empty() {
    assert!(Transaction::new().is_empty());
    assert!(
        !Transaction::default()
            .write(StateWrite::DiscardPendingIncident {
                id: IncidentId::new(),
                expected: 1,
            })
            .is_empty()
    );
}

#[test]
fn a_transaction_with_only_effect_lifecycle_rows_is_not_empty() {
    let token = ClaimToken::new();
    assert!(
        !Transaction::new()
            .reschedule(EffectReschedule {
                id: EffectId::new(),
                claim_token: token,
                not_before: Timestamp::constant(1_700_000_000, 0),
                error: "offline".to_owned(),
                at: Timestamp::constant(1_700_000_000, 0),
            })
            .is_empty()
    );
    assert!(
        !Transaction::new()
            .kill(EffectDeath {
                id: EffectId::new(),
                claim_token: token,
                error: "gave up".to_owned(),
                at: Timestamp::constant(1_700_000_000, 0),
            })
            .is_empty()
    );
}
