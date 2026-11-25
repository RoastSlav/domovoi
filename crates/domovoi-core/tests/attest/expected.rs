//! `expected_safety`: the strict wire form and the one function that requires it.

use domovoi_core::AuthorizationKind;
use domovoi_core::attest::{
    BootId, ExpectedSafety, MalformedSafety, PolicyRequiresHuman, UnverifiedReason, agent_accepts,
    require_expected_safety,
};

use crate::common::vector_text;
use crate::identity::identity;

fn rows() -> Vec<(bool, String)> {
    vector_text("expected_safety.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|row| {
            let (verdict, json) = row.split_once('\t').expect("a row");
            (verdict == "accept", json.to_owned())
        })
        .collect()
}

#[test]
fn expected_safety_vectors_accept_and_refuse_exactly() {
    let rows = rows();
    assert!(rows.len() >= 25);
    for (accept, json) in rows {
        let parsed = ExpectedSafety::parse_json(&json);
        assert_eq!(parsed.is_ok(), accept, "{json}");
    }
}

#[test]
fn expected_safety_parser_round_trips() {
    for (accept, json) in rows() {
        if !accept {
            continue;
        }
        let parsed = ExpectedSafety::parse_json(&json).expect("accepted");
        let written = serde_json::to_string(&parsed).expect("serializable");
        assert_eq!(
            ExpectedSafety::parse_json(&written).expect("re-read"),
            parsed
        );
        // Accepted values have exactly one spelling once keys are sorted.
        let reread: serde_json::Value = serde_json::from_str(&written).expect("json");
        let original: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(reread, original);
    }
}

#[test]
fn malformed_expected_safety_refused() {
    for json in [
        "{}",
        r#"{"state":"verified"}"#,
        r#"{"state":"unverified","reason":"nope"}"#,
        "42",
    ] {
        assert!(matches!(
            ExpectedSafety::parse_json(json),
            Err(MalformedSafety::Invalid(_))
        ));
    }
}

#[test]
fn helper_refuses_missing_expected_safety() {
    assert_eq!(require_expected_safety(None), Err(MalformedSafety::Missing));
    let tag = ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyMismatch,
    };
    assert_eq!(require_expected_safety(Some(&tag)), Ok(&tag));
}

#[test]
fn expected_safety_carries_the_identity_it_was_built_from() {
    let id = identity();
    let tag = ExpectedSafety::from(&id);
    assert_eq!(tag.identity(), Some(id));
    let ExpectedSafety::Verified {
        fingerprint,
        boot_id,
        ..
    } = &tag
    else {
        panic!("verified");
    };
    assert_eq!(fingerprint.to_string(), identity().fingerprint.to_string());
    assert_eq!(boot_id, &BootId::parse(boot_id.as_str()).expect("boot"));
    let unverified = ExpectedSafety::Unverified {
        reason: UnverifiedReason::AttestationStale,
    };
    assert_eq!(unverified.identity(), None);
}

#[test]
fn agent_accepts_refuses_unverified_for_policy_authority() {
    let unverified = ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    };
    let verified = ExpectedSafety::from(&identity());
    assert_eq!(
        agent_accepts(&unverified, AuthorizationKind::Policy),
        Err(PolicyRequiresHuman)
    );
    assert_eq!(agent_accepts(&unverified, AuthorizationKind::Human), Ok(()));
    assert_eq!(agent_accepts(&verified, AuthorizationKind::Policy), Ok(()));
    assert_eq!(agent_accepts(&verified, AuthorizationKind::Human), Ok(()));
}
