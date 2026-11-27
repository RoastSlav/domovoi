//! `agent.package.*` through `run_capability`: the order of refusals, the journal flow and
//! the catalogue the agent announces.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_agent::link::RequestKind;
use domovoi_agent::link::frame;
use domovoi_agent::packages::{PackageSurface, TracingEvents, TransferReceiver};
use domovoi_agent::requests::{catalog_entries, handle, serve};
use domovoi_config::MutationLimits;
use domovoi_core::capability::catalog_hash;
use domovoi_core::ports::Clock as _;
use domovoi_proto::messages::{
    ActionState, ActionStatusQuery, CapabilityOutcome, CapabilityResult, GetCatalog,
    OutcomeUnknownReason, RefusalCode, TransferBegin, TransferChunk, TransferEnd, TransferKind,
    TransferOutcome,
};
use domovoi_proto::{Base64Bytes, Message, Open, Sha256Bytes};
use domovoi_testkit::{BuiltPackage, PackageFixture};
use sha2::{Digest as _, Sha256};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::harness::{clock, identity, start_link_transfers};
use crate::hub::{TestHub, guard};
use crate::package_rig::{BOOT_ID, Options, Rig};

fn nginx() -> BuiltPackage {
    PackageFixture::new("nginx-tools", "1.2.0")
        .unwrap()
        .skill("nginx_reload")
        .build()
}

fn refusal(message: &Message) -> RefusalCode {
    match message {
        Message::Refused(refused) => *refused.code.known().expect("a known refusal code"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn result(message: Message) -> CapabilityResult {
    match message {
        Message::CapabilityResult(result) => result,
        other => panic!("expected a capability result, got {other:?}"),
    }
}

fn outputs(message: Message) -> BTreeMap<String, String> {
    match result(message).outcome {
        CapabilityOutcome::Succeeded { outputs } => outputs,
        CapabilityOutcome::Failed { message, .. } => panic!("the capability failed: {message}"),
    }
}

fn error_code(message: &Message) -> &str {
    match message {
        Message::Error(error) => &error.code,
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn package_install_end_to_end_over_the_link() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let mut hub = TestHub::start().await;
    let identity = identity(&hub);
    let clock = clock();
    let dir = tempfile::tempdir().unwrap();
    let staging = rig.service.as_ref().unwrap().staging().unwrap();
    let receiver = TransferReceiver::new(
        staging,
        true,
        20 * 1024 * 1024,
        clock.clone(),
        Arc::new(TracingEvents),
    );
    let (frames, incoming) = mpsc::channel(64);
    let link = start_link_transfers(&hub, &identity, &clock, &dir, frames);
    let receiving =
        tokio::spawn(receiver.serve(incoming, link.handle.clone(), link.shutdown.clone()));
    let ctx = Arc::new(rig.ctx);
    let requests = serve(
        ctx.clone(),
        link.requests,
        link.handle.clone(),
        link.shutdown.clone(),
    );
    let serving = tokio::spawn(requests);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;

    let mut ids = Vec::new();
    for (kind, name, bytes) in [
        (
            TransferKind::PackageArtifact,
            "package_artifact",
            built.archive.clone(),
        ),
        (
            TransferKind::PackageManifest,
            "package_manifest",
            built.sig_toml.clone().into_bytes(),
        ),
    ] {
        let id = Uuid::now_v7();
        ids.push(id);
        let begin = Message::TransferBegin(TransferBegin {
            transfer_id: id,
            kind: Open::new(name, Some(kind)),
            size: bytes.len() as u64,
            sha256: Sha256Bytes(Sha256::digest(&bytes).into()),
        });
        frame::send(conn.transport.as_mut(), &begin, None)
            .await
            .unwrap();
        for (seq, part) in bytes.chunks(32 * 1024).enumerate() {
            let chunk = Message::TransferChunk(TransferChunk {
                transfer_id: id,
                seq: seq as u64,
                bytes: Base64Bytes(part.to_vec()),
            });
            frame::send(conn.transport.as_mut(), &chunk, None)
                .await
                .unwrap();
        }
        let end = frame::send(
            conn.transport.as_mut(),
            &Message::TransferEnd(TransferEnd { transfer_id: id }),
            None,
        )
        .await
        .unwrap();
        let answer = guard(conn.expect()).await;
        assert_eq!(answer.reply_to, Some(end));
        let Message::TransferResult(stored) = answer.message else {
            panic!("expected a transfer result");
        };
        assert_eq!(stored.outcome, TransferOutcome::Stored);
    }

    let run = rig_run(&ids);
    let run_id = run.execution_id;
    let request = frame::send(conn.transport.as_mut(), &Message::RunCapability(run), None)
        .await
        .unwrap();
    let answer = guard(conn.expect()).await;
    assert_eq!(answer.reply_to, Some(request));
    let out = outputs(answer.message);
    assert_eq!(out["package"], "nginx-tools");
    assert_eq!(out["enabled"], "false");
    assert_eq!(out["key_id"], built.key_id.to_string());
    assert!(
        ctx.packages.as_ref().unwrap().list().unwrap().outputs()["packages"]
            .contains("nginx-tools")
    );
    let journal = ctx.journal.as_ref().unwrap();
    let status = journal
        .action_status(&ActionStatusQuery {
            execution_ids: vec![run_id],
            expected_epoch: journal.epoch(),
        })
        .unwrap();
    assert!(matches!(
        status.answers[0].state,
        ActionState::Finished { .. }
    ));
    link.shutdown.cancel();
    receiving.await.unwrap();
    serving.await.unwrap();
}

/// The install `run_capability` for transfers `ids` (artifact, then signature).
fn rig_run(ids: &[Uuid]) -> domovoi_proto::messages::RunCapability {
    let rig_args = [
        ("package", "nginx-tools".to_owned()),
        ("version", "1.2.0".to_owned()),
        ("artifact_transfer", ids[0].to_string()),
        ("signature_transfer", ids[1].to_string()),
    ];
    domovoi_proto::messages::RunCapability {
        execution_id: Uuid::now_v7(),
        contract: "agent.package.install".to_owned(),
        version: 1,
        target: domovoi_core::TargetKey::parse("package:nginx-tools").unwrap(),
        args: rig_args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
        plan_hash: "07".repeat(32).parse::<domovoi_core::PlanHash>().unwrap(),
        authorization_kind: domovoi_proto::messages::AuthorizationKind::Human,
        expires_at: domovoi_testkit::build::T0 + jiff::SignedDuration::from_mins(5),
        expected_safety: None,
    }
}

#[tokio::test]
async fn list_over_run_capability_reports_the_installed_package() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    outputs(rig.call(rig.run("agent.package.install", &args)).await);
    let listed = outputs(rig.call(rig.run("agent.package.list", &[])).await);
    assert_eq!(listed["count"], "1");
    assert_eq!(listed["install_allowed"], "true");
    assert!(listed["packages"].contains("\"nginx-tools\""));
    assert!(listed["packages"].contains(&built.key_id.to_string()));
}

#[tokio::test]
async fn policy_authorization_is_refused() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    let mut run = rig.run("agent.package.install", &args);
    run.authorization_kind = domovoi_proto::messages::AuthorizationKind::Policy;
    assert_eq!(refusal(&rig.call(run).await), RefusalCode::AllowRiskDenied);
}

#[tokio::test]
async fn expired_authorization_is_refused() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    let mut run = rig.run("agent.package.install", &args);
    run.expires_at = rig.fixture.clock.now();
    assert_eq!(
        refusal(&rig.call(run).await),
        RefusalCode::AuthorizationExpired
    );
}

#[tokio::test]
async fn duplicate_execution_id_replays_the_first_result_and_runs_once() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    let run = rig.run("agent.package.install", &args);
    let first = outputs(rig.call(run.clone()).await);
    let again = outputs(rig.call(run).await);
    assert_eq!(first, again, "the replay carries the stored result");
    assert_eq!(first["already_installed"], "false");
}

#[tokio::test]
async fn extra_capability_args_are_bad_request() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let mut args = rig.install_args(&built, "nginx-tools", "1.2.0");
    args.push(("public_key", built.public_key_b64.clone()));
    assert_eq!(
        error_code(&rig.call(rig.run("agent.package.install", &args)).await),
        "bad_request"
    );
    let missing = [("package", "nginx-tools".to_owned())];
    assert_eq!(
        error_code(&rig.call(rig.run("agent.package.uninstall", &missing)).await),
        "bad_request"
    );
    let extra = [("anything", "x".to_owned())];
    assert_eq!(
        error_code(&rig.call(rig.run("agent.package.list", &extra)).await),
        "bad_request"
    );
}

fn offered(rig: &Rig) -> Vec<String> {
    let surface = rig.ctx.package_surface();
    catalog_entries(&rig.ctx.catalog, surface)
        .keys()
        .map(|id| id.as_str().to_owned())
        .filter(|id| id.starts_with("agent.package."))
        .collect()
}

#[tokio::test]
async fn get_catalog_advertises_package_contracts_per_config() {
    let absent = Rig::new(
        &[],
        Options {
            section: false,
            ..Options::default()
        },
    );
    assert!(offered(&absent).is_empty());
    let list_only = Rig::new(
        &[],
        Options {
            allow_install: false,
            ..Options::default()
        },
    );
    assert_eq!(offered(&list_only), vec!["agent.package.list"]);
    let full = Rig::new(&[], Options::default());
    assert_eq!(
        offered(&full),
        vec![
            "agent.package.disable",
            "agent.package.enable",
            "agent.package.install",
            "agent.package.list",
            "agent.package.uninstall"
        ]
    );
    // The paged answer carries the same ids.
    let reply = handle(
        &full.ctx,
        &RequestKind::Catalog(GetCatalog {
            page: 0,
            page_size: 100,
        }),
    )
    .await;
    let Message::CatalogPage(served) = reply else {
        panic!("expected a catalogue page");
    };
    assert!(
        served
            .entries
            .iter()
            .any(|entry| entry.contract == "agent.package.install")
    );
}

#[tokio::test]
async fn catalog_hash_changes_with_allow_install() {
    let list_only = Rig::new(
        &[],
        Options {
            allow_install: false,
            ..Options::default()
        },
    );
    let full = Rig::new(&[], Options::default());
    let hash = |rig: &Rig| {
        catalog_hash(&catalog_entries(
            &rig.ctx.catalog,
            rig.ctx.package_surface(),
        ))
    };
    assert_ne!(hash(&list_only), hash(&full));
    let none = catalog_hash(&catalog_entries(
        &full.ctx.catalog,
        PackageSurface::default(),
    ));
    assert_ne!(none, hash(&list_only));
}

#[tokio::test]
async fn other_contracts_are_still_refused_unknown() {
    let rig = Rig::new(&[], Options::default());
    let message = rig.call(rig.run("container.restart", &[])).await;
    assert_eq!(refusal(&message), RefusalCode::ContractUnknown);
}

#[tokio::test]
async fn package_contracts_are_refused_without_a_packages_section() {
    let rig = Rig::new(
        &[],
        Options {
            section: false,
            ..Options::default()
        },
    );
    for contract in [
        "agent.package.install",
        "agent.package.uninstall",
        "agent.package.list",
    ] {
        assert_eq!(
            refusal(&rig.call(rig.run(contract, &[])).await),
            RefusalCode::ContractUnknown,
            "{contract}"
        );
    }
}

#[tokio::test]
async fn install_is_refused_when_allow_install_is_false() {
    let built = nginx();
    let rig = Rig::new(
        &[&built],
        Options {
            allow_install: false,
            ..Options::default()
        },
    );
    let uninstall = [
        ("package", "nginx-tools".to_owned()),
        ("version", "1.2.0".to_owned()),
    ];
    assert_eq!(
        refusal(
            &rig.call(rig.run("agent.package.uninstall", &uninstall))
                .await
        ),
        RefusalCode::ContractUnknown
    );
    outputs(rig.call(rig.run("agent.package.list", &[])).await);
}

#[tokio::test]
async fn mutation_limit_applies_to_install() {
    let built = nginx();
    let limits = MutationLimits {
        per_hour: 30,
        per_capability: BTreeMap::from([("agent.package.install".to_owned(), 0)]),
    };
    let rig = Rig::new(
        &[&built],
        Options {
            limits,
            ..Options::default()
        },
    );
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    assert_eq!(
        refusal(&rig.call(rig.run("agent.package.install", &args)).await),
        RefusalCode::RateLimited
    );
    // Listing is never counted.
    outputs(rig.call(rig.run("agent.package.list", &[])).await);
}

#[tokio::test]
async fn journal_reports_outcome_unknown_after_crash_mid_install() {
    let built = nginx();
    let rig = Rig::new(&[&built], Options::default());
    let state = rig.dirs.state_dir.clone();
    let id = Uuid::now_v7();
    {
        // What an install interrupted by a crash leaves behind: accepted and started, never
        // finished, the start recorded with pid 0 as the capability layer does.
        let journal = rig.ctx.journal.as_ref().unwrap();
        let request = domovoi_agent::journal::AcceptedRequest {
            execution_id: id,
            contract: "agent.package.install",
            version: 1,
            target: "package:nginx-tools",
            plan_hash: "0707070707070707070707070707070707070707070707070707070707070707",
            authorization_kind: domovoi_proto::messages::AuthorizationKind::Human,
            mutating: true,
        };
        journal
            .accept(&request, rig.fixture.clock.now(), BOOT_ID, 1)
            .unwrap();
        journal.started(id, 0, 0).unwrap();
    }
    drop(rig.ctx);
    let reopened =
        domovoi_agent::Journal::open(&state, BOOT_ID, 2, MutationLimits::default()).unwrap();
    reopened.recover_orphans(&*clock()).await;
    let answers = reopened
        .action_status(&ActionStatusQuery {
            execution_ids: vec![id],
            expected_epoch: reopened.epoch(),
        })
        .unwrap();
    let ActionState::OutcomeUnknown { reason } = &answers.answers[0].state else {
        panic!("expected an unknown outcome, got {:?}", answers.answers[0]);
    };
    assert_eq!(reason.known(), Some(&OutcomeUnknownReason::AgentRestarted));
}
