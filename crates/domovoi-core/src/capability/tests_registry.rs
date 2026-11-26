//! Tests of the availability, catalogue and target-identity types. Pure: no clock, no fakes.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::{from_value, to_value};

use super::availability::{Availability, NotAllowedOnHost, UnavailableReason};
use super::catalog::catalog_hash;
use super::catalog::{
    CatalogImplementation, CatalogPageData, CatalogQuery, CatalogSnapshot, CatalogueGap,
    CatalogueState,
};
use super::effect::TargetKind;
use super::ids::{ContractId, ContractVersion, TargetName};
use super::target::{
    ContainerId, TargetQuery, TargetUnresolved, canonical_container_identity,
    canonical_host_identity, validate_container_name,
};
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::HostId;
use jiff::Timestamp;

/// A host id that never changes, so an identity text can be compared literally.
const HOST_TEXT: &str = "018f4e46-4a2e-7a37-9d0a-4f2b6c8d0e1f";

/// A full container id, as Docker reports one.
const FULL_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A fixed time for the snapshot payloads the tests build.
fn at() -> Timestamp {
    Timestamp::constant(1_700_000_000, 0)
}

#[test]
fn availability_has_eight_states_with_codes() {
    let states = Availability::all();
    assert_eq!(states.len(), 8);
    let codes = states
        .iter()
        .map(|state| state.code())
        .collect::<Vec<&'static str>>();
    let expected: Vec<&'static str> = vec![
        "available",
        "not_installed",
        "disabled_by_hub",
        "not_allowed_on_host",
        "unavailable",
        "host_unknown",
        "host_absent",
        "host_incompatible",
    ];
    assert_eq!(codes, expected, "the states answer {codes:?}");
    let unique = codes.iter().copied().collect::<BTreeSet<&'static str>>();
    assert_eq!(unique.len(), 8, "{codes:?} repeats a state");
    let offered = states
        .iter()
        .filter(|state| state.is_available())
        .collect::<Vec<&Availability>>();
    assert_eq!(offered.len(), 1, "the states answer {codes:?}");
}

#[test]
fn availability_reasons_have_codes() {
    let unavailable: Vec<(UnavailableReason, &'static str)> = vec![
        (
            UnavailableReason::SkillRunnerMissing,
            "skill_runner_missing",
        ),
        (
            UnavailableReason::SkillRunnerUnsupported,
            "skill_runner_unsupported",
        ),
        (UnavailableReason::SkillRunnerError, "skill_runner_error"),
        (UnavailableReason::HelperMissing, "helper_missing"),
        (UnavailableReason::HelperError, "helper_error"),
        (
            UnavailableReason::CatalogueIncomplete,
            "catalogue_incomplete",
        ),
        (
            UnavailableReason::CatalogueHashMismatch,
            "catalogue_hash_mismatch",
        ),
        (UnavailableReason::CatalogueStale, "catalogue_stale"),
        (
            UnavailableReason::CatalogueInconsistent,
            "catalogue_inconsistent",
        ),
        (
            UnavailableReason::ContainerRuntimeUnavailable,
            "container_runtime_unavailable",
        ),
        (
            UnavailableReason::DockerWriteUnsupported,
            "docker_write_unsupported",
        ),
    ];
    for (reason, code) in unavailable {
        let state = Availability::Unavailable { reason };
        let answer = state.reason_code();
        assert_eq!(
            answer,
            Some(code),
            "the reason {reason:?} answers {answer:?}"
        );
        let sentence = reason.describe();
        assert_ne!(
            sentence, code,
            "the sentence for {reason:?} is a bare code: {sentence}"
        );
        assert!(
            sentence.len() > code.len(),
            "the sentence {sentence} is shorter than its code"
        );
    }
    let refused: Vec<(NotAllowedOnHost, &'static str)> = vec![
        (NotAllowedOnHost::CeilingAbsent, "ceiling_absent"),
        (
            NotAllowedOnHost::CeilingEntryMissing,
            "ceiling_entry_missing",
        ),
        (NotAllowedOnHost::AllowRisk, "allow_risk"),
    ];
    for (reason, code) in refused {
        let state = Availability::NotAllowedOnHost { reason };
        let answer = state.reason_code();
        assert_eq!(
            answer,
            Some(code),
            "the reason {reason:?} answers {answer:?}"
        );
        let sentence = reason.describe();
        assert_ne!(
            sentence, code,
            "the sentence for {reason:?} is a bare code: {sentence}"
        );
    }
    let plain = vec![
        Availability::Available,
        Availability::NotInstalled,
        Availability::DisabledByHub,
        Availability::HostUnknown,
    ];
    for state in plain {
        assert_eq!(
            state.reason_code(),
            None,
            "{state:?} carries a reason it should not"
        );
    }
}

#[test]
fn container_name_grammar_enforced() {
    let accepted = vec!["plex", "my-container", "a.b_c", "Plex", "ab"];
    for name in accepted {
        let checked = validate_container_name(name);
        assert!(checked.is_ok(), "the grammar refuses {name:?}: {checked:?}");
    }
    let too_long = "a".repeat(129);
    let rejected = vec![
        "",
        "p",
        "-plex",
        "_plex",
        ".plex",
        "plex ",
        " plex",
        "plex/1",
        "a b",
        too_long.as_str(),
    ];
    for name in rejected {
        assert!(
            validate_container_name(name).is_err(),
            "the grammar accepts {name:?}"
        );
    }
}

#[test]
fn container_id_requires_64_lowercase_hex() {
    let full = ContainerId::parse(FULL_ID);
    assert!(full.is_ok(), "{FULL_ID} is not a full id: {full:?}");
    let short = "0123456789ab";
    let upper = FULL_ID.to_uppercase();
    let one_short = &FULL_ID[..63];
    let one_long = format!("{FULL_ID}0");
    let prefix = &FULL_ID[..62];
    let prefixed = format!("0x{prefix}");
    let rejected = vec![
        short,
        upper.as_str(),
        one_short,
        one_long.as_str(),
        "",
        prefixed.as_str(),
    ];
    for text in rejected {
        assert!(
            ContainerId::parse(text).is_err(),
            "the grammar accepts {text:?} as a container id"
        );
    }
    let shown = format!("{}", full.unwrap());
    assert_eq!(shown, FULL_ID, "the parsed id reads {shown}, not {FULL_ID}");
}

#[test]
fn catalogue_state_serde_round_trip() {
    let hex_a = "a".repeat(64);
    let hex_b = "b".repeat(64);
    let contract = ContractId::parse("container.list").unwrap();
    let states = vec![
        CatalogueState::Complete {
            hash: hex_a.clone(),
        },
        CatalogueState::Partial {
            gap: CatalogueGap::NotWalked,
        },
        CatalogueState::Partial {
            gap: CatalogueGap::PageFailed { page: 3 },
        },
        CatalogueState::Partial {
            gap: CatalogueGap::DuplicateEntry {
                contract: contract.clone(),
            },
        },
        CatalogueState::Stale {
            announced: hex_a.clone(),
            snapshot: hex_b.clone(),
        },
    ];
    for state in states {
        let value = to_value(&state).unwrap();
        let back: CatalogueState = from_value(value).unwrap();
        assert_eq!(back, state, "the state {back:?} did not survive");
    }
    let page = CatalogPageData {
        entries: vec![CatalogImplementation {
            contract,
            version: ContractVersion::first(),
        }],
        next_page: Some(1),
    };
    let page_back: CatalogPageData = from_value(to_value(&page).unwrap()).unwrap();
    assert_eq!(page_back, page, "the page {page:?} did not survive");
    let query = CatalogQuery {
        page: 2,
        page_size: 100,
    };
    let query_back: CatalogQuery = from_value(to_value(query).unwrap()).unwrap();
    assert_eq!(query_back, query, "the query {query:?} did not survive");
    let snapshot = CatalogSnapshot {
        host: HostId::from_uuid(HOST_TEXT.parse().unwrap()),
        announced_hash: Some(hex_a.clone()),
        state: CatalogueState::Complete { hash: hex_a },
        entries: BTreeMap::new(),
        unknown: vec!["container.destroy".to_owned()],
        pages: 1,
        session_since: Some(at()),
        at: at(),
    };
    let snapshot_back: CatalogSnapshot = from_value(to_value(&snapshot).unwrap()).unwrap();
    assert_eq!(
        snapshot_back, snapshot,
        "the snapshot {snapshot:?} did not survive"
    );
    let empty: BTreeMap<ContractId, ContractVersion> = BTreeMap::new();
    assert_eq!(
        catalog_hash(&empty),
        catalog_hash(&snapshot_back.entries),
        "two empty lists hash apart"
    );
}

#[test]
fn target_query_host_identity_shape() {
    let host = HostId::from_uuid(HOST_TEXT.parse().unwrap());
    let key = KeyFingerprint::of_public_key(&[7u8; 32]);
    let identity = canonical_host_identity(host, &key).unwrap();
    let text = format!("{}", identity);
    assert_eq!(
        text,
        host_target_text(host, &key),
        "{text} is not the canonical text"
    );
    assert!(text.starts_with("host:"), "{text} has no host scheme");
    assert!(text.contains(HOST_TEXT), "{text} does not name the host");
    let fingerprint = format!("{}", key);
    assert!(
        text.ends_with(&fingerprint),
        "{text} does not embed the fingerprint"
    );
    assert_eq!(
        text.len(),
        106,
        "the host identity does not have the expected length"
    );
    let container = ContainerId::parse(FULL_ID).unwrap();
    let bound = canonical_container_identity(host, &key, &container).unwrap();
    let container_text = format!("{}", bound);
    assert_eq!(
        container_text.len(),
        176,
        "the container identity does not have the expected length"
    );
    assert!(
        container_text.len() <= 310,
        "the identity is over TargetIdentity's 310 limit"
    );
    assert!(
        container_text.starts_with("container:"),
        "{container_text} has no container scheme"
    );
    assert!(
        container_text.starts_with(&format!("container:{host}/")),
        "the host text is not embedded"
    );
    let query = TargetQuery::Container {
        host,
        name: TargetName::parse("plex").unwrap(),
    };
    assert_eq!(query.host(), host, "the query asks about another host");
    let unit = TargetQuery::Unit {
        host,
        name: TargetName::parse("nginx.service").unwrap(),
    };
    assert_eq!(
        unit.kind(),
        TargetKind::Unit,
        "the unit query reads as {unit:?}"
    );
}

#[test]
fn unit_identity_is_never_claimed() {
    let reason = TargetUnresolved::UnitIdentityUnproven;
    let sentence = reason.describe();
    assert!(
        sentence.contains("unproven"),
        "the sentence {sentence} claims an identity"
    );
    assert!(
        !sentence.contains("accepted"),
        "the sentence {sentence} says identity is accepted"
    );
    assert!(
        !sentence.contains("verified"),
        "the sentence {sentence} says identity is verified"
    );
    let shown = format!("{}", reason);
    assert!(
        shown.contains("not proven"),
        "{shown} is too permissive about unit identity"
    );
}
