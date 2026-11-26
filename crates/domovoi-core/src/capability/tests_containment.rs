//! Tests of the containment contracts, the network target and the plan hash.

use jiff::SignedDuration;

use super::containment::{BLOCK_SOURCE_ID, containment_contracts};
use super::contract::ContractError;
use super::effect::{EffectClass, ExecutionPath, TargetKind};
use super::index::ContractIndex;
use super::package::known_contracts;
use super::target::{TargetQuery, canonical_network_identity};
use crate::action::{ContainmentPlan, PlanAssurance};
use crate::attest::{BootId, Hash32, SafetyIdentity, UnverifiedReason};
use crate::containment::{Net, keys};
use crate::host::{KeyFingerprint, host_target_text};
use crate::id::HostId;

fn host(n: u128) -> HostId {
    HostId::from_uuid(uuid::Uuid::from_u128(n))
}

fn fingerprint(byte: u8) -> KeyFingerprint {
    KeyFingerprint::of_public_key(&[byte; 32])
}

fn net(text: &str) -> Net {
    Net::parse(text).unwrap()
}

fn identity(generation: u64) -> SafetyIdentity {
    SafetyIdentity {
        fingerprint: fingerprint(9),
        boot_id: BootId::parse("018f4e46-4a2e-7a37-9d0a-4f2b6c8d0e1f").unwrap(),
        generation,
        config_hash: Hash32::from_bytes([1; 32]),
        route_state_hash: Hash32::from_bytes([2; 32]),
    }
}

fn verified() -> PlanAssurance {
    PlanAssurance::Verified(identity(3))
}

fn plan(n: &Net, ttl: i64, h: HostId, key: &KeyFingerprint, a: &PlanAssurance) -> ContainmentPlan {
    ContainmentPlan::new(n, SignedDuration::from_secs(ttl), h, key, a)
}

fn base() -> ContainmentPlan {
    plan(
        &net("203.0.113.0/24"),
        600,
        host(1),
        &fingerprint(1),
        &verified(),
    )
}

#[test]
fn contracts_validate_and_join_known_contracts() {
    let contracts = containment_contracts();
    assert_eq!(contracts.len(), 2);
    for contract in &contracts {
        contract.validate().unwrap();
    }
    ContractIndex::new(known_contracts()).unwrap();
    let ids: Vec<String> = known_contracts().iter().map(|c| c.id.to_string()).collect();
    assert!(ids.contains(&"firewall.block_source".to_owned()));
    assert!(ids.contains(&"firewall.list_blocks".to_owned()));
    let block = &contracts[0];
    assert_eq!(block.effect, EffectClass::Contain);
    assert_eq!(block.execution, ExecutionPath::Helper);
    assert_eq!(block.target_kind, TargetKind::NetworkSource);
    assert_eq!(contracts[1].effect, EffectClass::Read);
}

#[test]
fn network_source_target_requires_a_target_arg() {
    let mut contract = containment_contracts().remove(0);
    contract.target_arg = None;
    assert!(matches!(
        contract.validate(),
        Err(ContractError::TargetArgMismatch { .. })
    ));
}

#[test]
fn contract_args_have_exactly_the_six_keys() {
    let contract = containment_contracts().remove(0);
    let props = contract.input_schema["properties"].as_object().unwrap();
    let mut got: Vec<&str> = props.keys().map(String::as_str).collect();
    got.sort_unstable();
    let mut want = keys::ARG_KEYS.to_vec();
    want.sort_unstable();
    assert_eq!(got, want);
    assert_eq!(contract.input_schema["additionalProperties"], false);
    assert_eq!(
        contract.input_schema["required"].as_array().unwrap().len(),
        6
    );
    assert_eq!(contract.target_arg.as_deref(), Some("source"));
}

#[test]
fn constrainable_args_name_both_prefix_floors_and_exist_in_the_schema() {
    let contract = containment_contracts().remove(0);
    let found: Vec<&str> = contract
        .constrainable_args
        .iter()
        .map(|a| a.key.as_str())
        .collect();
    assert_eq!(found, ["prefix_len_v4", "prefix_len_v6"]);
    for arg in &contract.constrainable_args {
        assert!(contract.input_schema["properties"].get(&arg.key).is_some());
    }
    assert_eq!(contract.id.as_str(), BLOCK_SOURCE_ID);
}

#[test]
fn ipnet_identity_binds_host_and_key() {
    let n = net("203.0.113.0/24");
    let a = canonical_network_identity(host(1), &fingerprint(1), &n).unwrap();
    assert_eq!(
        a.as_str(),
        format!(
            "ipnet:203.0.113.0/24@{}",
            host_target_text(host(1), &fingerprint(1))
        )
    );
    let other_key = canonical_network_identity(host(1), &fingerprint(2), &n).unwrap();
    let other_host = canonical_network_identity(host(2), &fingerprint(1), &n).unwrap();
    assert_ne!(a, other_key);
    assert_ne!(a, other_host);
    let v6 = canonical_network_identity(host(1), &fingerprint(1), &net("2001:db8:1::/64")).unwrap();
    assert!(v6.as_str().starts_with("ipnet:2001:db8:1::/64@host:"));
}

#[test]
fn network_source_query_names_host_and_kind() {
    let query = TargetQuery::NetworkSource {
        host: host(7),
        source: net("203.0.113.7"),
    };
    assert_eq!(query.host(), host(7));
    assert_eq!(query.kind(), TargetKind::NetworkSource);
    let json = serde_json::to_string(&query).unwrap();
    assert_eq!(serde_json::from_str::<TargetQuery>(&json).unwrap(), query);
}

#[test]
fn containment_plan_hash_changes_with_each_input() {
    let b = base().hash();
    let n = net("203.0.113.0/24");
    let key = fingerprint(1);
    assert_ne!(
        b,
        plan(&net("203.0.113.0/25"), 600, host(1), &key, &verified()).hash()
    );
    assert_ne!(b, plan(&n, 601, host(1), &key, &verified()).hash());
    assert_ne!(b, plan(&n, 600, host(2), &key, &verified()).hash());
    assert_ne!(
        b,
        plan(&n, 600, host(1), &fingerprint(2), &verified()).hash()
    );
    let next = PlanAssurance::Verified(identity(4));
    assert_ne!(b, plan(&n, 600, host(1), &key, &next).hash());
    let unverified = PlanAssurance::Unverified(UnverifiedReason::KeyNotPinned);
    let other = PlanAssurance::Unverified(UnverifiedReason::AttestationStale);
    let u = plan(&n, 600, host(1), &key, &unverified).hash();
    assert_ne!(b, u);
    assert_ne!(u, plan(&n, 600, host(1), &key, &other).hash());
    assert_eq!(b, base().hash());
}

#[test]
fn containment_plan_hash_ignores_phase_and_execution_id() {
    // The constructor has no phase or execution input; the canonical text names neither.
    let text = base().canonical();
    assert!(!text.contains("phase"));
    assert!(!text.contains("execution"));
    assert_eq!(base().hash(), base().hash());
}

#[test]
fn containment_plan_domain_is_not_plan_v2() {
    let text = base().canonical();
    assert!(text.starts_with("plan:containment-v1\n"));
    assert!(!text.contains("plan:v2"));
    assert!(text.contains("contract:23:firewall.block_source@1\n"));
    assert!(text.contains("source:14:203.0.113.0/24\n"));
    assert!(text.contains("ttl:600\n"));
    assert!(text.contains("assurance:"));
}
