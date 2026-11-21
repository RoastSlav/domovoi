//! The package half of the production link, over the test transport: frames stream through the
//! session outbox in order without a "busy" refusal, `End` waits for the reply to its own frame,
//! and a session change aborts. The fake clock and pending futures decide every ordering.

use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::{AgentLink, LinkError};
use domovoi_core::{
    ExecutionId, JournalEpoch, PackageCall, PackageDispatch, PlanHash, TransferAnswer,
    TransferFrame, TransferKind,
};
use domovoi_proto::messages::{
    AuthorizationKind, CapabilityFailureCode, CapabilityOutcome, CapabilityResult, ErrorMessage,
    RefusalCode, Refused, TransferFailureCode, TransferOutcome, TransferResult,
};
use domovoi_proto::{Decoded, Message, Open, decode};

use super::*;

/// The chunk size the transfer uses.
const CHUNK: usize = 65_536;

fn plan_hash() -> PlanHash {
    "ab".repeat(32).parse().unwrap()
}

fn uninstall_request() -> PackageDispatch {
    PackageDispatch {
        execution: ExecutionId::new(),
        call: PackageCall::Uninstall {
            package: PackageName::parse("tools").unwrap(),
            version: PackageVersion::parse("1.2.0").unwrap(),
        },
        plan_hash: plan_hash(),
        expires_at: later(600),
    }
}

fn chunk(transfer: Uuid, seq: u64) -> TransferFrame {
    TransferFrame::Chunk {
        transfer,
        seq,
        bytes: vec![seq as u8; CHUNK],
    }
}

/// A host with a live session over an outbox of `slots`, and the session's identity.
struct Session {
    world: World,
    outbox: mpsc::Receiver<String>,
    identity: AgentSessionId,
    session: SessionId,
}

fn session(slots: usize) -> Session {
    let world = world(Arc::new(InMemoryStore::new()));
    let (tx, outbox) = mpsc::channel(slots);
    let (live, session) = live_session(tx);
    let identity = live.identity;
    world.registry.register(world.host, live, None).unwrap();
    Session {
        world,
        outbox,
        identity,
        session,
    }
}

fn decoded(frame: &str) -> Decoded {
    decode(frame).unwrap()
}

#[tokio::test]
async fn transfer_streams_320_chunks_without_busy() {
    let mut s = session(256);
    let transfer = Uuid::now_v7();
    let (link, host, identity) = (s.world.link.clone(), s.world.host, s.identity);
    let task = tokio::spawn(async move {
        let begin = TransferFrame::Begin {
            transfer,
            kind: TransferKind::PackageArtifact,
            size: 320 * CHUNK as u64,
            sha256: [7; 32],
        };
        link.send_transfer(host, begin, identity, later(3600))
            .await?;
        for seq in 0..320 {
            link.send_transfer(host, chunk(transfer, seq), identity, later(3600))
                .await?;
        }
        Ok::<(), LinkError>(())
    });
    settle().await;
    assert!(!task.is_finished(), "the 257th frame waits for room");

    let mut seqs = Vec::new();
    let mut bytes = 0usize;
    let mut frames = 0usize;
    while seqs.len() < 320 {
        let frame = s.outbox.recv().await.expect("the sender is still going");
        frames += 1;
        match decoded(&frame).message {
            Message::TransferBegin(begin) => assert_eq!(begin.transfer_id, transfer),
            Message::TransferChunk(chunk) => {
                assert_eq!(chunk.transfer_id, transfer);
                bytes += chunk.bytes.as_bytes().len();
                seqs.push(chunk.seq);
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    task.await.unwrap().unwrap();
    assert_eq!(frames, 321);
    assert_eq!(seqs, (0..320).collect::<Vec<u64>>(), "ordered and gap free");
    assert_eq!(bytes, 20 * 1024 * 1024);
    assert_eq!(s.world.registry.pending_len(s.world.host), 0);
}

#[tokio::test]
async fn transfer_frames_for_one_transfer_stay_ordered() {
    let mut s = session(8);
    let transfer = Uuid::now_v7();
    let (link, host, identity) = (s.world.link.clone(), s.world.host, s.identity);
    let task = tokio::spawn(async move {
        let begin = TransferFrame::Begin {
            transfer,
            kind: TransferKind::PackageManifest,
            size: 3,
            sha256: [1; 32],
        };
        let frames = [begin, chunk(transfer, 0), chunk(transfer, 1)];
        for frame in frames {
            link.send_transfer(host, frame, identity, later(60)).await?;
        }
        link.send_transfer(host, TransferFrame::End { transfer }, identity, later(60))
            .await
    });
    let mut kinds = Vec::new();
    let mut end_id = None;
    for _ in 0..4 {
        let frame = decoded(&s.outbox.recv().await.unwrap());
        kinds.push(frame.message.kind().to_owned());
        end_id = Some(frame.id);
    }
    assert_eq!(
        kinds,
        [
            "transfer_begin",
            "transfer_chunk",
            "transfer_chunk",
            "transfer_end"
        ]
    );
    let reply = Message::TransferResult(TransferResult {
        transfer_id: transfer,
        outcome: TransferOutcome::Stored,
    });
    assert!(
        s.world
            .registry
            .deliver(s.world.host, s.session, end_id.unwrap(), reply)
    );
    assert_eq!(
        task.await.unwrap().unwrap(),
        Some(TransferAnswer::Stored),
        "only End has an answer"
    );
}

/// Starts an `End` for `transfer` and returns its task and the id of the frame it queued.
async fn start_end(
    s: &mut Session,
    transfer: Uuid,
    deadline_secs: i64,
) -> (
    tokio::task::JoinHandle<Result<Option<TransferAnswer>, LinkError>>,
    domovoi_core::RequestId,
) {
    let (link, host, identity) = (s.world.link.clone(), s.world.host, s.identity);
    let task = tokio::spawn(async move {
        link.send_transfer(
            host,
            TransferFrame::End { transfer },
            identity,
            later(deadline_secs),
        )
        .await
    });
    let id = decoded(&s.outbox.recv().await.unwrap()).id;
    (task, id)
}

#[tokio::test]
async fn transfer_end_waits_for_the_reply_to_that_frame() {
    let mut s = session(4);
    let transfer = Uuid::now_v7();
    let (task, end_id) = start_end(&mut s, transfer, 60).await;
    assert_eq!(s.world.registry.pending_len(s.world.host), 1);

    // An answer to some other request is not the answer to this frame.
    let stray = Message::TransferResult(TransferResult {
        transfer_id: transfer,
        outcome: TransferOutcome::Stored,
    });
    let other = domovoi_core::RequestId::new();
    assert!(
        !s.world
            .registry
            .deliver(s.world.host, s.session, other, stray)
    );
    settle().await;
    assert!(!task.is_finished());

    let failed = Message::TransferResult(TransferResult {
        transfer_id: transfer,
        outcome: TransferOutcome::Failed {
            code: Open::new(
                "checksum_mismatch",
                Some(TransferFailureCode::ChecksumMismatch),
            ),
            detail: "hash differs".to_owned(),
        },
    });
    assert!(
        s.world
            .registry
            .deliver(s.world.host, s.session, end_id, failed)
    );
    assert_eq!(
        task.await.unwrap().unwrap(),
        Some(TransferAnswer::Failed {
            code: "checksum_mismatch".to_owned(),
            detail: "hash differs".to_owned()
        })
    );
    assert_eq!(s.world.registry.pending_len(s.world.host), 0);
}

#[tokio::test]
async fn transfer_end_maps_a_refusal_or_an_error_to_rejected() {
    let mut s = session(4);
    let refused = Message::Refused(Refused {
        code: Open::new("rate_limited", Some(RefusalCode::RateLimited)),
        detail: "slow down".to_owned(),
        retry_after: None,
        scope: None,
    });
    let error = Message::Error(ErrorMessage {
        code: "invalid_payload".to_owned(),
        message: "bad chunk".to_owned(),
    });
    for (reply, expected) in [
        (refused, "rate_limited: slow down"),
        (error, "invalid_payload: bad chunk"),
    ] {
        let (task, id) = start_end(&mut s, Uuid::now_v7(), 60).await;
        assert!(s.world.registry.deliver(s.world.host, s.session, id, reply));
        assert_eq!(
            task.await.unwrap().unwrap_err(),
            LinkError::Rejected(expected.to_owned())
        );
    }
}

#[tokio::test]
async fn transfer_aborts_on_session_change() {
    // A call for a session that is not the live one queues nothing.
    let mut s = session(4);
    let stale = AgentSessionId::new();
    let result = s
        .world
        .link
        .send_transfer(s.world.host, chunk(Uuid::now_v7(), 0), stale, later(60))
        .await;
    assert_eq!(result.unwrap_err(), LinkError::SessionChanged);
    assert!(s.outbox.try_recv().is_err(), "nothing was queued");

    // A session replaced while a frame waits for room never receives it.
    let mut s = session(1);
    let transfer = Uuid::now_v7();
    let (link, host, identity) = (s.world.link.clone(), s.world.host, s.identity);
    link.send_transfer(host, chunk(transfer, 0), identity, later(60))
        .await
        .unwrap();
    let task = {
        let link = link.clone();
        tokio::spawn(async move {
            link.send_transfer(host, chunk(transfer, 1), identity, later(60))
                .await
        })
    };
    settle().await;
    assert!(!task.is_finished(), "the outbox is full");

    let (tx, mut replacement) = mpsc::channel(4);
    let (live, _) = live_session(tx);
    s.world.registry.register(host, live, None).unwrap();
    // Room opens in the old session's queue; the frame must not go anywhere.
    let _first = s.outbox.recv().await.unwrap();
    assert_eq!(task.await.unwrap().unwrap_err(), LinkError::SessionChanged);
    assert!(replacement.try_recv().is_err());
    assert!(s.outbox.try_recv().is_err());
}

#[tokio::test]
async fn transfer_end_times_out_with_the_fake_clock() {
    let mut s = session(4);
    let (task, _id) = start_end(&mut s, Uuid::now_v7(), 5).await;
    assert_eq!(s.world.registry.pending_len(s.world.host), 1);
    s.world.clock.advance(SignedDuration::from_secs(10));
    let err = task.await.unwrap().unwrap_err();
    assert!(
        matches!(err, LinkError::Indeterminate(_)),
        "the agent may have stored it: {err:?}"
    );
    assert_eq!(s.world.registry.pending_len(s.world.host), 0);

    // A frame that cannot get room before its deadline is a plain timeout.
    let mut s = session(1);
    let (link, host, identity) = (s.world.link.clone(), s.world.host, s.identity);
    link.send_transfer(host, chunk(Uuid::now_v7(), 0), identity, later(5))
        .await
        .unwrap();
    let waiting = tokio::spawn(async move {
        link.send_transfer(host, chunk(Uuid::now_v7(), 1), identity, later(5))
            .await
    });
    settle().await;
    s.world.clock.advance(SignedDuration::from_secs(10));
    assert_eq!(waiting.await.unwrap().unwrap_err(), LinkError::Timeout);
    assert!(s.outbox.try_recv().is_ok());
    assert!(s.outbox.try_recv().is_err());
}

#[tokio::test]
async fn start_capability_fails_fast_when_the_session_changed() {
    let mut s = session(4);
    let (host, identity) = (s.world.host, s.identity);
    let epoch = JournalEpoch::from_uuid(Uuid::nil());

    let stale = s
        .world
        .link
        .start_capability(
            host,
            uninstall_request(),
            AgentSessionId::new(),
            epoch,
            later(60),
        )
        .await;
    assert_eq!(stale.unwrap_err(), LinkError::SessionChanged);
    let other_epoch = JournalEpoch::from_uuid(Uuid::now_v7());
    let moved = s
        .world
        .link
        .start_capability(host, uninstall_request(), identity, other_epoch, later(60))
        .await;
    assert_eq!(moved.unwrap_err(), LinkError::SessionChanged);
    assert!(s.outbox.try_recv().is_err(), "nothing was queued");

    let req = uninstall_request();
    let handoff = s
        .world
        .link
        .start_capability(host, req.clone(), identity, epoch, later(60))
        .await
        .unwrap();
    let frame = decoded(&s.outbox.try_recv().unwrap());
    assert_eq!(frame.id, handoff.request());
    let Message::RunCapability(run) = frame.message else {
        panic!("a run_capability frame was queued");
    };
    assert_eq!(run.contract, "agent.package.uninstall");
    assert_eq!(run.version, 1);
    assert_eq!(run.target.as_str(), "package:tools");
    assert_eq!(run.authorization_kind, AuthorizationKind::Human);
    assert_eq!(run.execution_id, req.execution.as_uuid());
    assert_eq!(run.plan_hash, req.plan_hash);
    assert_eq!(run.expires_at, req.expires_at);
    assert!(run.expected_safety.is_none());
    let args: Vec<(&str, &str)> = run
        .args
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(args, [("package", "tools"), ("version", "1.2.0")]);
}

#[tokio::test]
async fn capability_result_maps_failed_refused_and_in_progress() {
    let mut s = session(8);
    let (host, identity) = (s.world.host, s.identity);
    let epoch = JournalEpoch::from_uuid(Uuid::nil());

    let mut outputs = BTreeMap::new();
    outputs.insert("removed".to_owned(), "true".to_owned());
    let replies = [
        (
            Message::CapabilityResult(CapabilityResult {
                outcome: CapabilityOutcome::Succeeded {
                    outputs: outputs.clone(),
                },
            }),
            Ok(domovoi_core::PackageOutputs(outputs)),
        ),
        (
            Message::CapabilityResult(CapabilityResult {
                outcome: CapabilityOutcome::Failed {
                    code: Open::new("refused", Some(CapabilityFailureCode::Refused)),
                    message: "not_installed: nothing to remove".to_owned(),
                },
            }),
            Err(LinkError::Rejected(
                "not_installed: nothing to remove".to_owned(),
            )),
        ),
        (
            Message::Refused(Refused {
                code: Open::new("contract_unknown", Some(RefusalCode::ContractUnknown)),
                detail: "no such contract".to_owned(),
                retry_after: None,
                scope: None,
            }),
            Err(LinkError::Rejected(
                "contract_unknown: no such contract".to_owned(),
            )),
        ),
        (
            Message::Error(ErrorMessage {
                code: "in_progress".to_owned(),
                message: "still running".to_owned(),
            }),
            Err(LinkError::Indeterminate(
                "in_progress: still running".to_owned(),
            )),
        ),
        (
            Message::Error(ErrorMessage {
                code: "outcome_unknown".to_owned(),
                message: "journal lost".to_owned(),
            }),
            Err(LinkError::Indeterminate(
                "outcome_unknown: journal lost".to_owned(),
            )),
        ),
    ];
    for (reply, expected) in replies {
        let handoff = s
            .world
            .link
            .start_capability(host, uninstall_request(), identity, epoch, later(60))
            .await
            .unwrap();
        let _sent = s.outbox.recv().await.unwrap();
        let (link, id) = (s.world.link.clone(), handoff);
        let task = tokio::spawn(async move { link.capability_result(host, id, later(60)).await });
        settle().await;
        assert!(
            s.world
                .registry
                .deliver(host, s.session, handoff.request(), reply)
        );
        assert_eq!(task.await.unwrap(), expected);
        assert_eq!(s.world.registry.pending_len(host), 0);
    }

    // No answer by the deadline is a timeout; a result nobody awaits is indeterminate.
    let handoff = s
        .world
        .link
        .start_capability(host, uninstall_request(), identity, epoch, later(60))
        .await
        .unwrap();
    let (link, id) = (s.world.link.clone(), handoff);
    let task = tokio::spawn(async move { link.capability_result(host, id, later(5)).await });
    settle().await;
    s.world.clock.advance(SignedDuration::from_secs(10));
    assert_eq!(task.await.unwrap().unwrap_err(), LinkError::Timeout);
    let again = s
        .world
        .link
        .capability_result(host, handoff, later(60))
        .await;
    assert!(matches!(again, Err(LinkError::Indeterminate(_))));
}
