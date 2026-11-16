//! Plan hash v2: inputs, canonical encoding, target identity and domain separation.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, CheckKey, ExecutionId, HostId, ImplHash, Incident, KeyFingerprint, ManifestHash,
    PlanBinding, PlanContract, PlanError, PlanHash, PlanTarget, Risk, ScriptDigest, Severity,
    Signal, SkillName, SkillSummary, SourceKind, TargetKey, ValidatedArgs, host_target_text,
};
use jiff::{SignedDuration, Timestamp};

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

#[cfg(test)]
fn host() -> HostId {
    HostId::from_uuid(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0001,
    ))
}

#[cfg(test)]
fn key(byte: u8) -> KeyFingerprint {
    KeyFingerprint::of_public_key(&[byte; 32])
}

#[cfg(test)]
fn incident(host: HostId, check: &str, target: Option<&str>) -> Incident {
    let signal = Signal::firing(
        SourceKind::BuiltIn,
        host,
        CheckKey::parse(check).unwrap(),
        target.map(|t| TargetKey::parse(t).unwrap()),
        Severity::Warning,
        "test",
        T0,
    );
    Incident::new_pending(&signal, T0)
}

#[cfg(test)]
fn target(host: HostId, byte: u8, check: &str, value: Option<&str>) -> PlanTarget {
    PlanTarget::of_incident(&incident(host, check, value), key(byte))
}

#[cfg(test)]
fn contract(id: &str, version: u32) -> PlanContract {
    PlanContract::new(id, version).unwrap()
}

#[cfg(test)]
fn impl_hash(manifest: u8, entry: u8) -> ImplHash {
    ImplHash::from_parts(
        format!("{manifest:02x}").repeat(32).parse().unwrap(),
        ScriptDigest::of_bytes(&[entry; 1]),
    )
}

#[cfg(test)]
fn args(value: &str) -> ValidatedArgs {
    let summary = summary("restart_container", Risk::Write);
    let mut map = BTreeMap::new();
    map.insert("container".to_owned(), value.to_owned());
    summary.validate_args(&map).unwrap()
}

#[cfg(test)]
fn summary(name: &str, risk: Risk) -> SkillSummary {
    use domovoi_core::{ParamKind, ParamName, ParamSpec};
    SkillSummary {
        name: SkillName::parse(name).unwrap(),
        description: "test".to_owned(),
        risk,
        params: vec![ParamSpec {
            name: ParamName::parse("container").unwrap(),
            description: "container".to_owned(),
            required: true,
            kind: ParamKind::String {
                pattern: "^[a-z0-9_.-]{1,64}$".to_owned(),
                max_len: 64,
            },
        }],
        applies_to: Vec::new(),
        manifest_hash: "ab".repeat(32).parse().unwrap(),
        entry_script_digest: ScriptDigest::of_bytes(b"entry"),
        timeout: SignedDuration::from_secs(60),
        verify: None,
        readme: None,
    }
}

#[cfg(test)]
fn execution() -> ExecutionId {
    ExecutionId::from_uuid(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0002,
    ))
}

#[cfg(test)]
fn hash(
    contract: &PlanContract,
    implementation: ImplHash,
    target: &PlanTarget,
    args: &ValidatedArgs,
    execution: ExecutionId,
) -> PlanHash {
    PlanHash::compute_v2(contract, implementation, target, args, execution)
}

#[test]
fn plan_hash_v2_covers_every_v2_input() {
    let base_contract = contract("restart_container", 1);
    let base_impl = impl_hash(1, 2);
    let base_target = target(host(), 7, "docker.container", Some("web"));
    let base_args = args("web");
    let base_execution = execution();
    let base = hash(
        &base_contract,
        base_impl,
        &base_target,
        &base_args,
        base_execution,
    );

    let changed = [
        hash(
            &contract("stop_container", 1),
            base_impl,
            &base_target,
            &base_args,
            base_execution,
        ),
        hash(
            &contract("restart_container", 2),
            base_impl,
            &base_target,
            &base_args,
            base_execution,
        ),
        hash(
            &base_contract,
            impl_hash(1, 3),
            &base_target,
            &base_args,
            base_execution,
        ),
        hash(
            &base_contract,
            base_impl,
            &target(host(), 8, "docker.container", Some("web")),
            &base_args,
            base_execution,
        ),
        hash(
            &base_contract,
            base_impl,
            &base_target,
            &args("sonarr"),
            base_execution,
        ),
        hash(
            &base_contract,
            base_impl,
            &base_target,
            &base_args,
            ExecutionId::from_uuid(uuid::Uuid::from_u128(3)),
        ),
    ];
    for (i, other) in changed.iter().enumerate() {
        assert_ne!(*other, base, "v2 ignores input {i}");
    }
}

#[test]
fn plan_hash_v2_is_domain_separated_from_v1() {
    let skill = summary("restart_container", Risk::Write);
    let plan_args = args("web");
    let v1 = PlanHash::compute(&skill.name, skill.manifest_hash, host(), &plan_args);
    let v2 = hash(
        &contract("restart_container", 1),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &target(host(), 7, "docker.container", Some("web")),
        &plan_args,
        execution(),
    );
    assert_ne!(v1, v2, "the domain line must keep the families apart");
}

#[test]
fn plan_hash_v2_encoding_is_unambiguous() {
    // Two pairs a naive newline join could confuse: the arguments already end with a newline,
    // and the execution id follows them behind a length prefix.
    let one = hash(
        &contract("restart_container", 1),
        impl_hash(1, 2),
        &target(host(), 7, "docker.container", None),
        &args("web"),
        ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
    );
    let two = hash(
        &contract("restart_container", 1),
        impl_hash(1, 2),
        &target(host(), 7, "docker.container", None),
        &args("web"),
        ExecutionId::from_uuid(uuid::Uuid::from_u128(2)),
    );
    assert_ne!(one, two);
    // Pins the exact bytes: changing the field order, a length prefix or a domain line moves
    // this digest. The value was produced by this implementation once, deliberately.
    assert_eq!(
        one.to_string(),
        "6d0e99661d9d646295a5e61963c5aba4f4b2dddbc5ef1b4d26dadedd5a45c22a",
        "the v2 encoding changed"
    );
}

#[test]
fn plan_hash_v2_rejects_a_non_canonical_contract_id() {
    for id in ["Restart", "", "restart@1", &"a".repeat(65)] {
        assert!(
            matches!(PlanContract::new(id, 1), Err(PlanError::ContractId { .. })),
            "`{id}` was accepted"
        );
    }
    assert!(PlanContract::new("restart_container", 1).is_ok());
}

#[test]
fn plan_hash_v2_rejects_a_non_canonical_target() {
    assert!(TargetKey::parse(" web").is_err());
    assert!(TargetKey::parse("web\n").is_err());
    assert!(TargetKey::parse("web\u{7f}").is_err());
    assert!(TargetKey::parse("web").is_ok());
}

#[test]
fn impl_hash_binds_the_entry_script_digest() {
    let manifest = "cd".repeat(32).parse::<ManifestHash>().unwrap();
    let a = ImplHash::from_parts(manifest, ScriptDigest::of_bytes(b"a"));
    let b = ImplHash::from_parts(manifest, ScriptDigest::of_bytes(b"b"));
    assert_ne!(a, b);
    assert_eq!(a.to_string().len(), 64);
    assert_eq!(manifest.to_string().len(), 64);
}

#[test]
fn approval_bound_to_displayed_plan_hash() {
    let shown = hash(
        &contract("restart_container", 1),
        impl_hash(1, 2),
        &target(host(), 7, "docker.container", Some("web")),
        &args("web"),
        execution(),
    );
    let other = hash(
        &contract("restart_container", 1),
        impl_hash(1, 2),
        &target(host(), 7, "docker.container", Some("web")),
        &args("sonarr"),
        execution(),
    );
    assert!(PlanBinding::Full(shown).matches(&shown));
    assert!(!PlanBinding::Full(shown).matches(&other));
    assert!(PlanBinding::Prefix(shown.as_bytes()[..3].to_vec()).matches(&shown));
    assert!(!PlanBinding::Prefix(shown.as_bytes()[..2].to_vec()).matches(&shown));
}

#[test]
fn execution_id_for_an_action_equals_the_action_id() {
    let id = ActionId::from_uuid(uuid::Uuid::from_u128(42));
    assert_eq!(ExecutionId::from_action(id).as_action(), id);
}

#[test]
fn plan_target_of_incident_takes_the_incidents_host() {
    let incident = incident(host(), "disk.usage", Some("/srv"));
    let canonical = PlanTarget::of_incident(&incident, key(7)).canonical();
    assert!(canonical.starts_with(&host_target_text(host(), &key(7))));
}

#[test]
fn plan_hash_changes_when_enrolled_agent_key_changes() {
    let first = target(host(), 7, "disk.usage", Some("/srv"));
    let second = target(host(), 8, "disk.usage", Some("/srv"));
    let a = hash(
        &contract("disk", 1),
        impl_hash(1, 2),
        &first,
        &args("web"),
        execution(),
    );
    let b = hash(
        &contract("disk", 1),
        impl_hash(1, 2),
        &second,
        &args("web"),
        execution(),
    );
    assert_ne!(a, b);
    assert_eq!(first.canonical().len(), second.canonical().len());
    assert_eq!(
        first.canonical().lines().next().map(str::len),
        second.canonical().lines().next().map(str::len),
        "both fingerprints are 64 hex; only the value changes",
    );
}

#[test]
fn target_text_embeds_the_fingerprint() {
    let target = target(host(), 7, "disk.usage", Some("/srv"));
    let text = target.canonical();
    assert_eq!(text.matches(&key(7).to_string()).count(), 1);
    assert!(text.starts_with(&host_target_text(host(), &key(7))));
    assert!(!text.contains("key:absent"));
}

#[test]
fn key_fingerprint_is_sha256_of_the_public_key() {
    use sha2::Digest as _;
    let public = [9u8; 32];
    let expected = hex::encode(sha2::Sha256::digest(public));
    assert_eq!(KeyFingerprint::of_public_key(&public).to_string(), expected);
    assert_eq!(expected.len(), 64);
    assert!("0X".repeat(32).parse::<KeyFingerprint>().is_err());
}

#[test]
fn host_target_text_is_the_only_formatter() {
    let text = host_target_text(host(), &key(7));
    assert!(text.starts_with(&format!("host:{}", host())));
    assert!(text.ends_with(&key(7).to_string()));
    let canonical = target(host(), 7, "disk.usage", None).canonical();
    assert!(canonical.starts_with(&format!("{text}\n")));
    assert!(canonical.contains("target:absent"));
    assert_ne!(
        canonical,
        target(host(), 8, "disk.usage", None).canonical(),
        "changing the key changes the text"
    );
}

#[test]
fn execution_identity_is_the_final_action_id() {
    let skill = summary("restart_container", Risk::Write);
    let plan_args = args("web");
    let plan_target = target(host(), 7, "docker.container", Some("web"));
    let request = domovoi_core::ActionRequest::propose(
        domovoi_core::IncidentId::new(),
        &skill,
        &plan_args,
        Some(plan_target.clone()),
        domovoi_core::Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    )
    .unwrap();
    let expected = PlanHash::compute_v2(
        &contract("restart_container", 1),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &plan_target,
        &plan_args,
        ExecutionId::from_action(request.id()),
    );
    assert_eq!(
        request.plan_hash_v2(),
        Some(expected),
        "the v2 hash was calculated with the final action id",
    );
}

#[test]
fn propose_refuses_a_host_without_an_enrolled_key() {
    let skill = summary("restart_container", Risk::Write);
    let result = domovoi_core::ActionRequest::propose(
        domovoi_core::IncidentId::new(),
        &skill,
        &args("web"),
        None,
        domovoi_core::Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        T0,
    );
    assert_eq!(result, Err(PlanError::NoEnrolledKey));
}
