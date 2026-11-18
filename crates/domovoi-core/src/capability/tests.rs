//! Tests for the contract types themselves. The module is pure, so no clock or fakes.
//!
//! The skill family and the built-in read table are covered in [`super::tests_skill`].

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use proptest::prelude::*;
use serde_json::{Value, json};

use super::builtin::READ_OUTPUT_LIMIT;
use super::ceiling::{CeilingEntry, CeilingSummary, CeilingTargets};
use super::contract::{
    CapabilityContract, ConstrainableArg, ConstraintKind, Containment, ContractError,
    ContractSource, ExpiryOwner, RollbackDeclaration,
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
        containment: None,
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
fn skill_id_carries_one_whole_skill_name() {
    // `skill.` on its own never reaches here: the id grammar refuses it at the boundary
    // (`skill_ids_stay_in_skill_namespace` and the parse tests).
    let canonical = crafted_skill(
        "skill.backup_db",
        EffectClass::Mutate,
        None,
        EffectClass::Mutate,
    );
    canonical.validate().unwrap();
    let split_name = crafted_skill(
        "skill.backup_db.extra",
        EffectClass::Mutate,
        None,
        EffectClass::Mutate,
    );
    assert_eq!(
        split_name.validate(),
        Err(ContractError::SkillIdNotCanonical {
            id: split_name.id.clone()
        }),
        "the runner would load a manifest that no skill name matches",
    );
    let impostor = crafted("skill.backup_db", "skill.mutate", EffectClass::Mutate);
    assert_eq!(
        impostor.validate(),
        Err(ContractError::SkillSourceMismatch {
            id: impostor.id.clone()
        }),
        "a built-in may not wear a skill id",
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

/// The containment guarantee a helper jail states, in the form §9.2 asks for: the TTL argument,
/// a kernel timer that ends the containment without the agent, a watchdog of its own because the
/// jail blocks a network path, and a journalled apply and undo.
fn jail_containment() -> Containment {
    Containment {
        ttl_argument: "hold_seconds".to_owned(),
        expiry: ExpiryOwner::KernelTimer,
        watchdog_argument: Some("watchdog_seconds".to_owned()),
        rollback: RollbackDeclaration::Journaled,
    }
}

/// The TTL definition the accepted jail contract states: a whole number of seconds from one
/// second to an hour.
fn jail_ttl() -> Value {
    json!({
        "type": "integer",
        "pattern": "^-?[0-9]+$",
        "minimum": 1,
        "maximum": 3600,
        "description": "How long the containment holds",
    })
}

/// The arguments the accepted jail contract requires: it cannot run without a TTL or leave the
/// watchdog unarmed.
fn jail_required() -> Vec<String> {
    ["hold_seconds".to_owned(), "watchdog_seconds".to_owned()].to_vec()
}

/// The schema of the jail contract, carrying `hold` as its TTL argument.
fn jail_schema(hold: Value, required: Vec<String>) -> Value {
    json!({
        "type": "object",
        "properties": {
            "hold_seconds": hold,
            "watchdog_seconds": {
                "type": "integer",
                "pattern": "^-?[0-9]+$",
                "minimum": 1,
                "maximum": 600,
                "description": "How long the watchdog of its own waits",
            },
        },
        "required": required,
        "additionalProperties": false,
    })
}

/// A helper containment contract naming `spec` over a TTL declared as `hold` and a required list
/// of `required`, so a test can break exactly one containment guarantee.
pub(super) fn containment_over(
    spec: Containment,
    hold: Value,
    required: Vec<String>,
) -> CapabilityContract {
    let mut contract = crafted("host.jail", "host.contain", EffectClass::Contain);
    contract.execution = ExecutionPath::Helper;
    contract.input_schema = jail_schema(hold, required);
    contract.containment = Some(spec);
    contract
}

/// The containment contract the hub accepts: a helper jail that says when its containment ends,
/// how it is undone and what arms its watchdog.
pub(super) fn containment_contract() -> CapabilityContract {
    containment_over(jail_containment(), jail_ttl(), jail_required())
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
fn blank_postcondition_rejected() {
    for description in ["".to_owned(), "   ".to_owned(), "\t".to_owned()] {
        let mut contract = crafted("x.y", "x.y", EffectClass::Mutate);
        contract.verification = VerificationHint::Postcondition {
            description: description.clone(),
        };
        assert_eq!(
            contract.validate(),
            Err(ContractError::BlankPostcondition {
                id: contract.id.clone()
            }),
            "a mutating built-in cannot answer its verification with empty metadata",
        );
    }
}

#[test]
fn meaningful_postcondition_is_kept() {
    let mut contract = crafted("x.y", "x.y", EffectClass::Mutate);
    contract.verification = VerificationHint::Postcondition {
        description: "The service reports running again".to_owned(),
    };
    contract.validate().unwrap();
}

#[test]
fn the_three_verification_kinds_stay_apart() {
    let mut without = crafted("x.y", "x.y", EffectClass::Mutate);
    without.verification = VerificationHint::None;
    assert_eq!(
        without.validate(),
        Err(ContractError::BuiltInNeedsReversibilityAndVerification {
            id: without.id.clone(),
        },),
        "declaring none is not the same as declaring a hollow one",
    );
    let mut waiting = crafted("x.y", "x.y", EffectClass::Mutate);
    waiting.verification = VerificationHint::Recheck {
        delay: SignedDuration::from_hours(2),
    };
    assert_eq!(
        waiting.validate(),
        Err(ContractError::RecheckDelayInvalid {
            id: waiting.id.clone()
        }),
        "a recheck waits",
    );
    let mut hollow = crafted("x.y", "x.y", EffectClass::Mutate);
    hollow.verification = VerificationHint::Postcondition {
        description: " ".to_owned(),
    };
    assert_eq!(
        hollow.validate(),
        Err(ContractError::BlankPostcondition {
            id: hollow.id.clone()
        }),
        "a postcondition says what must hold",
    );
    assert_ne!(
        VerificationHint::None,
        VerificationHint::Postcondition {
            description: " ".to_owned()
        },
        "no procedure is not a procedure that says nothing",
    );
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
    assert_eq!(
        privileged.validate(),
        Err(ContractError::ContainmentUndeclared {
            id: privileged.id.clone()
        }),
        "the helper path alone installs nothing: an empty schema cannot name a TTL",
    );
}

#[test]
fn accepted_helper_containment_states_every_guarantee() {
    containment_contract().validate().unwrap();
    assert_eq!(
        containment_contract().containment,
        Some(jail_containment()),
        "the accepted shape carries the TTL argument, the independent expiry, the watchdog and the journalled undo",
    );
}

#[test]
fn containment_ttl_argument_must_be_declared_and_well_named() {
    let undeclared = Containment {
        ttl_argument: "ghost_ttl".to_owned(),
        expiry: ExpiryOwner::KernelTimer,
        watchdog_argument: Some("watchdog_seconds".to_owned()),
        rollback: RollbackDeclaration::Journaled,
    };
    let contract = containment_over(undeclared, jail_ttl(), jail_required());
    assert_eq!(
        contract.validate(),
        Err(ContractError::ContainmentArgumentUndeclared {
            id: contract.id.clone(),
            key: "ghost_ttl".to_owned(),
        },),
        "a ceiling's `max_ttl` cannot stand in for a TTL the contract carries",
    );
    let badly_named = Containment {
        ttl_argument: "Hold Seconds".to_owned(),
        expiry: ExpiryOwner::KernelTimer,
        watchdog_argument: Some("watchdog_seconds".to_owned()),
        rollback: RollbackDeclaration::Journaled,
    };
    let misnamed = containment_over(badly_named, jail_ttl(), jail_required());
    assert_eq!(
        misnamed.validate(),
        Err(ContractError::ContainmentArgumentBadName {
            id: misnamed.id.clone(),
            key: "Hold Seconds".to_owned(),
        },),
    );
}

#[test]
fn containment_ttl_argument_must_be_a_required_bounded_integer() {
    let not_integer = containment_over(
        jail_containment(),
        json!({
            "type": "string",
            "pattern": "^[0-9]+$",
            "minLength": 1,
            "maxLength": 4,
            "description": "Not a duration the host can compare",
        }),
        jail_required(),
    );
    assert_eq!(
        not_integer.validate(),
        Err(ContractError::ContainmentArgumentNotInteger {
            id: not_integer.id.clone(),
            key: "hold_seconds".to_owned(),
        },),
        "the TTL must be a whole number of seconds",
    );
    let mut required: Vec<String> = ["watchdog_seconds".to_owned()].to_vec();
    let optional_ttl = containment_over(jail_containment(), jail_ttl(), required.clone());
    assert_eq!(
        optional_ttl.validate(),
        Err(ContractError::ContainmentArgumentNotRequired {
            id: optional_ttl.id.clone(),
            key: "hold_seconds".to_owned(),
        },),
        "an optional TTL lets a run install a containment that never expires",
    );
    required.push("hold_seconds".to_owned());
    for unbounded in [
        json!({
            "type": "integer",
            "pattern": "^-?[0-9]+$",
            "minimum": 1,
            "description": "No upper bound",
        }),
        json!({
            "type": "integer",
            "pattern": "^-?[0-9]+$",
            "minimum": 0,
            "maximum": 3600,
            "description": "Zero seconds means it may never expire",
        }),
    ] {
        let unbounded_contract = containment_over(jail_containment(), unbounded, required.clone());
        assert_eq!(
            unbounded_contract.validate(),
            Err(ContractError::ContainmentRangeUnbounded {
                id: unbounded_contract.id.clone(),
                key: "hold_seconds".to_owned(),
            },),
            "the TTL it names must be positive and bounded",
        );
    }
}

#[test]
fn containment_expiry_and_rollback_must_be_independent_and_journalled() {
    let agent_held = Containment {
        ttl_argument: "hold_seconds".to_owned(),
        expiry: ExpiryOwner::AgentProcess,
        watchdog_argument: Some("watchdog_seconds".to_owned()),
        rollback: RollbackDeclaration::Journaled,
    };
    let dependent = containment_over(agent_held, jail_ttl(), jail_required());
    assert_eq!(
        dependent.validate(),
        Err(ContractError::ContainmentExpiryNotIndependent {
            id: dependent.id.clone()
        }),
        "ending the agent is not what ends a containment",
    );
    let blind = Containment {
        ttl_argument: "hold_seconds".to_owned(),
        expiry: ExpiryOwner::KernelTimer,
        watchdog_argument: Some("watchdog_seconds".to_owned()),
        rollback: RollbackDeclaration::Unrecorded,
    };
    let unrecoverable = containment_over(blind, jail_ttl(), jail_required());
    assert_eq!(
        unrecoverable.validate(),
        Err(ContractError::ContainmentRollbackNotJournalled {
            id: unrecoverable.id.clone()
        }),
        "a partial run must leave a host the hub can bring back",
    );
}

#[test]
fn connectivity_containment_needs_a_watchdog_argument() {
    let mut ghost = jail_containment();
    ghost.watchdog_argument = Some("ghost_watchdog".to_owned());
    let contract = containment_over(ghost, jail_ttl(), jail_required());
    assert_eq!(
        contract.validate(),
        Err(ContractError::ContainmentArgumentUndeclared {
            id: contract.id.clone(),
            key: "ghost_watchdog".to_owned(),
        },),
        "the watchdog it claims cannot be armed",
    );
    let mut quiet = jail_containment();
    quiet.watchdog_argument = None;
    assert!(
        containment_over(quiet, jail_ttl(), jail_required())
            .validate()
            .is_ok(),
        "a containment that does not affect connectivity needs no watchdog"
    );
}

#[test]
fn containment_guarantee_only_belongs_to_a_contain_contract() {
    let mut mutate = crafted("host.arrest", "host.mutate", EffectClass::Mutate);
    mutate.containment = Some(jail_containment());
    assert_eq!(
        mutate.validate(),
        Err(ContractError::ContainmentNotApplicable {
            id: mutate.id.clone()
        }),
        "the guarantee would read as authoritative for nothing installed",
    );
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
