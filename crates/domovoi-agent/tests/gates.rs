//! Invariant gates for the agent (INV-3, INV-4): the hub can only run skills that are in
//! this host's catalogue, and only exactly as approved.

#[cfg(test)]
#[path = "common/skills.rs"]
mod common;
#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "common/packages.rs"]
mod package_rig;

use std::collections::BTreeMap;

use common::{Fixture, install};
use domovoi_agent::requests::run_skill;
use domovoi_core::{ActionId, PlanHash, Risk, SkillName};
use domovoi_proto::messages::{RunSkill, SkillFailureCode, SkillOutcome};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;

#[cfg(test)]
fn failure(outcome: SkillOutcome) -> SkillFailureCode {
    match outcome {
        SkillOutcome::Err(fail) => fail.code,
        SkillOutcome::Ok(_) => panic!("the skill ran"),
    }
}

#[cfg(test)]
fn args() -> BTreeMap<String, String> {
    BTreeMap::from([("service".to_owned(), "plex".to_owned())])
}

/// INV-3: a skill that is not in the local catalogue never reaches the runner, whatever
/// the hub sends, including a plan hash it computed for a manifest of its own.
#[tokio::test]
async fn hub_cannot_run_skill_not_in_local_catalog() {
    let fixture = Fixture::new();
    install(&fixture.skills(), "restart_app", "write");
    let ctx = fixture.ctx(
        vec![Risk::Read, Risk::Write, Risk::Destructive],
        None,
        Vec::new(),
    );
    let pushed = domovoi_testkit::build::manifest("hub_pushed", Risk::Write).unwrap();
    let validated = pushed.validate_args(&BTreeMap::new()).unwrap();
    for skill in ["hub_pushed", "../restart_app", "restart_app/../x"] {
        let Ok(skill) = SkillName::parse(skill) else {
            continue;
        };
        let request = RunSkill {
            action_id: ActionId::new(),
            plan_hash: PlanHash::compute(
                &skill,
                pushed.manifest_hash(),
                fixture.host_id,
                &validated,
            ),
            skill,
            args: args(),
            deadline: T0 + SignedDuration::from_mins(1),
        };
        assert_eq!(
            failure(run_skill(&ctx, &request).await),
            SkillFailureCode::NotFound
        );
    }
    assert!(fixture.exec.calls().is_empty(), "the runner was called");
}

/// INV-4: a plan hash that does not match this host's manifest, host and arguments never
/// executes.
#[cfg(unix)]
#[tokio::test]
async fn plan_hash_mismatch_never_executes() {
    let fixture = Fixture::new();
    install(&fixture.skills(), "restart_app", "write");
    let ctx = fixture.ctx(vec![Risk::Read, Risk::Write], None, Vec::new());
    let skill = SkillName::parse("restart_app").unwrap();
    let loaded = ctx.catalog.get(&skill).unwrap();
    let validated = loaded.manifest().validate_args(&args()).unwrap();
    let other_host = domovoi_core::HostId::new();
    let other_args = loaded
        .manifest()
        .validate_args(&BTreeMap::from([(
            "service".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let mut changed = loaded.manifest().clone();
    changed.description = "edited after approval".to_owned();
    let wrong = [
        PlanHash::compute(&skill, loaded.manifest_hash(), other_host, &validated),
        PlanHash::compute(&skill, loaded.manifest_hash(), fixture.host_id, &other_args),
        PlanHash::compute(&skill, changed.manifest_hash(), fixture.host_id, &validated),
    ];
    for plan_hash in wrong {
        let request = RunSkill {
            action_id: ActionId::new(),
            skill: skill.clone(),
            args: args(),
            plan_hash,
            deadline: T0 + SignedDuration::from_mins(1),
        };
        assert_eq!(
            failure(run_skill(&ctx, &request).await),
            SkillFailureCode::PlanMismatch
        );
    }
    assert!(fixture.exec.calls().is_empty(), "the runner was called");
}

/// RW-26: the hub cannot add a trusted key. A package signed by a key the owner never
/// listed is refused, a request that carries a key is malformed, and the key set has no
/// way to change after construction.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn hub_cannot_add_trusted_key() {
    use domovoi_proto::Message;
    use domovoi_proto::messages::{CapabilityOutcome, TransferKind};
    use domovoi_testkit::PackageFixture;
    use package_rig::{Options, Rig};

    let trusted = PackageFixture::new("nginx-tools", "1.2.0").unwrap().build();
    let stranger = PackageFixture::new("nginx-tools", "1.2.0")
        .unwrap()
        .skill("nginx_reload")
        .build();
    let rig = Rig::new(&[&trusted], Options::default());
    let mut args = rig.install_args(&stranger, "nginx-tools", "1.2.0");
    let refused = match rig.call(rig.run("agent.package.install", &args)).await {
        Message::CapabilityResult(result) => result,
        other => panic!("expected a result, got {other:?}"),
    };
    let CapabilityOutcome::Failed { message, .. } = refused.outcome else {
        panic!("an unknown signer must not install");
    };
    assert!(message.starts_with("untrusted_key"), "{message}");
    let list = rig.service.as_ref().unwrap().list().unwrap();
    assert_eq!(list.outputs()["count"], "0");
    assert!(!rig.dirs.packages_dir.join("nginx-tools").exists());

    args.push(("public_key", stranger.public_key_b64.clone()));
    args.push(("trusted_key", stranger.key_id.to_string()));
    let message = rig.call(rig.run("agent.package.install", &args)).await;
    assert!(matches!(&message, Message::Error(e) if e.code == "bad_request"));
    let _ = TransferKind::PackageArtifact;

    let source = include_str!("../src/packages/keys.rs");
    assert!(
        !source.contains("&mut self"),
        "the key set must have no method that changes it"
    );
}

/// Hostile archive entries cannot reach a sibling directory (the helper's), and a packages
/// directory inside a protected root is refused outright.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn package_install_cannot_write_helper_paths() {
    use domovoi_agent::packages::{PackageDeps, PackageDirs, PackageService};
    use domovoi_proto::messages::TransferKind;
    use domovoi_testkit::PackageFixture;
    use package_rig::section;

    let root = tempfile::tempdir().unwrap();
    let helper = root.path().join("helper");
    std::fs::create_dir_all(&helper).unwrap();
    std::fs::write(helper.join("keep"), "original").unwrap();
    let before = snapshot(&helper);

    let hostile = PackageFixture::new("nginx-tools", "1.2.0")
        .unwrap()
        .skill("nginx_reload")
        .raw_entry(
            b"nginx-tools/../../helper/evil",
            tar::EntryType::Regular,
            b"x",
        )
        .build();
    let clock = std::sync::Arc::new(domovoi_testkit::FakeClock::new(T0));
    let state = root.path().join("state");
    let skills = root.path().join("skills");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&skills).unwrap();
    let mut dirs = PackageDirs::new(root.path().join("packages"), state, skills);
    dirs.protected = vec![helper.clone()];
    let service = PackageService::open(
        &section(&[&hostile], true),
        dirs.clone(),
        PackageDeps::production(clock.clone(), false),
    )
    .unwrap();
    let staging = service.staging().unwrap();
    let stage = |kind: TransferKind, bytes: &[u8]| {
        let id = uuid::Uuid::now_v7();
        std::fs::write(staging.bin_path(id), bytes).unwrap();
        staging.register_staged(id, kind, bytes.len() as u64);
        id
    };
    let request = domovoi_agent::packages::InstallRequest {
        package: domovoi_core::package::PackageName::parse("nginx-tools").unwrap(),
        version: domovoi_core::package::PackageVersion::parse("1.2.0").unwrap(),
        artifact_transfer: stage(TransferKind::PackageArtifact, &hostile.archive),
        signature_transfer: stage(TransferKind::PackageManifest, hostile.sig_toml.as_bytes()),
    };
    assert!(service.install(uuid::Uuid::now_v7(), request).is_err());
    assert_eq!(snapshot(&helper), before, "the helper directory changed");
    assert!(!root.path().join("evil").exists());

    // A packages directory inside a protected root fences the service.
    let mut inside = dirs;
    inside.packages_dir = helper.join("packages");
    let fenced = PackageService::open(
        &section(&[&hostile], true),
        inside,
        PackageDeps::production(clock, false),
    )
    .unwrap();
    assert!(fenced.staging().is_none());
    assert!(fenced.list().is_err());
    assert_eq!(snapshot(&helper), before, "the helper directory changed");
}

/// The files and contents under `dir`, sorted, to compare before and after.
#[cfg(test)]
#[cfg(target_os = "linux")]
fn snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let name = path.strip_prefix(dir).unwrap().display().to_string();
                found.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    found.sort();
    found
}

/// Installing a package does not make its skills runnable: they are not in the catalogue
/// and `run_skill` does not find them.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn package_skills_are_not_runnable_before_the_skill_runner() {
    use domovoi_proto::messages::TransferKind;
    use domovoi_testkit::PackageFixture;
    use package_rig::{Options, Rig};

    let built = PackageFixture::new("nginx-tools", "1.2.0")
        .unwrap()
        .skill("nginx_reload")
        .build();
    let rig = Rig::new(&[&built], Options::default());
    let args = rig.install_args(&built, "nginx-tools", "1.2.0");
    rig.call(rig.run("agent.package.install", &args)).await;
    let _ = TransferKind::PackageManifest;
    assert!(rig.fixture.skills().read_dir().unwrap().next().is_none());
    let listed = rig.service.as_ref().unwrap().list().unwrap().outputs();
    assert!(listed["packages"].contains("nginx_reload"));
    assert!(listed["packages"].contains("skill_runner_not_integrated"));

    let skill = SkillName::parse("nginx_reload").unwrap();
    assert!(rig.ctx.catalog.get(&skill).is_none());
    assert!(rig.ctx.catalog.summaries().is_empty());
    let request = RunSkill {
        action_id: ActionId::new(),
        skill,
        args: BTreeMap::new(),
        plan_hash: "01".repeat(32).parse::<PlanHash>().unwrap(),
        deadline: T0 + SignedDuration::from_mins(1),
    };
    assert_eq!(
        failure(run_skill(&rig.ctx, &request).await),
        SkillFailureCode::NotFound
    );
}
