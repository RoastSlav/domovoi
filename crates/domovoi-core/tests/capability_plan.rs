//! The `plan:capability-v1` family: what a unit restart's approval or permit binds.

use std::collections::BTreeMap;

use domovoi_core::capability::TargetIdentity;
use domovoi_core::unit::{IdentityHash, UnitId, UnitName, canonical_unit_identity};
use domovoi_core::{CapabilityPlan, ExecutionId, HostId, KeyFingerprint, StepId};

#[cfg(test)]
fn host() -> HostId {
    HostId::from_uuid(uuid::Uuid::from_u128(
        0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
    ))
}

#[cfg(test)]
fn key(byte: u8) -> KeyFingerprint {
    KeyFingerprint::of_public_key(&[byte; 32])
}

#[cfg(test)]
fn execution(n: u128) -> ExecutionId {
    ExecutionId::from_step(StepId::from_uuid(uuid::Uuid::from_u128(n)))
}

#[cfg(test)]
fn hash(byte: u8) -> IdentityHash {
    hex::encode([byte; 32]).parse().unwrap()
}

#[cfg(test)]
fn target(id: &str) -> TargetIdentity {
    canonical_unit_identity(host(), &key(1), &UnitId::parse(id).unwrap()).unwrap()
}

#[cfg(test)]
struct Inputs {
    host: HostId,
    key: KeyFingerprint,
    target: TargetIdentity,
    identity: IdentityHash,
    args: BTreeMap<String, String>,
    execution: ExecutionId,
}

#[cfg(test)]
fn base() -> Inputs {
    Inputs {
        host: host(),
        key: key(1),
        target: target("example.service"),
        identity: hash(0xaa),
        args: BTreeMap::from([("unit".to_owned(), "example.service".to_owned())]),
        execution: execution(7),
    }
}

#[cfg(test)]
fn plan(i: &Inputs) -> CapabilityPlan {
    CapabilityPlan::unit_restart(i.host, &i.key, &i.target, &i.identity, &i.args, i.execution)
}

#[test]
#[cfg(test)]
fn capability_plan_hash_changes_with_each_input() {
    let reference = plan(&base()).hash();
    let mut seen = vec![reference];

    let mut changed = base();
    changed.host = HostId::from_uuid(uuid::Uuid::from_u128(9));
    seen.push(plan(&changed).hash());
    let mut changed = base();
    changed.key = key(2);
    seen.push(plan(&changed).hash());
    let mut changed = base();
    changed.target = target("other.service");
    seen.push(plan(&changed).hash());
    let mut changed = base();
    changed.identity = hash(0xbb);
    seen.push(plan(&changed).hash());
    let mut changed = base();
    changed
        .args
        .insert("unit".to_owned(), "other.service".to_owned());
    seen.push(plan(&changed).hash());
    let mut changed = base();
    changed.execution = execution(8);
    seen.push(plan(&changed).hash());
    seen.push(
        CapabilityPlan::unit_identity_read(
            host(),
            &key(1),
            &UnitName::parse("example.service").unwrap(),
            execution(7),
        )
        .hash(),
    );

    let unique: std::collections::BTreeSet<_> = seen.iter().map(ToString::to_string).collect();
    assert_eq!(unique.len(), seen.len(), "every input flips the hash");
}

#[test]
#[cfg(test)]
fn capability_plan_domain_is_not_plan_v1_or_v2() {
    let text = plan(&base()).canonical();
    assert!(text.starts_with("plan:capability-v1\n"));
    assert!(!text.starts_with("plan:v1") && !text.starts_with("plan:v2"));
}

#[test]
#[cfg(test)]
fn capability_plan_golden_vectors() {
    let text = plan(&base()).canonical();
    insta::assert_snapshot!(text.replace(&key(1).to_string(), "<key>"));
}
