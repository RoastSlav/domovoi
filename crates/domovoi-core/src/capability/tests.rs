//! Tests for the contract types themselves. The module is pure, so no clock or fakes.
//!
//! The skill family and the built-in read table are covered in [`super::tests_skill`].

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use proptest::prelude::*;
use serde_json::json;

use super::builtin::READ_OUTPUT_LIMIT;
use super::ceiling::{CeilingEntry, CeilingSummary, CeilingTargets};
use super::contract::{CapabilityContract, ContractError, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion, TargetIdentity, TargetName};
use super::index::{ContractIndex, IndexError};
use super::skill::{SkillFloor, effective_effect};
use crate::ScriptDigest;
use crate::skill::{Risk, SkillName, SkillSummary, VerifySpec};

/// The summary of a skill with no parameters, for the tests that build contracts.
pub(super) fn summary(name: &str, risk: Risk, verify: Option<VerifySpec>) -> SkillSummary {
    SkillSummary {
        name: SkillName::parse(name).unwrap(),
        description: format!("{name} does something"),
        risk,
        params: Vec::new(),
        applies_to: Vec::new(),
        manifest_hash: "00".repeat(32).parse().unwrap(),
        entry_script_digest: ScriptDigest::of_bytes(b"entry"),
        timeout: SignedDuration::from_secs(60),
        verify,
        readme: None,
    }
}

/// A rechecking verify spec, for the tests that need one.
pub(super) fn verify() -> VerifySpec {
    VerifySpec {
        recheck: true,
        delay: SignedDuration::from_secs(30),
    }
}

fn crafted(id: &str, category: &str, effect: EffectClass) -> CapabilityContract {
    CapabilityContract {
        id: ContractId::parse(id).unwrap(),
        version: ContractVersion::new(1).unwrap(),
        title: "Crafted".to_owned(),
        description: "A contract built for a test.".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        }),
        effect,
        category: ContractCategory::parse(category).unwrap(),
        target_kind: TargetKind::Host,
        target_arg: None,
        reversibility: if effect == EffectClass::Read {
            Reversibility::NotApplicable
        } else {
            Reversibility::Reversible
        },
        idempotency: if effect == EffectClass::Read {
            Idempotency::Idempotent
        } else {
            Idempotency::NotIdempotent
        },
        timeout: SignedDuration::from_secs(15),
        output_limit_bytes: READ_OUTPUT_LIMIT,
        verification: if effect == EffectClass::Read {
            VerificationHint::None
        } else {
            VerificationHint::Recheck
        },
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::Agent,
        source: ContractSource::BuiltIn,
    }
}

fn crafted_skill(
    id: &str,
    declared: EffectClass,
    floor: Option<EffectClass>,
    effect: EffectClass,
) -> CapabilityContract {
    let mut contract = crafted(id, "skill.mutate", effect);
    contract.execution = ExecutionPath::SkillRunner;
    contract.source = ContractSource::Skill { declared, floor };
    contract
}

#[test]
fn contract_id_grammar_accepts_and_rejects() {
    for accepted in ["container.list", "skill.backup_db", "a.b.c"] {
        assert!(
            ContractId::parse(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
    let too_long = format!("a.{}", "b".repeat(79));
    for rejected in [
        "list",
        "Container.list",
        "container..list",
        "container.",
        ".x",
        "1a.b",
        "container.list ",
        "",
        too_long.as_str(),
    ] {
        assert!(
            ContractId::parse(rejected).is_err(),
            "`{rejected}` was accepted"
        );
    }
}

#[test]
fn target_name_rejects_control_and_padding() {
    for accepted in ["plex", "my container"] {
        assert!(
            TargetName::parse(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
    let too_long = "a".repeat(129);
    for rejected in ["\tplex", "plex ", "a\u{0}b", "", too_long.as_str()] {
        assert!(
            TargetName::parse(rejected).is_err(),
            "`{rejected}` was accepted"
        );
    }
}

#[test]
fn target_identity_requires_known_scheme() {
    for accepted in [
        "host:0190",
        "container:abc/def",
        "unit:media/plex.service",
        "ipnet:10.0.0.0/24",
    ] {
        assert!(
            TargetIdentity::parse(accepted).is_ok(),
            "`{accepted}` was refused"
        );
    }
    for rejected in ["file:/etc", "container:", "container:a b", "container"] {
        assert!(
            TargetIdentity::parse(rejected).is_err(),
            "`{rejected}` was accepted"
        );
    }
}

#[test]
fn contract_version_rejects_zero() {
    assert!(ContractVersion::new(0).is_err());
    assert_eq!(ContractVersion::new(1).unwrap().get(), 1);
}

#[test]
fn unknown_effect_is_destructive() {
    for text in ["", "READ", "mutate", "contain", "nonsense"] {
        assert_eq!(
            EffectClass::from_declared_risk(text),
            EffectClass::Destructive,
            "`{text}` was not treated as destructive"
        );
    }
    assert_eq!(EffectClass::from_declared_risk("read"), EffectClass::Read);
    assert_eq!(
        EffectClass::from_declared_risk("write"),
        EffectClass::Mutate
    );
    assert_eq!(
        EffectClass::from_declared_risk("destructive"),
        EffectClass::Destructive
    );
}

proptest! {
    #[test]
    fn effective_effect_never_below_declared_or_floor(declared in 0u8..4, floor in 0u8..4) {
        let declared = EffectClass::ALL[usize::from(declared)];
        let floor = SkillFloor::new(EffectClass::ALL[usize::from(floor)]).ok();
        let effect = effective_effect(declared, floor);
        prop_assert!(effect >= declared);
        if let Some(floor) = floor {
            prop_assert!(effect >= floor.class());
        }
    }
}

#[test]
fn category_belongs_to_exactly_one_effect() {
    let read = crafted("x.y", "same.category", EffectClass::Read);
    let mutate = crafted("x.z", "same.category", EffectClass::Mutate);
    assert_eq!(
        ContractIndex::new(vec![read, mutate]).unwrap_err(),
        IndexError::CategorySpansEffects {
            category: ContractCategory::parse("same.category").unwrap(),
            first: EffectClass::Read,
            second: EffectClass::Mutate,
        }
    );
}

#[test]
fn duplicate_contract_id_rejected() {
    let contract = crafted("x.y", "x.y", EffectClass::Read);
    assert_eq!(
        ContractIndex::new(vec![contract.clone(), contract]).unwrap_err(),
        IndexError::DuplicateId(ContractId::parse("x.y").unwrap())
    );
}

#[test]
fn validate_rejects_read_with_side_effect_fields() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Read);
    contract.reversibility = Reversibility::Reversible;
    assert_eq!(
        contract.validate(),
        Err(ContractError::ReadHasSideEffectFields {
            id: contract.id.clone(),
            field: "reversibility",
        })
    );
}

#[test]
fn validate_requires_verification_for_mutating() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Mutate);
    contract.verification = VerificationHint::None;
    assert_eq!(
        contract.validate(),
        Err(ContractError::MutatingNeedsReversibilityAndVerification {
            id: contract.id.clone(),
        })
    );
}

#[test]
fn validate_ties_skill_id_to_skill_source() {
    let built_in_with_skill_id = crafted("skill.x", "skill.mutate", EffectClass::Mutate);
    assert_eq!(
        built_in_with_skill_id.validate(),
        Err(ContractError::SkillSourceMismatch {
            id: built_in_with_skill_id.id.clone(),
        })
    );

    let skill_with_plain_id = crafted_skill(
        "container.restart",
        EffectClass::Mutate,
        None,
        EffectClass::Mutate,
    );
    assert_eq!(
        skill_with_plain_id.validate(),
        Err(ContractError::SkillSourceMismatch {
            id: skill_with_plain_id.id.clone(),
        })
    );
}

#[test]
fn validate_rejects_a_skill_that_is_contain() {
    let contract = crafted_skill("skill.x", EffectClass::Contain, None, EffectClass::Contain);
    assert_eq!(
        contract.validate(),
        Err(ContractError::SkillContain {
            id: contract.id.clone(),
        })
    );
}

#[test]
fn ceiling_summary_serde_round_trip() {
    let entry = CeilingEntry {
        contract: ContractId::parse("container.restart").unwrap(),
        targets: CeilingTargets::Names(
            [TargetName::parse("plex").unwrap()]
                .into_iter()
                .collect::<BTreeSet<_>>(),
        ),
        max_per_hour: 4,
        max_active: Some(1),
        max_ttl: Some(SignedDuration::from_secs(900)),
        args: BTreeMap::from([("wait_seconds".to_owned(), 30i64)]),
    };
    let summary = CeilingSummary {
        enabled: true,
        entries: vec![
            CeilingEntry {
                targets: CeilingTargets::Any,
                ..entry.clone()
            },
            entry,
        ],
        hash: Some("ab".repeat(32)),
    };
    let value = serde_json::to_value(&summary).unwrap();
    let round_tripped: CeilingSummary = serde_json::from_value(value).unwrap();
    assert_eq!(round_tripped, summary);
}

#[test]
fn ceiling_summary_missing_entries_refused() {
    let missing = json!({
        "enabled": true,
        "hash": "ab",
    });
    assert!(
        serde_json::from_value::<CeilingSummary>(missing).is_err(),
        "a summary without the entries key invents an empty ceiling"
    );
}

#[test]
fn ceiling_entry_missing_args_refused() {
    let missing = json!({
        "contract": "container.restart",
        "targets": {
            "kind": "any",
        },
        "max_per_hour": 4,
    });
    assert!(
        serde_json::from_value::<CeilingEntry>(missing).is_err(),
        "an entry without the args key claims bounds that were never stated"
    );
}
