//! Tests for package import: a real file store in a temporary directory, the in-memory
//! database and a fake clock.

mod import;
mod store;

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_config::{PackagesHubSection, TrustedKey};
use domovoi_core::Actor;
use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, InstallRepo, Page, UnitOfWork};
use domovoi_testkit::build::T0;
use domovoi_testkit::{BuiltPackage, FakeClock, InMemoryStore, PackageFixture};
use ed25519_dalek::SigningKey;

use super::{
    ArchiveSource, ImportContext, PackageImporter, PackageKeys, PackageStore, ReaderSource,
};

/// A signature file for `archive` as `name`@`version`, signed by `key`.
pub fn signature_for(archive: &[u8], name: &str, version: &str, key: &SigningKey) -> String {
    use base64::Engine as _;
    use domovoi_core::package::{
        ArchiveDigest, KeyId, PackageName, PackageVersion, signing_message,
    };
    use ed25519_dalek::Signer as _;
    let digest = ArchiveDigest::of_bytes(archive);
    let key_id = KeyId::of_public_key(&key.verifying_key().to_bytes());
    let package = PackageName::parse(name).unwrap();
    let version = PackageVersion::parse(version).unwrap();
    let message = signing_message(&package, &version, &digest, &key_id);
    let signature = base64::engine::general_purpose::STANDARD.encode(key.sign(&message).to_bytes());
    format!(
        "format = 1
algorithm = \"ed25519\"
key_id = \"{key_id}\"
package = \"{package}\"
version = \"{version}\"
archive_sha256 = \"{digest}\"
signature = \"{signature}\"
"
    )
}

/// `len` bytes that start with the zstd magic number and are otherwise not an archive.
pub fn zstd_prefixed(len: usize) -> Vec<u8> {
    let mut bytes = vec![0x28, 0xB5, 0x2F, 0xFD];
    bytes.resize(len, 7);
    bytes
}

/// A hub with packages on, one owner key and an empty store.
pub struct Rig {
    pub dir: tempfile::TempDir,
    pub db: Arc<InMemoryStore>,
    pub importer: PackageImporter,
    pub owner: SigningKey,
}

pub fn owner_key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

pub fn trusted(key: &SigningKey, label: &str) -> TrustedKey {
    let built = package_with("probe", "1.0.0", key);
    TrustedKey {
        id: built.key_id.to_string(),
        public_key: built.public_key_b64,
        label: Some(label.to_owned()),
    }
}

pub fn package_with(name: &str, version: &str, key: &SigningKey) -> BuiltPackage {
    PackageFixture::new(name, version)
        .unwrap()
        .skill("probe_it")
        .sign_with(key)
        .build()
}

pub fn section(keys: Vec<TrustedKey>) -> PackagesHubSection {
    PackagesHubSection {
        enabled: true,
        owner_keys: keys,
        ..PackagesHubSection::default()
    }
}

impl Rig {
    pub async fn new() -> Self {
        let owner = owner_key(1);
        let config = section(vec![trusted(&owner, "Owner")]);
        Self::with(config, owner).await
    }

    pub async fn with(config: PackagesHubSection, owner: SigningKey) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(InMemoryStore::default());
        let keys = Arc::new(PackageKeys::from_config(&config).unwrap());
        let store = PackageStore::open(dir.path()).await.unwrap();
        let installs: Arc<dyn InstallRepo> = db.clone();
        let uow: Arc<dyn UnitOfWork> = db.clone();
        let importer = PackageImporter::new(
            config,
            keys,
            store,
            installs,
            uow,
            Arc::new(FakeClock::new(T0)),
        );
        Self {
            dir,
            db,
            importer,
            owner,
        }
    }

    /// Imports `built` as the signed-in owner, with rejections audited.
    pub async fn import(
        &self,
        built: &BuiltPackage,
    ) -> Result<super::ImportOutcome, super::ImportError> {
        let mut source = ReaderSource::new(built.archive.as_slice());
        self.importer
            .import(&owner_ctx(), &mut source, built.sig_toml.as_bytes())
            .await
    }

    pub async fn audit(&self, kind: &str) -> Vec<AuditEvent> {
        let filter = AuditFilter {
            kind: Some(kind.to_owned()),
            ..AuditFilter::default()
        };
        let page = Page {
            cursor: None,
            limit: 100,
        };
        self.db.list(&filter, &page).await.unwrap().items
    }

    /// The names of the files below the store's root, as `<aa>/<name>` or `.tmp/<name>`.
    pub fn files(&self) -> Vec<String> {
        let mut found = Vec::new();
        let root = self.dir.path().join("packages");
        for entry in walk(&root) {
            let relative = entry.strip_prefix(&root).unwrap();
            found.push(relative.to_string_lossy().replace('\\', "/"));
        }
        found.sort();
        found
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

pub fn owner_ctx() -> ImportContext {
    ImportContext {
        actor: Actor::User(domovoi_core::UserId::new()),
        audit_rejections: true,
    }
}

/// Hands out `chunks` one by one and counts how many were taken.
pub struct Chunks {
    chunks: VecDeque<Vec<u8>>,
    pub taken: usize,
}

impl Chunks {
    pub fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: chunks.into(),
            taken: 0,
        }
    }
}

#[async_trait]
impl ArchiveSource for Chunks {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        let next = self.chunks.pop_front();
        if next.is_some() {
            self.taken += 1;
        }
        Ok(next)
    }
}
