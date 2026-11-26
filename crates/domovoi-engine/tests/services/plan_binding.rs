//! Plan hashes: the first generation binds no key fingerprint, the second does.

use std::collections::BTreeMap;

use domovoi_core::{ExecutionId, ImplHash, KeyFingerprint, PlanContract, PlanHash, PlanTarget};
use domovoi_testkit::build::open_incident;

use crate::harness::{host_id, restart_skill};

#[test]
fn plan_hash_v1_has_no_key_fingerprint_and_v2_binds_it() {
    let skill = restart_skill();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let first = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    let second = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    assert_eq!(first, second, "v1 has no key fingerprint");

    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    let v2 = |key: u8| {
        PlanHash::compute_v2(
            &PlanContract::skill(&skill.name, 1).unwrap(),
            ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
            &PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&[key; 32])),
            &args,
            ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
        )
    };
    assert_ne!(v2(7), v2(8), "v2 binds the key fingerprint");
}
