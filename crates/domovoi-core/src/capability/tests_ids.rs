//! Tests for the identifier types at their serde boundary and the namespace rules
//! that separate the skill family from every built-in family.

use serde_json::json;

use super::effect::EffectClass;
use super::effect::TargetKind;
use super::ids::{
    ContractCategory, ContractId, ContractVersion, FactId, TargetIdentity, TargetName,
};
use crate::skill::SkillName;

#[test]
fn serde_refuses_malformed_contract_ids_from_json() {
    for rejected in [
        "",
        "list",
        "Container.list",
        "container.",
        "container.list ",
    ] {
        assert!(
            serde_json::from_value::<ContractId>(json!(rejected)).is_err(),
            "`{rejected}` was accepted from JSON"
        );
    }
    for rejected in ["", "Skill.mutate", "skill.", "a.b "] {
        assert!(
            serde_json::from_value::<ContractCategory>(json!(rejected)).is_err(),
            "`{rejected}` was accepted from JSON"
        );
    }
    for rejected in ["", "Fact.one", "_fact", "fact two"] {
        assert!(
            serde_json::from_value::<FactId>(json!(rejected)).is_err(),
            "`{rejected}` was accepted from JSON"
        );
    }
    for rejected in ["", " plex", "plex ", "a\u{0}b"] {
        assert!(
            serde_json::from_value::<TargetName>(json!(rejected)).is_err(),
            "`{rejected}` was accepted from JSON"
        );
    }
    for rejected in ["", "file:/etc", "container:", "container:a b"] {
        assert!(
            serde_json::from_value::<TargetIdentity>(json!(rejected)).is_err(),
            "`{rejected}` was accepted from JSON"
        );
    }
}

#[test]
fn serde_refuses_contract_version_zero() {
    assert!(
        serde_json::from_value::<ContractVersion>(json!(0)).is_err(),
        "a zero version deserialised from JSON"
    );
    let seven = ContractVersion::new(7).unwrap();
    assert_eq!(serde_json::to_value(seven).unwrap(), json!(7));
    assert_eq!(
        serde_json::from_value::<ContractVersion>(json!(7)).unwrap(),
        seven
    );
    assert!(
        serde_json::from_value::<ContractVersion>(json!(-1)).is_err(),
        "a negative version deserialised from JSON"
    );
}

#[test]
fn serde_round_trips_valid_names() {
    for id in ["container.list", "a.b.c", "skill.backup_db"] {
        let parsed = ContractId::parse(id).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(id));
        assert_eq!(
            serde_json::from_value::<ContractId>(json!(id)).unwrap(),
            parsed
        );
    }
    for category in ["container.observe", "skill.mutate"] {
        let parsed = ContractCategory::parse(category).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(category));
        assert_eq!(
            serde_json::from_value::<ContractCategory>(json!(category)).unwrap(),
            parsed
        );
    }
    for name in ["plex", "my container"] {
        let parsed = TargetName::parse(name).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<TargetName>(json!(name)).unwrap(),
            parsed
        );
    }
    for identity in ["host:0190", "container:abc/def", "ipnet:10.0.0.0/24"] {
        let parsed = TargetIdentity::parse(identity).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), json!(identity));
        assert_eq!(
            serde_json::from_value::<TargetIdentity>(json!(identity)).unwrap(),
            parsed
        );
    }
}

#[test]
fn skill_ids_stay_in_skill_namespace() {
    let name = SkillName::parse("backup_db").unwrap();
    assert_eq!(
        ContractId::skill(&name),
        ContractId::parse("skill.backup_db").unwrap()
    );
    assert!(ContractId::skill(&name).is_skill());
    assert_eq!(
        ContractId::skill(&name).skill_name().unwrap(),
        name,
        "the id must carry back the very skill name"
    );
    let built_in = ContractId::parse("container.list").unwrap();
    assert!(!built_in.is_skill());
    assert!(built_in.skill_name().is_none());
    assert_eq!(
        ContractId::skill(&SkillName::parse("list").unwrap()).as_str(),
        "skill.list",
        "a skill named like a built-in still owns a skill id"
    );
}

#[test]
fn skill_category_names_the_effect_class() {
    assert_eq!(
        ContractCategory::skill(EffectClass::Read),
        Some(ContractCategory::parse("skill.read").unwrap())
    );
    assert_eq!(
        ContractCategory::skill(EffectClass::Destructive),
        Some(ContractCategory::parse("skill.destructive").unwrap())
    );
    assert_eq!(
        ContractCategory::skill(EffectClass::Contain),
        None,
        "no skill category exists for contain"
    );
    assert_eq!(
        ContractCategory::parse("skill.mutate")
            .unwrap()
            .skill_class(),
        Some(EffectClass::Mutate)
    );
    assert!(
        ContractCategory::parse("skill.read")
            .unwrap()
            .is_skill_namespace()
    );
    assert!(
        !ContractCategory::parse("container.observe")
            .unwrap()
            .is_skill_namespace(),
        "a built-in category must not sit in the skill namespace"
    );
    assert!(
        ContractCategory::parse("container.observe")
            .unwrap()
            .is_observation()
    );
    assert!(
        !ContractCategory::parse("host.overview")
            .unwrap()
            .is_observation(),
        "only a built-in read category ends in `observe`"
    );
}

#[test]
fn identity_scheme_names_target_kind_except_ipnet() {
    let host = TargetIdentity::parse("host:0190").unwrap();
    assert_eq!(host.scheme(), "host");
    assert_eq!(host.target_kind(), Some(TargetKind::Host));
    for (identity, kind) in [
        ("container:abc/def", TargetKind::Container),
        ("unit:media/plex.service", TargetKind::Unit),
    ] {
        let parsed = TargetIdentity::parse(identity).unwrap();
        assert_eq!(
            parsed.target_kind(),
            Some(kind),
            "`{identity}` named another kind"
        );
    }
    let network = TargetIdentity::parse("ipnet:10.0.0.0/24").unwrap();
    assert_eq!(network.scheme(), "ipnet");
    assert_eq!(
        network.target_kind(),
        None,
        "no target kind exists for a network source yet"
    );
}

#[test]
fn identifier_length_boundaries() {
    let at_limit = format!("a.{}", "b".repeat(78));
    assert!(ContractId::parse(at_limit.as_str()).is_ok());
    assert!(ContractId::parse(format!("a.{}", "b".repeat(79)).as_str()).is_err());
    assert!(
        ContractCategory::parse(format!("x.{}", "y".repeat(62)).as_str()).is_ok(),
        "a 64 byte category was refused"
    );
    assert!(ContractCategory::parse(format!("x.{}", "y".repeat(63)).as_str()).is_err());
    assert!(FactId::parse("a".repeat(64).as_str()).is_ok());
    assert!(FactId::parse("a".repeat(65).as_str()).is_err());
    assert!(TargetName::parse("a".repeat(128).as_str()).is_ok());
    assert!(TargetName::parse("a".repeat(129).as_str()).is_err());
    let long = format!("host:{}", "a".repeat(300));
    let too_long = format!("host:{}", "a".repeat(301));
    assert!(TargetIdentity::parse(long.as_str()).is_ok());
    assert!(TargetIdentity::parse(too_long.as_str()).is_err());
}

#[test]
fn fact_id_grammar_accepts_and_rejects() {
    for accepted in ["host.not_in_maintenance", "single", "a_1.b_2"] {
        assert!(FactId::parse(accepted).is_ok(), "`{accepted}` was refused");
    }
    for rejected in ["", "Fact", "_fact", "fact two", "a".repeat(65).as_str()] {
        assert!(
            FactId::parse(rejected).is_err(),
            "`{rejected}` was accepted"
        );
    }
}
