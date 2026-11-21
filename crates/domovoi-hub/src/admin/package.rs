//! `domovoi package import` and `domovoi package list`: the owner's offline import path.

use std::path::Path;
use std::sync::Arc;

use domovoi_core::Actor;
use domovoi_core::install::PackageArtifact;
use domovoi_core::package::MAX_SIGNATURE_FILE_BYTES;
use domovoi_core::ports::{InstallRepo, UnitOfWork};
use tokio::io::AsyncReadExt as _;

use super::{AdminError, OfflineAdmin};
use crate::packages::{
    ImportContext, ImportOutcome, PackageImporter, PackageKeys, PackageStore, ReaderSource,
};

impl OfflineAdmin {
    fn package_importer(&self, store: PackageStore) -> Result<PackageImporter, AdminError> {
        let section = self.config.packages.clone();
        let keys = PackageKeys::from_config(&section)
            .map_err(|err| AdminError::Invalid(format!("[packages] {err}")))?;
        let installs: Arc<dyn InstallRepo> = self.store.clone();
        let uow: Arc<dyn UnitOfWork> = self.store.clone();
        Ok(PackageImporter::new(
            section,
            Arc::new(keys),
            store,
            installs,
            uow,
            self.clock.clone(),
        ))
    }

    /// Imports the archive at `archive` with the signature file at `signature`, as
    /// `system:cli`. The archive is streamed from disk, never read whole.
    ///
    /// # Errors
    /// [`AdminError::Package`] with the refusal's code, or [`AdminError::Invalid`] when a
    /// file cannot be read.
    pub async fn package_import(
        &self,
        archive: &Path,
        signature: &Path,
    ) -> Result<ImportOutcome, AdminError> {
        let unreadable = |path: &Path, err: std::io::Error| {
            AdminError::Invalid(format!("cannot read {}: {err}", path.display()))
        };
        let mut signature_bytes = Vec::new();
        tokio::fs::File::open(signature)
            .await
            .map_err(|err| unreadable(signature, err))?
            .take(MAX_SIGNATURE_FILE_BYTES as u64 + 1)
            .read_to_end(&mut signature_bytes)
            .await
            .map_err(|err| unreadable(signature, err))?;
        let file = tokio::fs::File::open(archive)
            .await
            .map_err(|err| unreadable(archive, err))?;
        let store = PackageStore::open(&self.config.hub.data_dir)
            .await
            .map_err(crate::packages::ImportError::from)?;
        let importer = self.package_importer(store)?;
        let ctx = ImportContext {
            actor: Actor::System("cli".to_owned()),
            audit_rejections: true,
        };
        let mut source = ReaderSource::new(file);
        Ok(importer.import(&ctx, &mut source, &signature_bytes).await?)
    }

    /// Every imported package, ordered by name then version.
    pub async fn package_list(&self) -> Result<Vec<PackageArtifact>, AdminError> {
        Ok(self.store.artifacts().await?)
    }
}
