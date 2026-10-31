//! Property tests for the fingerprint rule: volatile values never enter a fingerprint.

use domovoi_core::{
    CheckKey, Evidence, EvidenceValue, HostId, Severity, Signal, SignalClass, SourceKind, TargetKey,
};
use jiff::Timestamp;
use proptest::prelude::*;

fn arb_evidence() -> impl Strategy<Value = Evidence> {
    let value = prop_oneof![
        any::<String>().prop_map(EvidenceValue::Text),
        any::<i64>().prop_map(EvidenceValue::Int),
        (-1.0e9..1.0e9f64).prop_map(EvidenceValue::Float),
        any::<bool>().prop_map(EvidenceValue::Bool),
    ];
    proptest::collection::btree_map("[a-z_]{1,12}", value, 0..8).prop_map(|map| {
        let mut evidence = Evidence::new();
        for (key, value) in map {
            evidence.insert(key, value);
        }
        evidence
    })
}

fn arb_severity() -> impl Strategy<Value = Severity> {
    prop_oneof![
        Just(Severity::Info),
        Just(Severity::Warning),
        Just(Severity::Critical)
    ]
}

proptest! {
    #[test]
    fn fingerprint_ignores_evidence_and_timestamps(
        evidence_a in arb_evidence(),
        evidence_b in arb_evidence(),
        summary_a in ".{0,400}",
        summary_b in ".{0,400}",
        severity_a in arb_severity(),
        severity_b in arb_severity(),
        secs_a in 0i64..4_000_000_000,
        secs_b in 0i64..4_000_000_000,
        immediate in any::<bool>(),
    ) {
        let host = HostId::new();
        let check = CheckKey::parse("disk.free").unwrap();
        let target = Some(TargetKey::parse("/mnt/data").unwrap());
        let a = Signal::firing(
            SourceKind::BuiltIn,
            host,
            check.clone(),
            target.clone(),
            severity_a,
            summary_a,
            Timestamp::from_second(secs_a).unwrap(),
        )
        .with_evidence(evidence_a);
        let class = if immediate { SignalClass::Immediate } else { SignalClass::Normal };
        let b = Signal::firing(
            SourceKind::Agent,
            host,
            check,
            target,
            severity_b,
            summary_b,
            Timestamp::from_second(secs_b).unwrap(),
        )
        .with_evidence(evidence_b)
        .with_class(class);
        prop_assert_eq!(a.fingerprint(), b.fingerprint());
    }
}
