//! Boot recovery: make the files under `packages_dir` agree with the registry.
//!
//! The registry is authoritative. Anything under `<name>/<version>` it does not list is the
//! leftover of an interrupted install or uninstall and is deleted; anything else (foreign
//! files, hidden entries, odd names) is left alone. A registry that cannot be read fences the
//! service and nothing is swept, because deleting on a guess could destroy installed packages.

use std::path::Path;

use domovoi_core::package::{KeyState, PackageName, PackageVersion};

use super::PackageService;
use super::events::{EventKind, PackageEvent};
use super::registry::Registry;

/// Runs recovery for `service`. `.staging` was already wiped by `Staging::open`.
pub(super) fn run(service: &PackageService) {
    let registry = match Registry::read(&service.registry_path()) {
        Ok(registry) => registry,
        Err(error) => {
            service.set_fence(error);
            return;
        }
    };
    sweep(service, &registry);
    report(service, &registry);
}

/// Real directory (not a symlink) at `path`.
fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn sweep(service: &PackageService, registry: &Registry) {
    let root = &service.dirs.packages_dir;
    let Ok(listing) = std::fs::read_dir(root) else {
        return;
    };
    for item in listing.flatten() {
        let file_name = item.file_name();
        let Some(text) = file_name.to_str() else {
            continue;
        };
        if text.starts_with('.') {
            continue;
        }
        let Ok(name) = PackageName::parse(text) else {
            tracing::warn!("leaving an unexpected entry in the packages directory");
            continue;
        };
        let package_dir = item.path();
        if !is_real_dir(&package_dir) {
            continue;
        }
        let keep = registry.get(&name).map(|entry| entry.version.clone());
        sweep_package(service, &package_dir, &name, keep.as_ref());
        if std::fs::read_dir(&package_dir).is_ok_and(|mut rest| rest.next().is_none()) {
            let _ = std::fs::remove_dir(&package_dir);
        }
    }
}

fn sweep_package(
    service: &PackageService,
    package_dir: &Path,
    name: &PackageName,
    keep: Option<&PackageVersion>,
) {
    let Ok(listing) = std::fs::read_dir(package_dir) else {
        return;
    };
    for item in listing.flatten() {
        let file_name = item.file_name();
        let Some(text) = file_name.to_str() else {
            continue;
        };
        let Ok(version) = PackageVersion::parse(text) else {
            continue;
        };
        if keep == Some(&version) || !is_real_dir(&item.path()) {
            continue;
        }
        if std::fs::remove_dir_all(item.path()).is_ok() {
            service.deps.events.emit(
                PackageEvent::new(EventKind::Recovered)
                    .package(name, &version)
                    .detail("removed files that no registry entry lists"),
            );
        }
    }
}

fn report(service: &PackageService, registry: &Registry) {
    for entry in registry.entries() {
        let present = service
            .dirs
            .packages_dir
            .join(entry.name.as_str())
            .join(entry.version.as_str())
            .is_dir();
        if !present {
            service.deps.events.emit(
                PackageEvent::new(EventKind::Recovered)
                    .package(&entry.name, &entry.version)
                    .detail("the files of a registered package are missing"),
            );
        }
        if service.keys.state(&entry.key_id) != KeyState::Trusted {
            service.deps.events.emit(
                PackageEvent::new(EventKind::KeyUntrusted)
                    .package(&entry.name, &entry.version)
                    .key(&entry.key_id)
                    .digest(&entry.archive_sha256),
            );
        }
    }
}
