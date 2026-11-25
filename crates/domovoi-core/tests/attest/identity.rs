//! The safety identity, boot id, fingerprint and nonce types, and the reason enums.

use domovoi_core::KeyFingerprint;
use domovoi_core::attest::{
    BootId, Hash32, IdentityMatch, Nonce32, SafetyIdentity, UnverifiedReason, ZERO_HASH, compare,
    permit_identity_current,
};
use domovoi_core::helper::Nonce;
use domovoi_core::knowledge::UnverifiableReason;

use crate::common::{BOOT, BOOT_B, boot, hash};

pub fn identity() -> SafetyIdentity {
    SafetyIdentity {
        fingerprint: KeyFingerprint::of_public_key(&[7; 32]),
        boot_id: boot(),
        generation: 4,
        config_hash: Hash32::from_bytes([1; 32]),
        route_state_hash: Hash32::from_bytes([2; 32]),
    }
}

#[test]
fn safety_identity_canonical_round_trips() {
    let id = identity();
    let text = id.canonical();
    assert_eq!(
        text,
        format!(
            "v1:{}:{BOOT}:4:{}:{}",
            id.fingerprint,
            "01".repeat(32),
            "02".repeat(32)
        )
    );
    assert_eq!(
        SafetyIdentity::parse_canonical(&text).expect("canonical"),
        id
    );
    assert!(text.len() < 2048, "fits coverage evidence");
}

#[test]
fn safety_identity_parse_is_strict() {
    let text = identity().canonical();
    let parts: Vec<&str> = text.split(':').collect();
    let join = |parts: &[&str]| parts.join(":");
    let mut cases: Vec<String> = vec![
        format!(" {text}"),
        format!("{text} "),
        format!("{text}\n"),
        format!("{text}:extra"),
        text.replacen("v1", "v2", 1),
        text.replacen("v1", "V1", 1),
        text.to_uppercase(),
        join(&parts[..5]),
        String::new(),
    ];
    for (index, bad) in [
        (1, "ab".repeat(31)),
        (1, "AB".repeat(32)),
        (2, BOOT.to_uppercase()),
        (2, BOOT.replace('-', "")),
        (3, "04".to_owned()),
        (3, "-4".to_owned()),
        (3, "4.0".to_owned()),
        (3, String::new()),
        (4, "01".repeat(33)),
        (5, "0G".repeat(32)),
    ] {
        let mut changed = parts.clone();
        changed[index] = &bad;
        cases.push(join(&changed));
    }
    for case in cases {
        assert!(SafetyIdentity::parse_canonical(&case).is_err(), "{case:?}");
    }
}

#[test]
fn fingerprint_rejects_uppercase_and_wrong_length() {
    let good = "ab".repeat(32);
    assert!(good.parse::<Hash32>().is_ok());
    for bad in [
        good.to_uppercase(),
        "ab".repeat(31),
        "ab".repeat(33),
        String::new(),
        format!("{good} "),
    ] {
        assert!(bad.parse::<Hash32>().is_err(), "{bad:?}");
    }
    let mixed = format!("A{}", &good[1..]);
    assert!(mixed.parse::<Hash32>().is_err());
    assert!(serde_json::from_value::<Hash32>(serde_json::json!(good)).is_ok());
    assert!(serde_json::from_value::<Hash32>(serde_json::json!(mixed)).is_err());
    assert!(ZERO_HASH.is_zero());
}

#[test]
fn boot_id_rejects_uppercase_hyphenless_and_braced() {
    assert!(BootId::parse(BOOT).is_ok());
    for bad in [
        BOOT.to_uppercase(),
        BOOT.replace('-', ""),
        format!("{{{BOOT}}}"),
        format!("urn:uuid:{BOOT}"),
        BOOT.replacen('-', "_", 1),
        BOOT[1..].to_owned(),
        format!("{BOOT}0"),
        BOOT.replacen('5', "g", 1),
        String::new(),
    ] {
        assert!(BootId::parse(&bad).is_err(), "{bad:?}");
        assert!(
            serde_json::from_value::<BootId>(serde_json::json!(bad)).is_err(),
            "{bad:?}"
        );
    }
}

fn with(id: &SafetyIdentity, change: impl FnOnce(&mut SafetyIdentity)) -> SafetyIdentity {
    let mut changed = id.clone();
    change(&mut changed);
    changed
}

#[test]
fn compare_separates_a_reboot_from_every_other_change() {
    let id = identity();
    assert_eq!(compare(&id, &id), IdentityMatch::Exact);
    let rebooted = with(&id, |i| i.boot_id = BootId::parse(BOOT_B).expect("boot"));
    assert_eq!(compare(&id, &rebooted), IdentityMatch::BootOnlyDiffers);
    for different in [
        with(&id, |i| i.generation += 1),
        with(&id, |i| i.config_hash = hash(&"03".repeat(32))),
        with(&id, |i| i.route_state_hash = hash(&"03".repeat(32))),
        with(&id, |i| {
            i.fingerprint = KeyFingerprint::of_public_key(&[8; 32])
        }),
        with(&rebooted, |i| i.generation += 1),
    ] {
        assert_eq!(compare(&id, &different), IdentityMatch::Different);
    }
}

#[test]
fn permit_from_before_reboot_refused() {
    let id = identity();
    assert!(permit_identity_current(&id, &id));
    let rebooted = with(&id, |i| i.boot_id = BootId::parse(BOOT_B).expect("boot"));
    assert!(
        !permit_identity_current(&id, &rebooted),
        "boot_id included, no reboot exception"
    );
    assert!(!permit_identity_current(
        &id,
        &with(&id, |i| i.generation += 1)
    ));
    assert!(!permit_identity_current(
        &id,
        &with(&id, |i| i.config_hash = ZERO_HASH)
    ));
    assert!(!permit_identity_current(
        &id,
        &with(&id, |i| i.route_state_hash = ZERO_HASH)
    ));
}

#[test]
fn nonce_converts_to_and_from_the_helper_socket_nonce() {
    let nonce = Nonce32::from_bytes([0xab; 32]);
    let socket: Nonce = nonce.into();
    assert_eq!(socket.to_hex(), "ab".repeat(32));
    assert_eq!(Nonce32::from(socket), nonce);
    assert_eq!(nonce.to_string(), "ab".repeat(32));
    assert_eq!(Nonce32::parse(&"ab".repeat(32)), Some(nonce));
    assert_eq!(Nonce32::parse(&"AB".repeat(32)), None);
    assert_eq!(Nonce32::parse(&"ab".repeat(31)), None);
}

#[test]
fn reasons_map_one_to_one_except_not_available() {
    assert_eq!(UnverifiedReason::ALL.len(), 9);
    for reason in UnverifiedReason::ALL {
        assert_eq!(UnverifiedReason::parse(reason.token()), Ok(reason));
        assert_eq!(
            serde_json::to_value(reason).expect("serializable"),
            serde_json::json!(reason.token())
        );
        let coverage: UnverifiableReason = reason.into();
        assert_eq!(UnverifiedReason::from(coverage), reason);
        assert!(!coverage.describe().is_empty());
    }
    assert_eq!(
        UnverifiedReason::from(UnverifiableReason::NotAvailable),
        UnverifiedReason::AttestationMissing
    );
    assert!(UnverifiedReason::parse("path uncovered").is_err());
    assert!(UnverifiedReason::parse("").is_err());
    assert_eq!(
        UnverifiableReason::PathUncovered.describe(),
        "the helper's attestation does not cover this path"
    );
    assert_eq!(
        UnverifiableReason::ConfirmationStale.describe(),
        "the confirmation no longer matches the helper's current safety set"
    );
}
