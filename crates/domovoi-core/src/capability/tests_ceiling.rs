//! Tests for the hub's typed checks over an agent's unattended ceiling: every
//! nonsense number is refused with its name, and the ceiling is cross-checked
//! against the contracts the hub actually holds.

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;

use super::ceiling::{CeilingEntry, CeilingError, CeilingSummary, CeilingTargets};
use super::effect::EffectClass;
use super::ids::{ContractId, TargetName};
use super::index::ContractIndex;
use super::tests::bounded_constrainable;
use super::tests::containment_contract;
use super::tests::crafted;

/// A summary of the given entries, enabled.
fn enabled_summary(entries: Vec<CeilingEntry>) -> CeilingSummary {
    CeilingSummary {
        enabled: true,
        entries,
        hash: None,
    }
}

/// One ceiling entry with the given shape.
pub(super) fn ceiling_entry(
    contract: &str,
    targets: CeilingTargets,
    max_per_hour: u32,
    max_active: Option<u32>,
    max_ttl: Option<SignedDuration>,
    args: BTreeMap<String, i64>,
) -> CeilingEntry {
    CeilingEntry {
        contract: ContractId::parse(contract).unwrap(),
        targets,
        max_per_hour,
        max_active,
        max_ttl,
        args,
    }
}

/// The contracts the hub holds for the cross-checked tests: one bounded integer
/// argument the ceiling may constrain, plus a host contract.
fn hub_index() -> ContractIndex {
    ContractIndex::new(vec![
        bounded_constrainable(),
        crafted("host.arrest", "host.mutate", EffectClass::Mutate),
    ])
    .unwrap()
}

#[test]
fn ceiling_zero_rate_and_zero_active_refused() {
    let zero_rate = ceiling_entry("x.y", CeilingTargets::Any, 0, None, None, BTreeMap::new());
    assert_eq!(
        enabled_summary(vec![zero_rate]).validate(),
        Err(CeilingError::ZeroRate {
            contract: ContractId::parse("x.y").unwrap()
        })
    );
    let zero_active = ceiling_entry(
        "x.y",
        CeilingTargets::Any,
        4,
        Some(0),
        None,
        BTreeMap::new(),
    );
    assert_eq!(
        enabled_summary(vec![zero_active]).validate(),
        Err(CeilingError::ZeroActive {
            contract: ContractId::parse("x.y").unwrap()
        })
    );
}

#[test]
fn ceiling_nonpositive_ttl_refused() {
    for ttl in [
        Some(SignedDuration::ZERO),
        Some(SignedDuration::from_secs(-120)),
    ] {
        let entry = ceiling_entry("x.y", CeilingTargets::Any, 4, None, ttl, BTreeMap::new());
        assert_eq!(
            enabled_summary(vec![entry]).validate(),
            Err(CeilingError::NonPositiveTtl {
                contract: ContractId::parse("x.y").unwrap()
            })
        );
    }
}

#[test]
fn ceiling_empty_target_names_refused() {
    let empty = ceiling_entry(
        "x.y",
        CeilingTargets::Names(BTreeSet::<TargetName>::new()),
        4,
        None,
        None,
        BTreeMap::new(),
    );
    assert_eq!(
        enabled_summary(vec![empty]).validate(),
        Err(CeilingError::EmptyTargetNames {
            contract: ContractId::parse("x.y").unwrap()
        })
    );
}

#[test]
fn ceiling_duplicate_entries_refused() {
    let one = ceiling_entry("x.y", CeilingTargets::Any, 4, None, None, BTreeMap::new());
    let other = ceiling_entry("x.y", CeilingTargets::Any, 8, None, None, BTreeMap::new());
    assert_eq!(
        enabled_summary(vec![one, other]).validate(),
        Err(CeilingError::DuplicateEntry {
            contract: ContractId::parse("x.y").unwrap()
        })
    );
}

#[test]
fn ceiling_hash_must_be_canonical() {
    for malformed in ["zz", "", "AB", "g"] {
        let summary = CeilingSummary {
            enabled: true,
            entries: Vec::new(),
            hash: Some(malformed.to_string()),
        };
        assert_eq!(summary.validate(), Err(CeilingError::MalformedHash));
    }
    let canonical = CeilingSummary {
        enabled: true,
        entries: Vec::new(),
        hash: Some("ab".repeat(32)),
    };
    canonical.validate().unwrap();
    let longest = CeilingSummary {
        enabled: true,
        entries: Vec::new(),
        hash: Some("f".repeat(128)),
    };
    longest.validate().unwrap();
    assert_eq!(
        CeilingSummary {
            enabled: true,
            entries: Vec::new(),
            hash: Some("f".repeat(129)),
        }
        .validate(),
        Err(CeilingError::MalformedHash)
    );
}

#[test]
fn ceiling_disabled_still_validated() {
    let zero_rate = ceiling_entry("x.y", CeilingTargets::Any, 0, None, None, BTreeMap::new());
    let summary = CeilingSummary {
        enabled: false,
        entries: vec![zero_rate],
        hash: None,
    };
    assert_eq!(
        summary.validate(),
        Err(CeilingError::ZeroRate {
            contract: ContractId::parse("x.y").unwrap()
        })
    );
}

#[test]
fn ceiling_against_index_refuses_unknown_contract_and_undeclared_arg() {
    let index = hub_index();
    let ghost = ceiling_entry(
        "host.ghost",
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::new(),
    );
    assert_eq!(
        enabled_summary(vec![ghost]).validate_against(&index),
        Err(CeilingError::UnknownContract {
            contract: ContractId::parse("host.ghost").unwrap()
        })
    );
    let bounded = bounded_constrainable();
    let undeclared = ceiling_entry(
        bounded.id.as_str(),
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::from([("extra".to_owned(), 5i64)]),
    );
    assert_eq!(
        enabled_summary(vec![undeclared]).validate_against(&index),
        Err(CeilingError::UndeclaredArg {
            contract: bounded.id.clone(),
            key: "extra".to_string(),
        },)
    );
}

#[test]
fn ceiling_bound_outside_contract_schema_refused() {
    let index = hub_index();
    let bounded = bounded_constrainable();
    let outside = ceiling_entry(
        bounded.id.as_str(),
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::from([("wait_seconds".to_owned(), 90i64)]),
    );
    assert_eq!(
        enabled_summary(vec![outside]).validate_against(&index),
        Err(CeilingError::BoundOutsideSchema {
            contract: bounded.id.clone(),
            key: "wait_seconds".to_string(),
        },)
    );
    // The containment-only fields stay off this entry: it bounds a `mutate` contract, whose only
    // limits are its rate and its argument bounds. `containment_keeps_both_limits` covers the
    // containment side.
    let inside = ceiling_entry(
        bounded.id.as_str(),
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::from([("wait_seconds".to_owned(), 30i64)]),
    );
    enabled_summary(vec![inside])
        .validate_against(&index)
        .unwrap();
}

#[test]
fn ceiling_names_targets_only_for_named_kinds() {
    let index = hub_index();
    let named = TargetName::parse("plex").unwrap();
    let against_host = ceiling_entry(
        "host.arrest",
        CeilingTargets::Names([named].into_iter().collect::<BTreeSet<TargetName>>()),
        4,
        None,
        None,
        BTreeMap::new(),
    );
    assert_eq!(
        enabled_summary(vec![against_host]).validate_against(&index),
        Err(CeilingError::NamesForUnnamedTarget {
            contract: ContractId::parse("host.arrest").unwrap(),
        })
    );
    let any_host = ceiling_entry(
        "host.arrest",
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::new(),
    );
    enabled_summary(vec![any_host])
        .validate_against(&index)
        .unwrap();
}

/// The hub's contracts for the containment-only fields: one `contain` helper capability, the only
/// class that has runs whose width and lifetime the ceiling can bound.
fn containment_index() -> ContractIndex {
    ContractIndex::new(vec![containment_contract()]).unwrap()
}

#[test]
fn containment_ceiling_keeps_active_and_ttl() {
    let index = containment_index();
    let entry = ceiling_entry(
        "host.jail",
        CeilingTargets::Any,
        2,
        Some(1),
        Some(SignedDuration::from_secs(900)),
        BTreeMap::new(),
    );
    enabled_summary(vec![entry])
        .validate_against(&index)
        .unwrap();
}

#[test]
fn ceiling_active_limit_refused_on_a_capability_that_does_not_contain() {
    for effect in [
        EffectClass::Read,
        EffectClass::Mutate,
        EffectClass::Destructive,
    ] {
        let contract = crafted("x.y", "x.y", effect);
        let index = ContractIndex::new(vec![contract]).unwrap();
        let entry = ceiling_entry(
            "x.y",
            CeilingTargets::Any,
            4,
            Some(2),
            None,
            BTreeMap::new(),
        );
        assert_eq!(
            enabled_summary(vec![entry]).validate_against(&index),
            Err(CeilingError::ActiveNotContainment {
                contract: ContractId::parse("x.y").unwrap(),
            },),
            "`max_active` on it would read as authoritative and bound nothing",
        );
    }
}

#[test]
fn ceiling_ttl_limit_refused_on_a_capability_that_does_not_contain() {
    for effect in [
        EffectClass::Read,
        EffectClass::Mutate,
        EffectClass::Destructive,
    ] {
        let contract = crafted("x.y", "x.y", effect);
        let index = ContractIndex::new(vec![contract]).unwrap();
        let entry = ceiling_entry(
            "x.y",
            CeilingTargets::Any,
            4,
            None,
            Some(SignedDuration::from_secs(900)),
            BTreeMap::new(),
        );
        assert_eq!(
            enabled_summary(vec![entry]).validate_against(&index),
            Err(CeilingError::TtlNotContainment {
                contract: ContractId::parse("x.y").unwrap(),
            },),
            "`max_ttl` on it would read as authoritative and bound nothing",
        );
    }
}

#[test]
fn ceiling_limits_are_optional_on_every_class() {
    let limits = ceiling_entry(
        "host.jail",
        CeilingTargets::Any,
        4,
        None,
        None,
        BTreeMap::new(),
    );
    enabled_summary(vec![limits])
        .validate_against(&containment_index())
        .unwrap();
    for contract_id in ["x.y", "host.arrest"] {
        let plain = ceiling_entry(
            contract_id,
            CeilingTargets::Any,
            4,
            None,
            None,
            BTreeMap::new(),
        );
        enabled_summary(vec![plain])
            .validate_against(&hub_index())
            .unwrap();
    }
}
