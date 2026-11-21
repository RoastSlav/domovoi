//! What the hub believes a host has installed: uninstalling and refreshing.

use std::collections::BTreeMap;

use domovoi_core::package::{
    KeyId, KeyState, PackageListEntry, PackageName, PackageStatus, PackageVersion, keys,
};
use domovoi_core::ports::{InstallRepo, InstallWrite, StateWrite, Transaction, UnitOfWork};
use domovoi_core::{
    HostPackage, InstallKind, ObservedBy, PackageCall, PackageOutputs, TargetStatus,
};
use domovoi_testkit::build::T0;
use domovoi_testkit::link::PackageEvent;

use super::world::{KEY, World};

fn entry(name: &str, status: PackageStatus, state: KeyState) -> PackageListEntry {
    PackageListEntry {
        name: PackageName::parse(name).unwrap(),
        version: PackageVersion::parse("1.0.0").unwrap(),
        key_id: KeyId::parse(KEY).unwrap(),
        archive_sha256: domovoi_core::package::ArchiveDigest::from_bytes([3; 32]),
        installed_at: T0,
        skills: vec!["status".to_owned()],
        status,
        key_state: state,
        executable: status == PackageStatus::Active,
        executable_reason: None,
    }
}

fn listing(entries: &[PackageListEntry]) -> PackageOutputs {
    PackageOutputs(BTreeMap::from([
        (keys::OUT_COUNT.to_owned(), entries.len().to_string()),
        (keys::OUT_INSTALL_ALLOWED.to_owned(), "true".to_owned()),
        (
            keys::OUT_PRIVILEGE_BOUNDARY.to_owned(),
            "in_effect".to_owned(),
        ),
        (
            keys::OUT_PACKAGES.to_owned(),
            serde_json::to_string(entries).unwrap(),
        ),
    ]))
}

#[tokio::test]
async fn uninstall_sends_no_transfers_and_deletes_inventory() {
    let world = World::new().await;
    let artifact = world.import("web-tools", "1.0.0", b"small archive").await;
    world.agent_installs(&artifact);
    let installed = world.install("web-tools", "1.0.0").await;
    world.settled(&installed).await;
    let rows = InstallRepo::inventory(&*world.store, world.host)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].observed_by, ObservedBy::Job);
    assert_eq!(rows[0].skills, ["restart", "status"]);
    assert_eq!(rows[0].key_state, KeyState::Trusted);
    let before = world.link.package_events().len();

    world.link.on_capability(|_, request| {
        let PackageCall::Uninstall { package, version } = &request.call else {
            panic!("not an uninstall: {:?}", request.call);
        };
        Ok(PackageOutputs(BTreeMap::from([
            (keys::OUT_PACKAGE.to_owned(), package.to_string()),
            (keys::OUT_VERSION.to_owned(), version.to_string()),
            (keys::OUT_REMOVED.to_owned(), "true".to_owned()),
        ])))
    });
    let request = world.request(InstallKind::Uninstall, "web-tools", "1.0.0", &[world.host]);
    let plan = world.handle.plan(&request).await.unwrap();
    assert_eq!(
        plan.hosts[0]
            .installed
            .as_ref()
            .map(|row| row.name.to_string()),
        Some("web-tools".to_owned())
    );
    let (_, targets) = world.create(request).await.unwrap();
    let done = world.settled(&targets[0]).await;
    assert_eq!(done.status, TargetStatus::Succeeded);

    let events = world.link.package_events();
    let new = &events[before..];
    assert_eq!(new.len(), 2, "{new:#?}");
    assert!(matches!(&new[0], PackageEvent::Capability { .. }));
    assert!(matches!(&new[1], PackageEvent::CapabilityResult { .. }));
    assert!(done.artifact_transfer.is_none() && done.signature_transfer.is_none());
    assert!(
        InstallRepo::inventory(&*world.store, world.host)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(world.audit("package.uninstalled").await.len(), 1);
}

#[tokio::test]
async fn refresh_replaces_inventory_and_audits_untrusted_keys() {
    let world = World::new().await;
    // A row the agent no longer lists.
    let stale = HostPackage::from_entry(
        world.host,
        &entry("old-tools", PackageStatus::Active, KeyState::Trusted),
        domovoi_core::package::PrivilegeBoundary::InEffect,
        T0,
        ObservedBy::Job,
    );
    let seed = Transaction::new().write(StateWrite::Install(InstallWrite::Inventory {
        host: world.host,
        entries: vec![stale],
        replace: false,
    }));
    UnitOfWork::commit(&*world.store, seed).await.unwrap();

    let listed = [
        entry("web-tools", PackageStatus::Active, KeyState::Trusted),
        entry(
            "net-tools",
            PackageStatus::DisabledUntrustedKey,
            KeyState::Revoked,
        ),
    ];
    let answer = listing(&listed);
    world.link.on_capability(move |_, _| Ok(answer.clone()));

    let rows = world
        .handle
        .refresh_inventory(world.host, world.user)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let stored = InstallRepo::inventory(&*world.store, world.host)
        .await
        .unwrap();
    let names: Vec<String> = stored.iter().map(|row| row.name.to_string()).collect();
    assert_eq!(names, ["net-tools", "web-tools"], "the old row is gone");
    assert!(
        stored
            .iter()
            .all(|row| row.observed_by == ObservedBy::Refresh)
    );
    assert_eq!(world.audit("package.inventory_refreshed").await.len(), 1);
    let untrusted = world.audit("package.key_untrusted_detected").await;
    assert_eq!(untrusted.len(), 1);
    assert_eq!(untrusted[0].subject, "package:net-tools@1.0.0");

    // The same answer again reports nothing new.
    world
        .handle
        .refresh_inventory(world.host, world.user)
        .await
        .unwrap();
    assert_eq!(world.audit("package.inventory_refreshed").await.len(), 2);
    assert_eq!(world.audit("package.key_untrusted_detected").await.len(), 1);
}
