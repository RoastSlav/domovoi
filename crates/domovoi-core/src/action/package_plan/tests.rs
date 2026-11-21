//! Tests of the package plan hashes.

use super::*;

fn name(text: &str) -> PackageName {
    PackageName::parse(text).unwrap()
}

fn version(text: &str) -> PackageVersion {
    PackageVersion::parse(text).unwrap()
}

fn digest(byte: u8) -> ArchiveDigest {
    ArchiveDigest::from_bytes([byte; 32])
}

fn key_id(text: &str) -> KeyId {
    KeyId::parse(text).unwrap()
}

fn fingerprint(byte: u8) -> KeyFingerprint {
    KeyFingerprint::of_public_key(&[byte; 32])
}

fn host(n: u128) -> HostId {
    HostId::from_uuid(uuid::Uuid::from_u128(n))
}

fn base() -> PackagePlan {
    PackagePlan::install(
        &name("mail-tools"),
        &version("1.2.3"),
        &digest(1),
        &key_id("0123456789abcdef"),
        host(1),
        &fingerprint(1),
    )
}

#[test]
fn package_plan_hash_changes_with_each_input() {
    let reference = base().hash();
    let variants = [
        PackagePlan::install(
            &name("mail-other"),
            &version("1.2.3"),
            &digest(1),
            &key_id("0123456789abcdef"),
            host(1),
            &fingerprint(1),
        ),
        PackagePlan::install(
            &name("mail-tools"),
            &version("1.2.4"),
            &digest(1),
            &key_id("0123456789abcdef"),
            host(1),
            &fingerprint(1),
        ),
        PackagePlan::install(
            &name("mail-tools"),
            &version("1.2.3"),
            &digest(2),
            &key_id("0123456789abcdef"),
            host(1),
            &fingerprint(1),
        ),
        PackagePlan::install(
            &name("mail-tools"),
            &version("1.2.3"),
            &digest(1),
            &key_id("fedcba9876543210"),
            host(1),
            &fingerprint(1),
        ),
        PackagePlan::install(
            &name("mail-tools"),
            &version("1.2.3"),
            &digest(1),
            &key_id("0123456789abcdef"),
            host(2),
            &fingerprint(1),
        ),
        PackagePlan::install(
            &name("mail-tools"),
            &version("1.2.3"),
            &digest(1),
            &key_id("0123456789abcdef"),
            host(1),
            &fingerprint(2),
        ),
        PackagePlan::uninstall(
            &name("mail-tools"),
            &version("1.2.3"),
            host(1),
            &fingerprint(1),
        ),
        PackagePlan::list(host(1), &fingerprint(1)),
    ];
    let mut seen = vec![reference];
    for variant in variants {
        let hash = variant.hash();
        assert!(!seen.contains(&hash), "{variant:?} collided");
        seen.push(hash);
    }
}

#[test]
fn package_plan_hash_ignores_execution_id() {
    // The plan has no execution input: building it again, as a retry does, gives the
    // same hash, and nothing in the encoding names an execution.
    assert_eq!(base().hash(), base().hash());
    assert!(!base().canonical().contains("execution"));
}

#[test]
fn package_plan_domain_is_not_plan_v2() {
    let canonical = base().canonical();
    assert!(canonical.starts_with("plan:package-v1\ncontract:"));
    assert!(!canonical.starts_with("plan:v2"));
    let body = canonical.strip_prefix("plan:package-v1").unwrap();
    let as_v2 = PlanHash::digest(format!("plan:v2{body}").as_bytes());
    assert_ne!(base().hash(), as_v2);
    let as_v1 = PlanHash::digest(format!("plan:v1{body}").as_bytes());
    assert_ne!(base().hash(), as_v1);
}

#[test]
fn package_plan_encoding_is_length_prefixed() {
    let text = PackagePlan::list(host(1), &fingerprint(1)).canonical();
    let target = host_target_text(host(1), &fingerprint(1));
    assert_eq!(
        text,
        format!(
            "plan:package-v1\ncontract:20:agent.package.list@1\nsubject:4:none\ntarget:{}:{target}\n",
            target.len()
        )
    );
    let install = base().canonical();
    assert!(install.contains("agent.package.install@1"));
    assert!(install.contains("name=mail-tools\nversion=1.2.3\nsha256="));
    assert!(install.contains("\nkey=0123456789abcdef"));
}

#[test]
fn package_contract_ids_are_valid_plan_contracts() {
    use crate::action::PlanContract;
    for id in [
        "agent.package.install",
        "agent.package.uninstall",
        "agent.package.list",
    ] {
        assert!(PlanContract::new(id, 1).is_ok(), "{id}");
    }
}

#[test]
fn job_plan_hash_is_order_independent_and_kind_bound() {
    let a = PackagePlan::list(host(1), &fingerprint(1)).hash();
    let b = PackagePlan::list(host(2), &fingerprint(2)).hash();
    let c = PackagePlan::list(host(3), &fingerprint(3)).hash();
    let install = job_plan_hash(InstallKind::Install, &[a, b, c]);
    assert_eq!(install, job_plan_hash(InstallKind::Install, &[c, a, b]));
    assert_ne!(install, job_plan_hash(InstallKind::Uninstall, &[a, b, c]));
    assert_ne!(install, job_plan_hash(InstallKind::Install, &[a, b]));
    assert_ne!(install, job_plan_hash(InstallKind::Install, &[a, b, a]));
}
