//! Plan hashes: the agent check hash binds no key fingerprint, the plan hash does.

use std::collections::BTreeMap;

use domovoi_core::{ExecutionId, ImplHash, KeyFingerprint, PlanContract, PlanHash, PlanTarget};
use domovoi_testkit::build::open_incident;

use crate::harness::{host_id, restart_skill};

#[test]
fn the_agent_check_hash_has_no_key_fingerprint_and_the_plan_hash_binds_it() {
    let skill = restart_skill();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let first = PlanHash::agent_check(&skill.name, skill.manifest_hash, host_id(), &args);
    let second = PlanHash::agent_check(&skill.name, skill.manifest_hash, host_id(), &args);
    assert_eq!(first, second, "the agent check hash has no key fingerprint");

    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    let plan = |key: u8| {
        PlanHash::compute(
            &PlanContract::skill(&skill.name, 1).unwrap(),
            ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
            &PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&[key; 32])),
            &args,
            ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
        )
    };
    assert_ne!(plan(7), plan(8), "the plan hash binds the key fingerprint");
}
