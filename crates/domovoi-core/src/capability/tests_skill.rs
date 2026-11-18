//! Tests for the `skill.<name>` family and the built-in read contract table.

use serde_json::json;

use super::builtin::{builtin_contracts, contract_id_of};
use super::contract::ContractSource;
use super::effect::{EffectClass, TargetKind};
use super::ids::ContractId;
use super::index::ContractIndex;
use super::skill::{FloorError, SkillFloor, skill_contract, skill_input_schema};
use super::tests::{summary, verify};
use crate::skill::{ParamKind, ParamName, ParamSpec, Risk};
use crate::triage::ReadTool;

#[test]
fn provider_cannot_lower_effect_class() {
    let destructive = summary("wipe", Risk::Destructive, None);
    let contract = skill_contract(
        &destructive,
        Some(SkillFloor::new(EffectClass::Read).unwrap()),
    );
    assert_eq!(contract.effect, EffectClass::Destructive);

    let read = summary("look", Risk::Read, None);
    let contract = skill_contract(&read, Some(SkillFloor::new(EffectClass::Mutate).unwrap()));
    assert_eq!(contract.effect, EffectClass::Mutate);

    assert_eq!(
        EffectClass::from_declared_risk("pwn"),
        EffectClass::Destructive,
        "an unknown declaration is the most dangerous class"
    );

    let named = summary("docker_ps", Risk::Write, Some(verify()));
    let contract = skill_contract(&named, None);
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
            let contract = skill_contract(&summary("backup_db", risk, verify.clone()), None);
            contract.validate().unwrap();
            let raised = skill_contract(
                &summary("backup_db", risk, verify),
                Some(SkillFloor::new(EffectClass::Destructive).unwrap()),
            );
            raised.validate().unwrap();
        }
    }
}

#[test]
fn skill_contract_category_follows_effective_class() {
    let contract = skill_contract(
        &summary("backup_db", Risk::Read, None),
        Some(SkillFloor::new(EffectClass::Destructive).unwrap()),
    );
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
                "minimum": 0,
                "maximum": 10,
                "description": "How many",
            },
            "force": {
                "type": "boolean",
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
    assert_eq!(skill_input_schema(&params), expected);
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
