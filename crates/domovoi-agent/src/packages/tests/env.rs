//! A throwaway host for service tests: temporary directories, a fake clock, a recording
//! event sink and a fake free-space probe.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use domovoi_config::{PackageLimits, PackagesSection, TrustedKey};
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_proto::messages::TransferKind;
use domovoi_testkit::{BuiltPackage, FakeClock};
use tempfile::TempDir;
use uuid::Uuid;

use super::support::start;
use crate::packages::events::RecordingEvents;
use crate::packages::{
    InstallOutcome, InstallRequest, PackageDeps, PackageDirs, PackageError, PackageService,
    SpaceProbe,
};

/// A free-space probe that reports a number the test sets.
#[derive(Debug)]
pub(super) struct FakeSpace(pub AtomicU64);

impl SpaceProbe for FakeSpace {
    fn free_bytes(&self, _path: &Path) -> std::io::Result<u64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}

/// Temporary directories plus the fakes a service needs.
pub(super) struct Env {
    pub root: TempDir,
    pub dirs: PackageDirs,
    pub clock: Arc<FakeClock>,
    pub events: Arc<RecordingEvents>,
    pub space: Arc<FakeSpace>,
}

impl Env {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let skills = root.path().join("skills");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(&skills).unwrap();
        let dirs = PackageDirs::new(root.path().join("packages"), state, skills);
        Self {
            root,
            dirs,
            clock: Arc::new(FakeClock::new(start())),
            events: Arc::new(RecordingEvents::default()),
            space: Arc::new(FakeSpace(AtomicU64::new(u64::MAX))),
        }
    }

    pub(super) fn packages(&self) -> &Path {
        &self.dirs.packages_dir
    }

    /// A section that allows installs and trusts the signing key of each of `built`.
    pub(super) fn section(built: &[&BuiltPackage]) -> PackagesSection {
        PackagesSection {
            allow_install: true,
            revoked: Vec::new(),
            trusted_keys: built
                .iter()
                .map(|package| TrustedKey {
                    id: package.key_id.to_string(),
                    public_key: package.public_key_b64.clone(),
                    label: None,
                })
                .collect(),
            limits: PackageLimits::default(),
        }
    }

    pub(super) fn deps(&self, runs_as_root: bool) -> PackageDeps {
        PackageDeps {
            clock: self.clock.clone(),
            runs_as_root,
            platform: "linux-x86_64".to_owned(),
            events: self.events.clone(),
            space: self.space.clone(),
        }
    }

    pub(super) fn try_open(
        &self,
        section: &PackagesSection,
    ) -> Result<PackageService, PackageError> {
        PackageService::open(section, self.dirs.clone(), self.deps(false))
    }

    pub(super) fn open(&self, section: &PackagesSection) -> PackageService {
        self.try_open(section).unwrap()
    }
}

/// Stores `bytes` as a finished transfer of `kind` and returns its id.
pub(super) fn stage(service: &PackageService, kind: TransferKind, bytes: &[u8]) -> Uuid {
    let id = Uuid::now_v7();
    let staging = service.staging().unwrap();
    std::fs::write(staging.bin_path(id), bytes).unwrap();
    staging.register_staged(id, kind, bytes.len() as u64);
    id
}

/// Stages both transfers of `built` and returns the install request for it.
pub(super) fn request(
    service: &PackageService,
    built: &BuiltPackage,
    name: &str,
    version: &str,
) -> InstallRequest {
    InstallRequest {
        package: PackageName::parse(name).unwrap(),
        version: PackageVersion::parse(version).unwrap(),
        artifact_transfer: stage(service, TransferKind::PackageArtifact, &built.archive),
        signature_transfer: stage(
            service,
            TransferKind::PackageManifest,
            built.sig_toml.as_bytes(),
        ),
    }
}

/// Stages `built` and installs it as package `name` at `version`.
pub(super) fn install(
    service: &PackageService,
    built: &BuiltPackage,
    name: &str,
    version: &str,
) -> Result<InstallOutcome, PackageError> {
    let req = request(service, built, name, version);
    service.install(Uuid::now_v7(), req)
}
