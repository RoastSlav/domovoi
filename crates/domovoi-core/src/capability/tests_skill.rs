//! Tests for the `skill.<name>` family and the built-in read contract table.

use jiff::SignedDuration;

use serde_json::json;

use super::builtin::{builtin_contracts, contract_id_of};
use super::contract::ContractSource;
use super::effect::{EffectClass, TargetKind, VerificationHint};
use super::ids::{ContractCategory, ContractId};
use super::index::ContractIndex;
use super::schema::validate_schema;
use super::skill::{
    FloorError, SkillContractError, SkillFloor, skill_contract, skill_input_schema,
};
use super::tests::{summary, verify};
use crate::skill::{ParamKind, ParamName, ParamSpec, Risk, SkillName, SkillSummary, VerifySpec};
use crate::triage::{MAX_SKILL_ARGS, ReadTool};

#[test]
fn provider_cannot_lower_effect_class() {
    let destructive = summary("wipe", Risk::Destructive, None);
    let contract = skill_contract(
        &destructive,
        Some(SkillFloor::new(EffectClass::Read).unwrap()),
    )
    .unwrap();
    assert_eq!(contract.effect, EffectClass::Destructive);

    let read = summary("look", Risk::Read, None);
    let contract =
        skill_contract(&read, Some(SkillFloor::new(EffectClass::Mutate).unwrap())).unwrap();
    assert_eq!(contract.effect, EffectClass::Mutate);

    assert_eq!(
        EffectClass::from_declared_risk("pwn"),
        EffectClass::Destructive,
        "an unknown declaration is the most dangerous class"
    );

    let named = summary("docker_ps", Risk::Write, Some(verify()));
    let contract = skill_contract(&named, None).unwrap();
    assert_eq!(contract.id.as_str(), "skill.docker_ps");
    assert_ne!(contract.id.as_str(), "container.list");
}

#[test]
fn skill_floor_rejects_contain() {
    assert_eq!(
        SkillFloor::new(EffectClass::Contain),
        Err(FloorError::ContainNotAllowed)
    );
    for class in [
        EffectClass::Read,
        EffectClass::Mutate,
        EffectClass::Destructive,
    ] {
        assert!(SkillFloor::new(class).is_ok());
    }
}

#[test]
fn skill_contract_validates_for_every_risk() {
    for risk in [Risk::Read, Risk::Write, Risk::Destructive] {
        for verify in [None, Some(verify())] {
            if risk == Risk::Read && verify.is_some() {
                assert!(
                    matches!(
                        skill_contract(&summary("backup_db", risk, verify.clone()), None),
                        Err(SkillContractError::VerifyForRead { .. }),
                    ),
                    "a read skill that declares a verification is refused",
                );
                continue;
            }
            let contract =
                skill_contract(&summary("backup_db", risk, verify.clone()), None).unwrap();
            contract.validate().unwrap();
            let raised = skill_contract(
                &summary("backup_db", risk, verify),
                Some(SkillFloor::new(EffectClass::Destructive).unwrap()),
            )
            .unwrap();
            raised.validate().unwrap();
        }
    }
}

#[test]
fn skill_contract_category_follows_effective_class() {
    let contract = skill_contract(
        &summary("backup_db", Risk::Read, None),
        Some(SkillFloor::new(EffectClass::Destructive).unwrap()),
    )
    .unwrap();
    assert_eq!(contract.effect, EffectClass::Destructive);
    assert_eq!(contract.category.as_str(), "skill.destructive");
    assert_eq!(
        contract.source,
        ContractSource::Skill {
            declared: EffectClass::Read,
            floor: Some(EffectClass::Destructive),
        }
    );
}

#[test]
fn skill_input_schema_covers_every_param_kind() {
    let params = vec![
        ParamSpec {
            name: ParamName::parse("name").unwrap(),
            description: "A name".to_owned(),
            required: true,
            kind: ParamKind::String {
                pattern: "^[a-z]+$".to_owned(),
                max_len: 8,
            },
        },
        ParamSpec {
            name: ParamName::parse("count").unwrap(),
            description: "How many".to_owned(),
            required: false,
            kind: ParamKind::Integer { min: 0, max: 10 },
        },
        ParamSpec {
            name: ParamName::parse("force").unwrap(),
            description: "Force it".to_owned(),
            required: true,
            kind: ParamKind::Boolean,
        },
        ParamSpec {
            name: ParamName::parse("mode").unwrap(),
            description: "Which mode".to_owned(),
            required: false,
            kind: ParamKind::Enum {
                values: vec!["fast".to_owned(), "safe".to_owned()],
            },
        },
    ];
    let expected = json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "pattern": "^[a-z]+$",
                "maxLength": 8,
                "description": "A name",
            },
            "count": {
                "type": "integer",
                "pattern": "^-?[0-9]+$",
                "minimum": 0,
                "maximum": 10,
                "description": "How many",
            },
            "force": {
                "type": "boolean",
                "pattern": "^(true|false)$",
                "description": "Force it",
            },
            "mode": {
                "type": "string",
                "enum": ["fast", "safe"],
                "description": "Which mode",
            },
        },
        "required": ["name", "force"],
        "additionalProperties": false,
    });
    assert_eq!(
        skill_input_schema(SkillName::parse("deploy").unwrap(), &params),
        Ok(expected),
    );
}

#[test]
fn builtin_contracts_cover_every_read_tool() {
    let expected = [
        "container.list",
        "container.logs",
        "container.inspect",
        "container.events",
        "host.overview",
        "system.journal",
        "network.listening_ports",
        "disk.smart_report",
        "incident.history",
        "notes.search",
    ];
    let ids: Vec<String> = builtin_contracts()
        .iter()
        .map(|contract| contract.id.to_string())
        .collect();
    assert_eq!(ids, expected);
    let from_tools: Vec<ContractId> = ReadTool::ALL
        .iter()
        .filter_map(|tool| contract_id_of(*tool))
        .collect();
    assert_eq!(ids.len(), from_tools.len());
    assert_eq!(
        from_tools
            .iter()
            .map(ContractId::as_str)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(contract_id_of(ReadTool::RunReadSkill), None);
}

#[test]
fn builtin_contracts_validate_and_index() {
    let contracts = builtin_contracts();
    for contract in &contracts {
        contract.validate().unwrap();
    }
    let index = ContractIndex::new(contracts).unwrap();
    assert_eq!(index.len(), 10);
}

#[test]
fn builtin_input_schema_matches_read_tool_schema() {
    let index = ContractIndex::new(builtin_contracts()).unwrap();
    for tool in ReadTool::ALL {
        let Some(id) = contract_id_of(tool) else {
            continue;
        };
        let contract = index.get(&id).unwrap();
        assert_eq!(contract.input_schema, tool.json_schema(), "{id}");
    }
}

#[test]
fn builtin_container_target_args_exist_in_schema() {
    let index = ContractIndex::new(builtin_contracts()).unwrap();
    for id in ["container.logs", "container.inspect"] {
        let id = ContractId::parse(id).unwrap();
        let contract = index.get(&id).unwrap();
        assert_eq!(contract.target_arg.as_deref(), Some("container"));
        assert!(
            contract.input_schema["properties"]
                .get("container")
                .is_some(),
            "{id} names a container argument its schema does not declare"
        );
    }
}

#[test]
fn index_meta_exposes_category_effect_and_facts() {
    let index = ContractIndex::new(builtin_contracts()).unwrap();
    let id = ContractId::parse("container.logs").unwrap();
    let meta = index.meta(&id).unwrap();
    assert_eq!(meta.category.as_str(), "container.observe");
    assert_eq!(meta.effect, EffectClass::Read);
    assert_eq!(meta.target_kind, TargetKind::Container);
    assert!(meta.safety_facts.is_empty());
    assert_eq!(
        index.effect_of_category(&meta.category),
        Some(EffectClass::Read)
    );
}

/// A summary of one skill whose single declared parameter has the given kind.
fn summary_with_param(kind: ParamKind) -> SkillSummary {
    let mut made = summary("tune_service", Risk::Write, None);
    made.params = vec![ParamSpec {
        name: ParamName::parse("level").unwrap(),
        description: "A level".to_owned(),
        required: false,
        kind,
    }];
    made
}

#[test]
fn skill_contract_timeout_out_of_range_rejected() {
    for timeout in [
        SignedDuration::ZERO,
        SignedDuration::from_millis(999),
        SignedDuration::from_hours(2),
    ] {
        let mut made = summary("slow_one", Risk::Write, None);
        made.timeout = timeout;
        assert_eq!(
            skill_contract(&made, None),
            Err(SkillContractError::Timeout {
                skill: SkillName::parse("slow_one").unwrap()
            }),
        );
    }
}

#[test]
fn skill_verify_delay_out_of_range_rejected() {
    let made = summary(
        "restart_one",
        Risk::Write,
        Some(VerifySpec {
            recheck: true,
            delay: SignedDuration::from_hours(2),
        }),
    );
    assert_eq!(
        skill_contract(&made, None),
        Err(SkillContractError::VerifyDelay {
            skill: SkillName::parse("restart_one").unwrap()
        }),
    );
}

#[test]
fn verify_delay_preserved_in_recheck_hint() {
    let made = summary(
        "restart_one",
        Risk::Write,
        Some(VerifySpec {
            recheck: true,
            delay: SignedDuration::from_secs(45),
        }),
    );
    let contract = skill_contract(&made, None).unwrap();
    assert_eq!(
        contract.verification,
        VerificationHint::Recheck {
            delay: SignedDuration::from_secs(45)
        },
    );
}

#[test]
fn missing_verification_stays_distinct_from_postcondition() {
    let without = skill_contract(&summary("wipe", Risk::Destructive, None), None).unwrap();
    assert_eq!(without.verification, VerificationHint::None);
    let declared_off = skill_contract(
        &summary(
            "wipe",
            Risk::Destructive,
            Some(VerifySpec {
                recheck: false,
                delay: SignedDuration::from_secs(5),
            }),
        ),
        None,
    )
    .unwrap();
    assert_eq!(declared_off.verification, VerificationHint::None);
    let immediate = skill_contract(
        &summary(
            "wipe",
            Risk::Destructive,
            Some(VerifySpec {
                recheck: true,
                delay: SignedDuration::ZERO,
            }),
        ),
        None,
    )
    .unwrap();
    assert_eq!(
        immediate.verification,
        VerificationHint::Recheck {
            delay: SignedDuration::ZERO
        },
    );
}

#[test]
fn skill_contract_refuses_unusable_parameter() {
    assert_eq!(
        skill_contract(
            &summary_with_param(ParamKind::Integer { min: 5, max: 1 }),
            None
        ),
        Err(SkillContractError::ParamKind {
            param: "level".to_owned(),
            reason: "min must not exceed max".to_string(),
        },)
    );
    assert_eq!(
        skill_contract(
            &summary_with_param(ParamKind::Enum { values: Vec::new() }),
            None,
        ),
        Err(SkillContractError::ParamKind {
            param: "level".to_owned(),
            reason: "enum must have at least one value".to_string(),
        },)
    );
    assert_eq!(
        skill_contract(
            &summary_with_param(ParamKind::String {
                pattern: "[a-z]+".to_string(),
                max_len: 8
            }),
            None,
        ),
        Err(SkillContractError::ParamKind {
            param: "level".to_owned(),
            reason: "pattern must be anchored with ^ and $".to_string(),
        },)
    );
    assert_eq!(
        skill_contract(
            &summary_with_param(ParamKind::String {
                pattern: "^[a-z]+$".to_string(),
                max_len: 0
            }),
            None,
        ),
        Err(SkillContractError::ParamKind {
            param: "level".to_owned(),
            reason: "max_len must be between 1 and 1024".to_string(),
        },)
    );
}

#[test]
fn skill_contract_refuses_too_many_parameters() {
    let mut made = summary("many_params", Risk::Write, None);
    for index in 0..=MAX_SKILL_ARGS {
        made.params.push(ParamSpec {
            name: ParamName::parse(format!("p{}", index).as_str()).unwrap(),
            description: "A parameter".to_owned(),
            required: false,
            kind: ParamKind::Boolean,
        });
    }
    assert_eq!(
        skill_contract(&made, None),
        Err(SkillContractError::TooManyParams {
            skill: SkillName::parse("many_params").unwrap(),
            count: MAX_SKILL_ARGS + 1,
            limit: MAX_SKILL_ARGS,
        },)
    );
}

#[test]
fn skill_can_never_be_contain() {
    assert_eq!(
        SkillFloor::new(EffectClass::Contain),
        Err(FloorError::ContainNotAllowed)
    );
    assert_eq!(ContractCategory::skill(EffectClass::Contain), None);
    let contract = skill_contract(
        &summary("wipe", Risk::Write, None),
        Some(SkillFloor::new(EffectClass::Destructive).unwrap()),
    )
    .unwrap();
    assert_ne!(contract.effect, EffectClass::Contain);
    assert_ne!(contract.category.as_str(), "skill.contain");
    assert_eq!(
        contract.category,
        ContractCategory::skill(EffectClass::Destructive).unwrap()
    );
    assert_eq!(
        contract.category.skill_class(),
        Some(EffectClass::Destructive)
    );
}

#[test]
fn skill_input_schema_structure_is_valid() {
    let params = vec![
        ParamSpec {
            name: ParamName::parse("name").unwrap(),
            description: "A name".to_owned(),
            required: true,
            kind: ParamKind::String {
                pattern: "^[a-z]+$".to_string(),
                max_len: 8,
            },
        },
        ParamSpec {
            name: ParamName::parse("count").unwrap(),
            description: "How many".to_owned(),
            required: false,
            kind: ParamKind::Integer { min: 0, max: 10 },
        },
        ParamSpec {
            name: ParamName::parse("force").unwrap(),
            description: "Force it".to_owned(),
            required: true,
            kind: ParamKind::Boolean,
        },
        ParamSpec {
            name: ParamName::parse("mode").unwrap(),
            description: "Which mode".to_owned(),
            required: false,
            kind: ParamKind::Enum {
                values: vec!["fast".to_owned(), "safe".to_owned()],
            },
        },
    ];
    let schema = skill_input_schema(SkillName::parse("deploy").unwrap(), &params).unwrap();
    validate_schema(&schema).unwrap();
}

/// A summary of `tune_service` declaring two parameters that share the name `level`, which is
/// what a provider's manifest may hold before the hub has checked it.
fn summary_duplicated(
    first: ParamKind,
    second: ParamKind,
    first_required: bool,
    second_required: bool,
) -> SkillSummary {
    let mut made = summary("tune_service", Risk::Write, None);
    made.params = vec![
        ParamSpec {
            name: ParamName::parse("level").unwrap(),
            description: "A level".to_owned(),
            required: first_required,
            kind: first,
        },
        ParamSpec {
            name: ParamName::parse("level").unwrap(),
            description: "Another level".to_owned(),
            required: second_required,
            kind: second,
        },
    ];
    made
}

#[test]
fn skill_contract_rejects_duplicate_optional_params() {
    let made = summary_duplicated(
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        ParamKind::String {
            pattern: "^[a-z0-9]+$".to_owned(),
            max_len: 4,
        },
        false,
        false,
    );
    assert_eq!(
        skill_contract(&made, None),
        Err(SkillContractError::DuplicateParam {
            skill: SkillName::parse("tune_service").unwrap(),
            param: "level".to_string(),
        },),
        "one declaration would be lost without a word about it",
    );
}

#[test]
fn skill_contract_rejects_duplicate_required_params() {
    let made = summary_duplicated(
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        true,
        true,
    );
    assert_eq!(
        skill_contract(&made, None),
        Err(SkillContractError::DuplicateParam {
            skill: SkillName::parse("tune_service").unwrap(),
            param: "level".to_string(),
        },),
    );
}

#[test]
fn skill_contract_rejects_duplicate_params_of_conflicting_kinds() {
    let made = summary_duplicated(
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        ParamKind::Integer { min: 0, max: 10 },
        true,
        false,
    );
    assert_eq!(
        skill_contract(&made, None),
        Err(SkillContractError::DuplicateParam {
            skill: SkillName::parse("tune_service").unwrap(),
            param: "level".to_string(),
        },),
        "the kind of the loser is unknowable",
    );
}

#[test]
fn skill_contract_rejects_duplicate_params_differing_only_in_required() {
    let made = summary_duplicated(
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        ParamKind::String {
            pattern: "^[a-z]+$".to_owned(),
            max_len: 8,
        },
        true,
        false,
    );
    assert!(
        matches!(
            skill_contract(&made, None),
            Err(SkillContractError::DuplicateParam { .. }),
        ),
        "the required flag of the loser is unknowable",
    );
}

#[test]
fn skill_contract_carries_every_distinct_declaration() {
    let mut made = summary("tune_service", Risk::Write, None);
    made.params = vec![
        ParamSpec {
            name: ParamName::parse("level").unwrap(),
            description: "A level".to_owned(),
            required: true,
            kind: ParamKind::String {
                pattern: "^[a-z]+$".to_owned(),
                max_len: 8,
            },
        },
        ParamSpec {
            name: ParamName::parse("count").unwrap(),
            description: "How many".to_owned(),
            required: false,
            kind: ParamKind::Integer { min: 0, max: 10 },
        },
    ];
    let contract = skill_contract(&made, None).unwrap();
    assert_eq!(
        contract.input_schema,
        json!({
            "type": "object",
            "properties": {
                "level": {
                    "type": "string",
                    "pattern": "^[a-z]+$",
                    "maxLength": 8,
                    "description": "A level",
                },
                "count": {
                    "type": "integer",
                    "pattern": "^-?[0-9]+$",
                    "minimum": 0,
                    "maximum": 10,
                    "description": "How many",
                },
            },
            "required": ["level"],
            "additionalProperties": false,
        }),
    );
}
