use domovoi_core::attest::{
    BootId, ExpectedSafety, Hash32, SafetyIdentity, UnverifiedReason, permit_identity_current,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_helper::attest::guard::{ComputeFailure, GuardMode, GuardRefusal, check_expected};

fn identity() -> SafetyIdentity {
    SafetyIdentity {
        fingerprint: Hash32::of(b"key").into(),
        boot_id: BootId::parse("5f0c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b").expect("boot"),
        generation: 7,
        config_hash: Hash32::of(b"config"),
        route_state_hash: Hash32::of(b"routes"),
    }
}

fn verified(identity: &SafetyIdentity) -> ExpectedSafety {
    ExpectedSafety::from(identity)
}

fn unknown() -> ComputeFailure {
    ComputeFailure {
        token: "route_read_partial".to_owned(),
    }
}

#[test]
fn helper_refuses_missing_expected_safety() {
    let now = identity();
    assert_eq!(
        check_expected(None, Ok(&now)),
        Err(GuardRefusal::MalformedSafety)
    );
    // Absence is refused before anything is computed, so even an uncomputable identity gives
    // the malformed answer.
    assert_eq!(
        check_expected(None, Err(unknown())),
        Err(GuardRefusal::MalformedSafety)
    );
}

#[test]
fn malformed_expected_safety_refused() {
    for text in [
        r#"{"state":"verified"}"#,
        r#"{"state":"unverified","reason":"made_up"}"#,
        r#"{"state":"unverified","reason":"key_not_pinned","extra":1}"#,
        r#"{"state":"other"}"#,
        "null",
        "[]",
    ] {
        assert!(ExpectedSafety::parse_json(text).is_err(), "{text}");
    }
}

#[test]
fn helper_fails_closed_when_safety_state_unknown() {
    let now = identity();
    let tag = verified(&now);
    assert_eq!(
        check_expected(Some(&tag), Err(unknown())),
        Err(GuardRefusal::SafetyUnknown)
    );
    let unverified = ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    };
    assert_eq!(
        check_expected(Some(&unverified), Err(unknown())),
        Err(GuardRefusal::SafetyUnknown),
        "an unverified tag skips the comparison, not the computation"
    );
}

#[test]
fn a_verified_tag_equal_on_all_five_fields_is_compared_and_an_unverified_one_skips() {
    let now = identity();
    assert_eq!(
        check_expected(Some(&verified(&now)), Ok(&now)),
        Ok(GuardMode::Compared)
    );
    let unverified = ExpectedSafety::Unverified {
        reason: UnverifiedReason::AttestationStale,
    };
    assert_eq!(
        check_expected(Some(&unverified), Ok(&now)),
        Ok(GuardMode::SkippedUnverified)
    );
}

fn changed(field: &str) -> SafetyIdentity {
    let mut other = identity();
    match field {
        "fingerprint" => other.fingerprint = KeyFingerprint::from(Hash32::of(b"other key")),
        "boot_id" => {
            other.boot_id = BootId::parse("00000000-0000-4000-8000-000000000000").expect("boot");
        }
        "generation" => other.generation += 1,
        "config_hash" => other.config_hash = Hash32::of(b"other config"),
        _ => other.route_state_hash = Hash32::of(b"other routes"),
    }
    other
}

#[test]
fn config_change_between_attestation_and_apply_refused() {
    let was = identity();
    let now = changed("config_hash");
    assert_eq!(
        check_expected(Some(&verified(&was)), Ok(&now)),
        Err(GuardRefusal::SafetyChanged)
    );
}

#[test]
fn network_change_between_attestation_and_apply_refused() {
    let was = identity();
    let now = changed("route_state_hash");
    assert_eq!(
        check_expected(Some(&verified(&was)), Ok(&now)),
        Err(GuardRefusal::SafetyChanged)
    );
}

#[test]
fn reboot_between_permit_and_apply_refused() {
    let was = identity();
    let now = changed("boot_id");
    assert_eq!(
        check_expected(Some(&verified(&was)), Ok(&now)),
        Err(GuardRefusal::SafetyChanged)
    );
    assert!(!permit_identity_current(&was, &now));
}

#[test]
fn generation_change_between_permit_and_apply_refused() {
    let was = identity();
    let now = changed("generation");
    assert_eq!(
        check_expected(Some(&verified(&was)), Ok(&now)),
        Err(GuardRefusal::SafetyChanged)
    );
    assert!(!permit_identity_current(&was, &now));
}

#[test]
fn a_changed_helper_key_is_a_changed_safety_set() {
    let was = identity();
    let now = changed("fingerprint");
    assert_eq!(
        check_expected(Some(&verified(&was)), Ok(&now)),
        Err(GuardRefusal::SafetyChanged)
    );
    assert!(permit_identity_current(&was, &was.clone()));
}
