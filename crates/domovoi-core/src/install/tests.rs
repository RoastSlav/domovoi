//! Tests of the install types.

use uuid::Uuid;

use super::*;
use crate::package::{PackageFailureCode, PackageName, PackageVersion, keys};
use crate::ports::{AggregateKind, AggregateRef, JournalKind};

#[test]
fn install_status_terminal_set_is_exact() {
    let terminal: Vec<TargetStatus> = TargetStatus::ALL
        .into_iter()
        .filter(|status| status.is_terminal())
        .collect();
    assert_eq!(
        terminal,
        [
            TargetStatus::Succeeded,
            TargetStatus::Failed,
            TargetStatus::NotRun,
            TargetStatus::OutcomeUnknown,
            TargetStatus::Cancelled,
        ]
    );
    for status in [
        TargetStatus::Queued,
        TargetStatus::Transferring,
        TargetStatus::Dispatched,
    ] {
        assert!(!status.is_terminal(), "{}", status.as_str());
    }
}

#[test]
fn install_enums_round_trip_through_their_text() {
    for status in TargetStatus::ALL {
        assert_eq!(TargetStatus::parse(status.as_str()), Some(status));
    }
    assert_eq!(TargetStatus::parse("done"), None);
    for kind in [InstallKind::Install, InstallKind::Uninstall] {
        assert_eq!(InstallKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(InstallKind::parse("upgrade"), None);
    assert_eq!(ArtifactSource::OwnerImport.as_str(), "owner_import");
    assert_eq!(
        ArtifactSource::parse("owner_import"),
        Some(ArtifactSource::OwnerImport)
    );
    assert_eq!(ArtifactSource::parse("registry"), None);
    for by in [ObservedBy::Job, ObservedBy::Refresh] {
        assert_eq!(ObservedBy::parse(by.as_str()), Some(by));
    }
    assert_eq!(ObservedBy::parse("agent"), None);
}

#[test]
fn aggregate_and_journal_kinds_round_trip() {
    assert_eq!(AggregateKind::InstallJob.as_str(), "install_job");
    assert_eq!(
        AggregateKind::parse("install_job"),
        Some(AggregateKind::InstallJob)
    );
    let id = crate::id::InstallJobId::new();
    let aggregate = AggregateRef::install_job(id);
    assert_eq!(aggregate.kind, AggregateKind::InstallJob);
    assert_eq!(aggregate.to_string(), format!("install_job:{id}"));
    assert_eq!(JournalKind::InstallTarget.as_str(), "install.target");
    assert_eq!(
        JournalKind::parse("install.target"),
        Some(JournalKind::InstallTarget)
    );
}

#[test]
fn install_ids_round_trip_through_text() {
    let artifact = crate::id::PackageArtifactId::new();
    assert_eq!(artifact.to_string().parse(), Ok(artifact));
    let job = crate::id::InstallJobId::new();
    assert_eq!(job.to_string().parse(), Ok(job));
    let target = crate::id::InstallTargetId::new();
    assert_eq!(target.to_string().parse(), Ok(target));
}

fn name() -> PackageName {
    PackageName::parse("mail-tools").unwrap()
}

fn version() -> PackageVersion {
    PackageVersion::parse("1.2.3").unwrap()
}

#[test]
fn package_call_has_no_key_argument() {
    let allowed = [
        keys::ARG_PACKAGE,
        keys::ARG_VERSION,
        keys::ARG_ARTIFACT_TRANSFER,
        keys::ARG_SIGNATURE_TRANSFER,
    ];
    let calls = [
        PackageCall::Install {
            package: name(),
            version: version(),
            artifact_transfer: Uuid::from_u128(1),
            signature_transfer: Uuid::from_u128(2),
        },
        PackageCall::Uninstall {
            package: name(),
            version: version(),
        },
        PackageCall::List,
    ];
    for call in &calls {
        for key in call.args().keys() {
            assert!(
                allowed.contains(key),
                "{} sends `{key}`",
                call.contract_id()
            );
            assert!(!key.contains("key"), "{key}");
        }
    }
    assert_eq!(calls[0].args().len(), 4);
    assert_eq!(calls[1].args().len(), 2);
    assert!(calls[2].args().is_empty());
}

#[test]
fn package_call_names_its_contract_and_target() {
    let install = PackageCall::Install {
        package: name(),
        version: version(),
        artifact_transfer: Uuid::from_u128(1),
        signature_transfer: Uuid::from_u128(2),
    };
    assert_eq!(install.contract_id(), "agent.package.install");
    assert_eq!(install.target_text(), "package:mail-tools");
    assert_eq!(
        install.args()[keys::ARG_ARTIFACT_TRANSFER],
        Uuid::from_u128(1).to_string()
    );
    assert_eq!(PackageCall::List.contract_id(), "agent.package.list");
    assert_eq!(PackageCall::List.target_text(), "packages:all");
    let known: Vec<String> = crate::capability::package_contracts()
        .into_iter()
        .map(|contract| contract.id.as_str().to_owned())
        .collect();
    assert!(known.iter().any(|id| id == install.contract_id()));
    assert!(known.iter().any(|id| id == PackageCall::List.contract_id()));
    assert!(crate::signal::TargetKey::parse(&install.target_text()).is_ok());
    assert!(crate::signal::TargetKey::parse(&PackageCall::List.target_text()).is_ok());
}

#[test]
fn transfer_frames_know_their_transfer() {
    let id = Uuid::from_u128(9);
    let frames = [
        TransferFrame::Begin {
            transfer: id,
            kind: TransferKind::PackageArtifact,
            size: 3,
            sha256: [0; 32],
        },
        TransferFrame::Chunk {
            transfer: id,
            seq: 0,
            bytes: vec![1, 2, 3],
        },
        TransferFrame::End { transfer: id },
    ];
    for frame in frames {
        assert_eq!(frame.transfer(), id);
    }
    assert_eq!(MAX_CHUNK_BYTES, 65_536);
}

#[test]
fn agent_failure_codes_pass_through_verbatim() {
    for code in PackageFailureCode::ALL {
        let failure = TargetFailure::from_agent_message(&format!("{code}: detail here"));
        assert_eq!(failure.code(), code.as_str());
        assert_eq!(failure.message(), "detail here");
    }
    for code in REFUSAL_CODES {
        let failure = TargetFailure::from_agent_message(&format!("{code}: no"));
        assert_eq!(failure.code(), code);
    }
}

#[test]
fn unrecognised_agent_message_is_agent_refused_and_truncated() {
    let failure = TargetFailure::from_agent_message("something odd: happened");
    assert_eq!(failure.code(), codes::AGENT_REFUSED);
    assert_eq!(failure.message(), "something odd: happened");

    let long = "é".repeat(900);
    let failure = TargetFailure::from_agent_message(&long);
    assert_eq!(failure.code(), codes::AGENT_REFUSED);
    assert_eq!(failure.message().chars().count(), MAX_FAILURE_MESSAGE_CHARS);

    let failure = TargetFailure::new("Not A Token", "x");
    assert_eq!(failure.code(), codes::AGENT_REFUSED);
    let failure = TargetFailure::new(codes::HUB_RESTARTED, "the hub restarted");
    assert_eq!(failure.code(), "hub_restarted");
}
