//! Reconciling what a hub dispatched but could not see finish: the session tells the action
//! service when an agent authenticates, and the link carries the journal's epoch and its
//! answers about each run. Nothing is reconciled for a caller that has not proved its key.

use std::sync::Arc;

use domovoi_core::ports::{ActionAnswer, AgentLink, Clock, UnknownReason};
use domovoi_core::{ActionId, HostId, JournalEpoch};
use domovoi_proto::messages::{
    ActionState, ActionStatusAnswer, ActionStatusAnswers, CapabilityOutcome, CapabilityResult,
    OutcomeUnknownReason, RejectReason,
};
use domovoi_proto::{Message, Open, PROTOCOL_VERSION, ProtocolVersion};
use domovoi_testkit::RecordingReconciler;
use jiff::SignedDuration;

use crate::agent::{FakeAgent, Ident, enroll, hello_with_epoch, online, rejected};
use crate::harness::{Hub, guard};

/// Authenticates `ident` on a fresh connection announcing journal `epoch`, and returns the
/// connection.
async fn authenticate_with(hub: &Hub, ident: &Ident, epoch: uuid::Uuid) -> FakeAgent {
    let mut agent = FakeAgent::connect(hub.addr).await;
    agent
        .send(hello_with_epoch(ident.agent, PROTOCOL_VERSION, epoch), None)
        .await;
    let Message::Challenge(challenge) = agent.expect().await.message else {
        panic!("expected a challenge");
    };
    assert!(matches!(
        agent.answer(&challenge, ident.agent, &ident.key).await,
        Message::Welcome(_)
    ));
    agent
}

#[tokio::test]
async fn authenticated_hello_starts_reconciliation() {
    let hub = Hub::start().await;
    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());

    let (ident, _agent) = online(&hub, "nas").await;

    let asked = guard(reconciler.wait_for(1)).await;
    assert_eq!(
        asked,
        vec![ident.host],
        "the host whose agent authenticated is the one reconciled",
    );
}

#[tokio::test]
async fn unauthenticated_hello_reconciles_nothing() {
    let hub = Hub::start().await;
    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());
    let ident = enroll(&hub, "nas").await;

    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = agent.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer_without_the_key(&challenge, ident.agent).await;
    assert!(
        rejected(&reply).is_some(),
        "a signature from the wrong key is rejected: {reply:?}",
    );

    assert!(
        reconciler.hosts().is_empty(),
        "nothing is read or written for a caller that did not prove its key",
    );
}

#[tokio::test]
async fn incompatible_agent_keeps_dispatched_action() {
    let hub = Hub::start().await;
    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());
    let ident = enroll(&hub, "nas").await;

    let mut agent = FakeAgent::connect(hub.addr).await;
    let old = ProtocolVersion { major: 1, minor: 0 };
    let Message::Challenge(challenge) = agent.hello(ident.agent, old).await else {
        panic!("the hub asks every caller to prove its key first");
    };
    let reply = agent.answer(&challenge, ident.agent, &ident.key).await;
    assert!(
        matches!(rejected(&reply), Some(RejectReason::Incompatible { .. })),
        "an agent on another protocol is refused: {reply:?}",
    );

    assert!(
        reconciler.hosts().is_empty(),
        "a rejected agent has no session to reconcile against",
    );
    assert_eq!(
        hub.server.link().journal_epoch(ident.host),
        None,
        "with no live session there is no journal epoch, so the run stays dispatched",
    );
}

#[tokio::test]
async fn duplicate_hello_reconciles_each_time() {
    // A reconnect, or a second session replacing the first, is a fresh idempotent pass: the
    // runs still dispatched are asked about again.
    let hub = Hub::start().await;
    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());
    let ident = enroll(&hub, "nas").await;

    let _first = authenticate_with(&hub, &ident, uuid::Uuid::now_v7()).await;
    guard(reconciler.wait_for(1)).await;
    let _second = authenticate_with(&hub, &ident, uuid::Uuid::now_v7()).await;

    let asked = guard(reconciler.wait_for(2)).await;
    assert_eq!(
        asked,
        vec![ident.host, ident.host],
        "every authenticated hello reconciles that host again",
    );
}

#[tokio::test]
async fn agent_that_never_reconnects_keeps_dispatched_action() {
    // No session means no epoch and no answer: the pass cannot start, and nothing invents
    // one for a host that is merely known.
    let hub = Hub::start().await;
    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());
    let (ident, _token) = hub.token("nas").await;

    hub.clock.advance(SignedDuration::from_hours(2));
    guard(async {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    })
    .await;

    assert!(
        reconciler.hosts().is_empty(),
        "a host that never reconnects is never reconciled",
    );
    assert_eq!(hub.server.link().journal_epoch(ident), None);
}

#[tokio::test]
async fn a_host_already_connected_is_reconciled_when_the_action_service_arrives() {
    // The agent server starts before the action service, so an agent can authenticate in
    // between: its dispatched runs must still be settled.
    let hub = Hub::start().await;
    let (ident, _agent) = online(&hub, "nas").await;

    let reconciler = Arc::new(RecordingReconciler::new());
    hub.server.set_reconciler(reconciler.clone());

    let asked = guard(reconciler.wait_for(1)).await;
    assert_eq!(
        asked,
        vec![ident.host],
        "a host that was live before the service existed is reconciled too",
    );
}

#[tokio::test]
async fn the_link_reports_the_journal_epoch_the_agent_announced() {
    let hub = Hub::start().await;
    let link = hub.server.link();
    let epoch = uuid::Uuid::now_v7();
    let ident = enroll(&hub, "nas").await;
    let _agent = authenticate_with(&hub, &ident, epoch).await;

    assert_eq!(
        link.journal_epoch(ident.host),
        Some(JournalEpoch::from_uuid(epoch)),
        "the epoch comes from the agent's authenticated hello",
    );
    assert_eq!(
        link.journal_epoch(HostId::new()),
        None,
        "a host with no session has no journal to ask",
    );
}

#[tokio::test]
async fn the_link_asks_the_journal_about_each_run_and_maps_the_answers() {
    let hub = Hub::start().await;
    let link = hub.server.link();
    let epoch = uuid::Uuid::now_v7();
    let ident = enroll(&hub, "nas").await;
    let mut agent = authenticate_with(&hub, &ident, epoch).await;

    let finished = ActionId::new();
    let failed = ActionId::new();
    let missing = ActionId::new();
    let unprovable = ActionId::new();
    let asked = vec![finished, failed, missing, unprovable];
    let deadline = hub.clock.now() + SignedDuration::from_secs(10);
    let waiting = tokio::spawn({
        let link = link.clone();
        let asked = asked.clone();
        async move {
            link.action_status(ident.host, asked, JournalEpoch::from_uuid(epoch), deadline)
                .await
        }
    });

    let request = agent.expect().await;
    let Message::ActionStatus(query) = request.message else {
        panic!("expected action_status");
    };
    assert_eq!(
        query.execution_ids,
        asked.iter().map(ActionId::as_uuid).collect::<Vec<_>>(),
        "the hub asks about exactly the runs it could not settle",
    );
    assert_eq!(query.expected_epoch, epoch);

    let answers = ActionStatusAnswers {
        epoch,
        answers: vec![
            ActionStatusAnswer {
                execution_id: finished.as_uuid(),
                state: ActionState::Finished {
                    result: CapabilityResult {
                        outcome: CapabilityOutcome::Succeeded {
                            outputs: Default::default(),
                        },
                    },
                },
            },
            ActionStatusAnswer {
                execution_id: failed.as_uuid(),
                state: ActionState::Finished {
                    result: CapabilityResult {
                        outcome: CapabilityOutcome::Failed {
                            code: Open::new("exited_non_zero", None),
                            message: "exit status 1".to_owned(),
                        },
                    },
                },
            },
            ActionStatusAnswer {
                execution_id: missing.as_uuid(),
                state: ActionState::NotFound,
            },
            ActionStatusAnswer {
                execution_id: unprovable.as_uuid(),
                state: ActionState::OutcomeUnknown {
                    reason: Open::new("no_start_record", Some(OutcomeUnknownReason::NoStartRecord)),
                },
            },
        ],
    };
    agent
        .send(Message::ActionStatusAnswers(answers), Some(request.id))
        .await;

    let got = guard(waiting).await.unwrap().unwrap();
    assert_eq!(got.epoch, JournalEpoch::from_uuid(epoch));
    let settled: Vec<(ActionId, ActionAnswer)> = got
        .answers
        .into_iter()
        .map(|entry| (entry.execution, entry.answer))
        .collect();
    assert_eq!(
        settled,
        vec![
            (
                finished,
                ActionAnswer::Finished {
                    succeeded: true,
                    detail: None,
                },
            ),
            (
                failed,
                ActionAnswer::Finished {
                    succeeded: false,
                    detail: Some("exit status 1".to_owned()),
                },
            ),
            (missing, ActionAnswer::NotFound),
            (
                unprovable,
                ActionAnswer::Unknown {
                    reason: UnknownReason::NoStartRecord,
                },
            ),
        ],
        "each journal state reaches the engine as the fact it is",
    );
}

#[tokio::test]
async fn an_unrecognised_journal_reason_is_treated_as_a_lost_record() {
    let hub = Hub::start().await;
    let link = hub.server.link();
    let epoch = uuid::Uuid::now_v7();
    let ident = enroll(&hub, "nas").await;
    let mut agent = authenticate_with(&hub, &ident, epoch).await;

    let run = ActionId::new();
    let deadline = hub.clock.now() + SignedDuration::from_secs(10);
    let waiting = tokio::spawn({
        let link = link.clone();
        async move {
            link.action_status(
                ident.host,
                vec![run],
                JournalEpoch::from_uuid(epoch),
                deadline,
            )
            .await
        }
    });
    let request = agent.expect().await;
    agent
        .send(
            Message::ActionStatusAnswers(ActionStatusAnswers {
                epoch,
                answers: vec![ActionStatusAnswer {
                    execution_id: run.as_uuid(),
                    state: ActionState::OutcomeUnknown {
                        reason: Open::new("a_reason_from_a_later_agent", None),
                    },
                }],
            }),
            Some(request.id),
        )
        .await;

    let got = guard(waiting).await.unwrap().unwrap();
    assert_eq!(
        got.answers.first().map(|entry| entry.answer.clone()),
        Some(ActionAnswer::Unknown {
            reason: UnknownReason::JournalLost,
        }),
        "a reason this build does not know assumes the record is lost, never that it ran",
    );
}
