//! A request context with a real journal and a real package service, for the capability
//! tests and the package gates.

use std::sync::Arc;

use domovoi_agent::Journal;
use domovoi_agent::link::RequestKind;
use domovoi_agent::packages::{PackageDeps, PackageDirs, PackageService};
use domovoi_agent::requests::{RequestCtx, handle};
use domovoi_config::{MutationLimits, PackageLimits, PackagesSection, TrustedKey};
use domovoi_core::PlanHash;
use domovoi_core::TargetKey;
use domovoi_proto::Message;
use domovoi_proto::messages::{AuthorizationKind, RunCapability, TransferKind};
use domovoi_testkit::BuiltPackage;
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use uuid::Uuid;

use crate::common::Fixture;

/// The boot id the fixture host reports.
pub const BOOT_ID: &str = "018f0000-0000-7000-8000-000000000001";

/// How a rig is set up.
pub struct Options {
    /// Whether `agent.toml` has a `[packages]` section at all.
    pub section: bool,
    /// `allow_install` inside it.
    pub allow_install: bool,
    /// Per-capability mutation limits.
    pub limits: MutationLimits,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            section: true,
            allow_install: true,
            limits: MutationLimits::default(),
        }
    }
}

/// A context around a journal and a package service trusting `trusted`.
pub struct Rig {
    pub fixture: Fixture,
    pub ctx: RequestCtx,
    pub service: Option<Arc<PackageService>>,
    pub dirs: PackageDirs,
}

/// The section for `trusted` keys.
pub fn section(trusted: &[&BuiltPackage], allow_install: bool) -> PackagesSection {
    PackagesSection {
        allow_install,
        revoked: Vec::new(),
        trusted_keys: trusted
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

impl Rig {
    pub fn new(trusted: &[&BuiltPackage], options: Options) -> Self {
        let fixture = Fixture::new();
        let host = fixture.dir.path().join("host");
        std::fs::create_dir_all(host.join("proc/sys/kernel/random")).unwrap();
        std::fs::write(
            host.join("proc/sys/kernel/random/boot_id"),
            format!("{BOOT_ID}\n"),
        )
        .unwrap();
        std::fs::write(host.join("proc/uptime"), "1.00 1.00\n").unwrap();
        let state = fixture.dir.path().join("state");
        Journal::init(&state, T0, BOOT_ID, 1).unwrap();
        let journal = Journal::open(&state, BOOT_ID, 1, options.limits).unwrap();
        let dirs = PackageDirs::new(fixture.dir.path().join("packages"), state, fixture.skills());
        let mut ctx = fixture.ctx(Vec::new(), None, Vec::new());
        ctx.journal = Some(Arc::new(journal));
        let service = options.section.then(|| {
            let deps = PackageDeps::production(fixture.clock.clone(), false, true);
            let section = section(trusted, options.allow_install);
            Arc::new(PackageService::open(&section, dirs.clone(), deps).unwrap())
        });
        ctx.packages = service.clone();
        Self {
            fixture,
            ctx,
            service,
            dirs,
        }
    }

    /// Stores `bytes` as a finished transfer and returns its id.
    pub fn stage(&self, kind: TransferKind, bytes: &[u8]) -> Uuid {
        let id = Uuid::now_v7();
        let staging = self.service.as_ref().unwrap().staging().unwrap();
        std::fs::write(staging.bin_path(id), bytes).unwrap();
        staging.register_staged(id, kind, bytes.len() as u64);
        id
    }

    /// Stages both transfers of `built` and returns the install arguments.
    pub fn install_args(
        &self,
        built: &BuiltPackage,
        name: &str,
        version: &str,
    ) -> Vec<(&'static str, String)> {
        let artifact = self.stage(TransferKind::PackageArtifact, &built.archive);
        let signature = self.stage(TransferKind::PackageManifest, built.sig_toml.as_bytes());
        vec![
            ("package", name.to_owned()),
            ("version", version.to_owned()),
            ("artifact_transfer", artifact.to_string()),
            ("signature_transfer", signature.to_string()),
        ]
    }

    /// A `run_capability` for `contract` with `args`, authorized by a human for five minutes.
    pub fn run(&self, contract: &str, args: &[(&str, String)]) -> RunCapability {
        RunCapability {
            execution_id: Uuid::now_v7(),
            contract: contract.to_owned(),
            version: 1,
            target: TargetKey::parse("package:nginx-tools").unwrap(),
            args: args
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
            plan_hash: "07".repeat(32).parse::<PlanHash>().unwrap(),
            authorization_kind: AuthorizationKind::Human,
            expires_at: T0 + SignedDuration::from_mins(5),
            expected_safety: None,
        }
    }

    /// Serves `run` through the request handler.
    pub async fn call(&self, run: RunCapability) -> Message {
        handle(&self.ctx, &RequestKind::Capability(run)).await
    }
}
