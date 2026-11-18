//! Tests for the contract types themselves. The module is pure, so no clock or fakes.
//!
//! The skill family and the built-in read table are covered in [`super::tests_skill`].

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use proptest::prelude::*;
use serde_json::json;

use super::builtin::READ_OUTPUT_LIMIT;
use super::ceiling::{CeilingEntry, CeilingSummary, CeilingTargets};
use super::contract::{
    CapabilityContract, ConstrainableArg, ConstraintKind, ContractError, ContractSource,
};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion, TargetIdentity, TargetName};
use super::index::{ContractIndex, IndexError};
use super::schema::SchemaError;
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

pub(super) fn crafted(id: &str, category: &str, effect: EffectClass) -> CapabilityContract {
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
            VerificationHint::Recheck {
                delay: SignedDuration::from_secs(0),
            }
        },
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::Agent,
        source: ContractSource::BuiltIn,
    }
}

pub(super) fn crafted_skill(
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
        Err(ContractError::BuiltInNeedsReversibilityAndVerification {
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

/// A contract that declares one bounded integer argument the ceiling may constrain.
pub(super) fn bounded_constrainable() -> CapabilityContract {
    let mut contract = crafted("x.y", "x.y", EffectClass::Mutate);
    contract.input_schema = json!({
        "type": "object",
        "properties": {
            "wait_seconds": {
                "type": "integer",
                "minimum": 0,
                "maximum": 60,
                "description": "How long to wait",
            },
        },
        "required": [],
        "additionalProperties": false,
    });
    contract.constrainable_args = vec![ConstrainableArg {
        key: "wait_seconds".to_owned(),
        kind: ConstraintKind::IntAtMost,
        description: "Most seconds the host may wait".to_owned(),
    }];
    contract
}

#[test]
fn output_limit_outside_transport_rejected() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Read);
    contract.output_limit_bytes = 0;
    assert_eq!(
        contract.validate(),
        Err(ContractError::OutputLimit {
            id: contract.id.clone(),
            bytes: 0
        })
    );
    let mut too_big = crafted("x.y", "x.y", EffectClass::Read);
    too_big.output_limit_bytes = 1_048_577;
    assert_eq!(
        too_big.validate(),
        Err(ContractError::OutputLimit {
            id: too_big.id.clone(),
            bytes: 1_048_577
        })
    );
}

#[test]
fn timeout_outside_range_rejected() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Read);
    contract.timeout = SignedDuration::ZERO;
    assert_eq!(
        contract.validate(),
        Err(ContractError::Timeout {
            id: contract.id.clone()
        })
    );
    let mut too_long = crafted("x.y", "x.y", EffectClass::Read);
    too_long.timeout = SignedDuration::from_hours(2);
    assert_eq!(
        too_long.validate(),
        Err(ContractError::Timeout {
            id: too_long.id.clone()
        })
    );
}

#[test]
fn blank_title_or_description_rejected() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Read);
    contract.title = " ".to_string();
    assert_eq!(
        contract.validate(),
        Err(ContractError::BlankText {
            id: contract.id.clone(),
            field: "title"
        })
    );
    let mut blank_description = crafted("x.y", "x.y", EffectClass::Read);
    blank_description.description = " ".to_string();
    assert_eq!(
        blank_description.validate(),
        Err(ContractError::BlankText {
            id: blank_description.id.clone(),
            field: "description"
        })
    );
}

#[test]
fn recheck_delay_outside_range_rejected() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Mutate);
    contract.verification = VerificationHint::Recheck {
        delay: SignedDuration::from_hours(2),
    };
    assert_eq!(
        contract.validate(),
        Err(ContractError::RecheckDelayInvalid {
            id: contract.id.clone()
        })
    );
    let mut waiting = crafted("x.y", "x.y", EffectClass::Mutate);
    waiting.verification = VerificationHint::Recheck {
        delay: SignedDuration::ZERO,
    };
    waiting.validate().unwrap();
}

#[test]
fn target_argument_agrees_with_target_kind() {
    let mut host_with_argument = crafted("host.restart", "host.mutate", EffectClass::Mutate);
    host_with_argument.target_arg = Some("display".to_owned());
    assert_eq!(
        host_with_argument.validate(),
        Err(ContractError::TargetArgMismatch {
            id: host_with_argument.id.clone(),
        })
    );
    let mut unit_without_argument = crafted("unit.restart", "unit.mutate", EffectClass::Mutate);
    unit_without_argument.target_kind = TargetKind::Unit;
    assert_eq!(
        unit_without_argument.validate(),
        Err(ContractError::TargetArgMismatch {
            id: unit_without_argument.id.clone(),
        })
    );
}

#[test]
fn target_argument_name_must_be_an_argument() {
    let mut contract = crafted("container.kill", "container.mutate", EffectClass::Mutate);
    contract.target_kind = TargetKind::Container;
    contract.target_arg = Some("Bad Name".to_owned());
    assert_eq!(
        contract.validate(),
        Err(ContractError::TargetArgBadName {
            id: contract.id.clone(),
            key: "Bad Name".to_string(),
        })
    );
}

#[test]
fn target_argument_must_be_declared_text_and_required() {
    let mut undeclared = crafted("container.kill", "container.mutate", EffectClass::Mutate);
    undeclared.target_kind = TargetKind::Container;
    undeclared.target_arg = Some("container".to_owned());
    assert_eq!(
        undeclared.validate(),
        Err(ContractError::TargetArgUndeclared {
            id: undeclared.id.clone(),
            key: "container".to_string(),
        })
    );

    let mut not_text = crafted("container.kill", "container.mutate", EffectClass::Mutate);
    not_text.target_kind = TargetKind::Container;
    not_text.target_arg = Some("container".to_owned());
    not_text.input_schema = json!({
        "type": "object",
        "properties": {
            "container": {
                "type": "integer",
                "minimum": 0,
                "maximum": 10,
                "description": "Which one",
            },
        },
        "required": ["container"],
        "additionalProperties": false,
    });
    assert_eq!(
        not_text.validate(),
        Err(ContractError::TargetArgNotText {
            id: not_text.id.clone(),
            key: "container".to_string(),
        })
    );

    let mut not_required = crafted("container.kill", "container.mutate", EffectClass::Mutate);
    not_required.target_kind = TargetKind::Container;
    not_required.target_arg = Some("container".to_owned());
    not_required.input_schema = json!({
        "type": "object",
        "properties": {
            "container": {
                "type": "string",
                "pattern": "^[a-z]+$",
                "description": "Which one",
            },
        },
        "required": [],
        "additionalProperties": false,
    });
    assert_eq!(
        not_required.validate(),
        Err(ContractError::TargetArgNotRequired {
            id: not_required.id.clone(),
            key: "container".to_string(),
        })
    );
}

#[test]
fn invalid_constrainable_arg_rejected() {
    let mut contract = bounded_constrainable();
    contract.constrainable_args[0].key = "Wait Seconds".to_string();
    assert_eq!(
        contract.validate(),
        Err(ContractError::BadConstrainableKey {
            id: contract.id.clone(),
            key: "Wait Seconds".to_string(),
        },)
    );

    let mut duplicate = bounded_constrainable();
    duplicate
        .constrainable_args
        .push(duplicate.constrainable_args[0].clone());
    assert_eq!(
        duplicate.validate(),
        Err(ContractError::DuplicateConstrainableArg {
            id: duplicate.id.clone(),
            key: "wait_seconds".to_string(),
        },)
    );

    let mut undeclared = bounded_constrainable();
    undeclared.constrainable_args[0].key = "extra".to_owned();
    assert_eq!(
        undeclared.validate(),
        Err(ContractError::ConstrainableUndeclared {
            id: undeclared.id.clone(),
            key: "extra".to_string(),
        },)
    );

    let mut not_integer = bounded_constrainable();
    not_integer.input_schema = json!({
        "type": "object",
        "properties": {
            "wait_seconds": {
                "type": "string",
                "pattern": "^[0-9]+$",
                "description": "How long to wait",
            },
        },
        "required": [],
        "additionalProperties": false,
    });
    assert_eq!(
        not_integer.validate(),
        Err(ContractError::ConstrainableNotInteger {
            id: not_integer.id.clone(),
            key: "wait_seconds".to_string(),
        },)
    );

    let mut unbounded = bounded_constrainable();
    unbounded.input_schema = json!({
        "type": "object",
        "properties": {
            "wait_seconds": { "type": "integer", "description": "How long to wait" },
        },
        "required": [],
        "additionalProperties": false,
    });
    assert_eq!(
        unbounded.validate(),
        Err(ContractError::ConstrainableUnbounded {
            id: unbounded.id.clone(),
            key: "wait_seconds".to_string(),
        },)
    );

    let mut blank = bounded_constrainable();
    blank.constrainable_args[0].description = " ".to_string();
    assert_eq!(
        blank.validate(),
        Err(ContractError::ConstrainableBlankDescription {
            id: blank.id.clone(),
            key: "wait_seconds".to_string(),
        },)
    );
}

#[test]
fn contract_schema_structure_must_validate() {
    bounded_constrainable().validate().unwrap();
    let mut not_an_object = crafted("x.y", "x.y", EffectClass::Read);
    not_an_object.input_schema = json!([]);
    assert_eq!(
        not_an_object.validate(),
        Err(ContractError::SchemaInvalid {
            id: not_an_object.id.clone(),
            reason: SchemaError::NotObject,
        },)
    );
    let mut open = crafted("x.y", "x.y", EffectClass::Read);
    open.input_schema = json!({
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": true,
    });
    assert_eq!(
        open.validate(),
        Err(ContractError::SchemaInvalid {
            id: open.id.clone(),
            reason: SchemaError::OpenAdditionalProperties,
        },)
    );
}

#[test]
fn built_in_may_not_take_skill_category() {
    let contract = crafted("x.y", "skill.mutate", EffectClass::Mutate);
    assert_eq!(
        contract.validate(),
        Err(ContractError::BuiltInSkillCategory {
            id: contract.id.clone()
        })
    );
}

#[test]
fn unsupported_execution_path_for_read_rejected() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Read);
    contract.execution = ExecutionPath::Helper;
    assert_eq!(
        contract.validate(),
        Err(ContractError::HelperPathForRead {
            id: contract.id.clone()
        })
    );
}

#[test]
fn contain_requires_helper_path() {
    let contract = crafted("host.arrest", "host.mutate", EffectClass::Contain);
    assert_eq!(
        contract.validate(),
        Err(ContractError::ContainNeedsHelper {
            id: contract.id.clone()
        })
    );
    let mut privileged = crafted("host.arrest", "host.mutate", EffectClass::Contain);
    privileged.execution = ExecutionPath::Helper;
    privileged.validate().unwrap();
}

#[test]
fn empty_index_means_nothing_composed() {
    let index = ContractIndex::new(Vec::new()).unwrap();
    assert!(index.is_empty());
    assert!(
        index
            .effect_of_category(&ContractCategory::parse("x.y").unwrap())
            .is_none()
    );
}

#[test]
fn index_refuses_an_unvalidated_contract() {
    let mut broken = crafted("x.y", "x.y", EffectClass::Read);
    broken.timeout = SignedDuration::ZERO;
    assert_eq!(
        ContractIndex::new(vec![broken.clone()]).unwrap_err(),
        IndexError::Invalid(ContractError::Timeout {
            id: broken.id.clone()
        })
    );
}

#[test]
fn composed_index_holds_every_builtin_once() {
    let skill = crafted_skill(
        "skill.restart",
        EffectClass::Mutate,
        None,
        EffectClass::Mutate,
    );
    let index = ContractIndex::with_builtins(vec![skill]).unwrap();
    assert_eq!(index.len(), 11);
    assert_eq!(
        index.effect_of_category(&ContractCategory::parse("skill.mutate").unwrap()),
        Some(EffectClass::Mutate)
    );

    let impostor = crafted_skill(
        "container.list",
        EffectClass::Mutate,
        None,
        EffectClass::Mutate,
    );
    assert_eq!(
        ContractIndex::with_builtins(vec![impostor]).unwrap_err(),
        IndexError::Invalid(ContractError::SkillSourceMismatch {
            id: ContractId::parse("container.list").unwrap(),
        },)
    );

    let hidden_builtin = crafted("container.list", "container.observe", EffectClass::Read);
    assert_eq!(
        ContractIndex::with_builtins(vec![hidden_builtin.clone()]).unwrap_err(),
        IndexError::NotASkill {
            id: hidden_builtin.id.clone()
        }
    );
}
