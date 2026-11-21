//! Importing a signed package: stream the archive to disk, check it against its signature
//! file and the owner keys, store it under its digest and index it.
//!
//! The archive is never held in memory and never opened: the hub only counts its bytes,
//! checks the zstd magic number on the first four, and hashes it while it streams.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_config::PackagesHubSection;
use domovoi_core::PackageArtifactId;
use domovoi_core::actor::Actor;
use domovoi_core::install::{ArtifactGuard, ArtifactSource, PackageArtifact};
use domovoi_core::package::ArchiveDigest;
use domovoi_core::ports::{
    AuditDraft, Clock, CommitError, InstallRepo, InstallWrite, StateWrite, Transaction, UnitOfWork,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt as _};

use super::error::ImportError;
use super::keys::PackageKeys;
use super::store::{PackageStore, Placed, StagedFile, SweepReport};
use super::verify::{parse_signature_file, verify_signature};

/// The first four bytes of every zstd frame.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// Where the archive's bytes come from, one chunk at a time. The HTTP upload hands over
/// `field.chunk()`; the command line hands over a file.
#[async_trait]
pub trait ArchiveSource: Send {
    /// The next chunk, or `None` at the end. An error is reported as an upload failure.
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, std::io::Error>;
}

/// An [`ArchiveSource`] over any async reader, such as a file.
pub struct ReaderSource<R> {
    reader: R,
    chunk: usize,
}

impl<R: AsyncRead + Unpin + Send> ReaderSource<R> {
    /// Reads `reader` in chunks of 64 KiB.
    pub fn new(reader: R) -> Self {
        Self::with_chunk_size(reader, 64 * 1024)
    }

    /// Reads `reader` in chunks of at most `chunk` bytes (at least 1).
    pub fn with_chunk_size(reader: R, chunk: usize) -> Self {
        Self {
            reader,
            chunk: chunk.max(1),
        }
    }
}

#[async_trait]
impl<R: AsyncRead + Unpin + Send> ArchiveSource for ReaderSource<R> {
    async fn next_chunk(&mut self) -> Result<Option<Vec<u8>>, std::io::Error> {
        let mut buffer = vec![0u8; self.chunk];
        let read = self.reader.read(&mut buffer).await?;
        buffer.truncate(read);
        Ok((read > 0).then_some(buffer))
    }
}

/// Who is importing, and whether a refusal is worth an audit row.
#[derive(Debug, Clone)]
pub struct ImportContext {
    /// The actor on every audit row: the signed-in owner, or `system:cli`.
    pub actor: Actor,
    /// Record `package.import_rejected` for a refusal. Set only for an authenticated caller,
    /// so an anonymous request cannot grow the audit log.
    pub audit_rejections: bool,
}

/// An archive that was streamed to disk and hashed, waiting for its signature file.
#[derive(Debug)]
pub struct StagedArchive {
    file: StagedFile,
    digest: ArchiveDigest,
    size: u64,
}

impl StagedArchive {
    /// The SHA-256 of the streamed bytes.
    #[must_use]
    pub const fn digest(&self) -> &ArchiveDigest {
        &self.digest
    }

    /// How many bytes were streamed.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Deletes the staged file, for a request that failed before [`PackageImporter::finish`].
    pub async fn discard(self) {
        self.file.discard().await;
    }
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// The index row, new or existing.
    pub artifact: PackageArtifact,
    /// True when the same archive was already imported; nothing changed except that a
    /// missing file was restored.
    pub already_imported: bool,
}

/// Imports and deletes packages. One value serves the whole hub; commits are serialized.
pub struct PackageImporter {
    config: PackagesHubSection,
    keys: Arc<PackageKeys>,
    store: PackageStore,
    installs: Arc<dyn InstallRepo>,
    uow: Arc<dyn UnitOfWork>,
    clock: Arc<dyn Clock>,
    serial: tokio::sync::Mutex<()>,
}

impl PackageImporter {
    /// An importer that follows `config`'s limits and trusts `keys`.
    pub fn new(
        config: PackagesHubSection,
        keys: Arc<PackageKeys>,
        store: PackageStore,
        installs: Arc<dyn InstallRepo>,
        uow: Arc<dyn UnitOfWork>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            config,
            keys,
            store,
            installs,
            uow,
            clock,
            serial: tokio::sync::Mutex::new(()),
        }
    }

    /// The file store, for reading file status.
    #[must_use]
    pub const fn store(&self) -> &PackageStore {
        &self.store
    }

    /// Streams `source` into the store's `.tmp`, hashing and counting as it goes.
    ///
    /// # Errors
    /// [`ImportError::PackagesDisabled`], [`ImportError::TooLarge`] on the first byte past
    /// `max_artifact_bytes`, [`ImportError::NotZstd`] when the first four bytes are not the
    /// zstd magic, or an upload or I/O failure. The partial file is deleted on every error.
    pub async fn stage(
        &self,
        ctx: &ImportContext,
        source: &mut dyn ArchiveSource,
    ) -> Result<StagedArchive, ImportError> {
        let result = self.stream(source).await;
        self.audited(ctx, "package:import", result).await
    }

    /// Checks `staged` against `signature` (the signature file's bytes), then stores and
    /// indexes it, or recognizes it as already imported.
    ///
    /// # Errors
    /// Any [`ImportError`]; the staged file is deleted on every error.
    pub async fn finish(
        &self,
        ctx: &ImportContext,
        staged: StagedArchive,
        signature: &[u8],
    ) -> Result<ImportOutcome, ImportError> {
        let subject = parse_signature_file(signature).map_or_else(
            |_| "package:import".to_owned(),
            |(file, _)| subject_of(&file),
        );
        let result = self.commit(ctx, staged, signature).await;
        self.audited(ctx, &subject, result).await
    }

    /// [`stage`](Self::stage) then [`finish`](Self::finish), for callers that have the
    /// signature file before the archive.
    ///
    /// # Errors
    /// Any [`ImportError`].
    pub async fn import(
        &self,
        ctx: &ImportContext,
        source: &mut dyn ArchiveSource,
        signature: &[u8],
    ) -> Result<ImportOutcome, ImportError> {
        let staged = self.stage(ctx, source).await?;
        self.finish(ctx, staged, signature).await
    }

    async fn stream(&self, source: &mut dyn ArchiveSource) -> Result<StagedArchive, ImportError> {
        if !self.config.enabled {
            return Err(ImportError::PackagesDisabled);
        }
        let max = self.config.max_artifact_bytes;
        let mut writer = self.store.begin().await?;
        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut head: Vec<u8> = Vec::with_capacity(ZSTD_MAGIC.len());
        while let Some(chunk) = source
            .next_chunk()
            .await
            .map_err(|err| ImportError::Upload(err.to_string()))?
        {
            size = size.saturating_add(chunk.len() as u64);
            if size > max {
                return Err(ImportError::TooLarge { max });
            }
            if head.len() < ZSTD_MAGIC.len() {
                let take = (ZSTD_MAGIC.len() - head.len()).min(chunk.len());
                head.extend_from_slice(&chunk[..take]);
                if head.len() == ZSTD_MAGIC.len() && head != ZSTD_MAGIC {
                    return Err(ImportError::NotZstd);
                }
            }
            hasher.update(&chunk);
            writer.write(&chunk).await?;
        }
        if head != ZSTD_MAGIC {
            return Err(ImportError::NotZstd);
        }
        let digest = ArchiveDigest::from_bytes(hasher.finalize().into());
        Ok(StagedArchive {
            file: writer.finish().await?,
            digest,
            size,
        })
    }

    async fn commit(
        &self,
        ctx: &ImportContext,
        staged: StagedArchive,
        signature: &[u8],
    ) -> Result<ImportOutcome, ImportError> {
        if !self.config.enabled {
            return Err(ImportError::PackagesDisabled);
        }
        let (file, text) = parse_signature_file(signature)?;
        let label = verify_signature(&self.keys, &file, &staged.digest)?;
        let _turn = self.serial.lock().await;
        let existing = self.installs.artifact(&file.package, &file.version).await?;
        if let Some(row) = existing {
            if row.archive_sha256 != staged.digest {
                return Err(ImportError::PackageConflict {
                    name: file.package,
                    version: file.version,
                });
            }
            // The same archive again changes nothing but a file that went missing.
            self.store.place(staged.file, &staged.digest).await?;
            return Ok(ImportOutcome {
                artifact: row,
                already_imported: true,
            });
        }
        self.check_caps(&staged).await?;
        let artifact = PackageArtifact {
            id: PackageArtifactId::new(),
            name: file.package,
            version: file.version,
            archive_sha256: staged.digest,
            key_id: file.key_id,
            archive_size: staged.size,
            signature_toml: text,
            signer_label: label,
            source: ArtifactSource::OwnerImport,
            imported_by: ctx.actor.clone(),
            imported_at: self.clock.now(),
        };
        let placed = self
            .store
            .place(staged.file, &artifact.archive_sha256)
            .await?;
        let tx = Transaction::new()
            .write(StateWrite::Install(InstallWrite::ImportArtifact(
                artifact.clone(),
            )))
            .audit(AuditDraft::new(
                ctx.actor.clone(),
                artifact.imported_at,
                "package.imported".to_owned(),
                subject_of_artifact(&artifact),
                json!({
                    "package": artifact.name,
                    "version": artifact.version,
                    "digest": artifact.archive_sha256,
                    "key_id": artifact.key_id,
                    "size": artifact.archive_size,
                }),
            ));
        if let Err(err) = self.uow.commit(tx).await {
            if placed == Placed::New {
                // Nothing else indexed this file, or the import would have been a repeat.
                let _ = self.store.remove(&artifact.archive_sha256).await;
            }
            return Err(match err {
                CommitError::Constraint(_) => ImportError::PackageConflict {
                    name: artifact.name,
                    version: artifact.version,
                },
                other => ImportError::Commit(other),
            });
        }
        Ok(ImportOutcome {
            artifact,
            already_imported: false,
        })
    }

    /// Refuses an import that would pass `max_artifacts` or `max_store_bytes`. An archive
    /// whose bytes are already stored under another name adds no bytes.
    async fn check_caps(&self, staged: &StagedArchive) -> Result<(), ImportError> {
        let rows = self.installs.artifacts().await?;
        if rows.len() as u64 >= u64::from(self.config.max_artifacts) {
            return Err(ImportError::StoreFull(format!(
                "it already holds {} packages, the most this hub keeps",
                rows.len()
            )));
        }
        let mut files = std::collections::BTreeMap::new();
        for row in &rows {
            files.insert(row.archive_sha256, row.archive_size);
        }
        if !files.contains_key(&staged.digest) {
            let used: u64 = files.values().sum();
            if used.saturating_add(staged.size) > self.config.max_store_bytes {
                return Err(ImportError::StoreFull(format!(
                    "{used} bytes are stored and this archive adds {}, over the {} the hub keeps",
                    staged.size, self.config.max_store_bytes
                )));
            }
        }
        Ok(())
    }

    /// Removes the imported package `id`, and its file when no other row has the same digest.
    ///
    /// # Errors
    /// [`ImportError::NoSuchPackage`], [`ImportError::PackageInUse`] while a non-terminal
    /// install job uses it, or a store failure.
    pub async fn delete(
        &self,
        actor: &Actor,
        id: PackageArtifactId,
    ) -> Result<PackageArtifact, ImportError> {
        let _turn = self.serial.lock().await;
        let Some(artifact) = self
            .installs
            .artifacts()
            .await?
            .into_iter()
            .find(|row| row.id == id)
        else {
            return Err(ImportError::NoSuchPackage);
        };
        let tx = Transaction::new()
            .write(StateWrite::Install(InstallWrite::DeleteArtifact {
                id,
                expected: ArtifactGuard {
                    archive_sha256: artifact.archive_sha256,
                },
            }))
            .audit(AuditDraft::new(
                actor.clone(),
                self.clock.now(),
                "package.deleted".to_owned(),
                subject_of_artifact(&artifact),
                json!({
                    "package": artifact.name,
                    "version": artifact.version,
                    "digest": artifact.archive_sha256,
                    "key_id": artifact.key_id,
                }),
            ));
        match self.uow.commit(tx).await {
            Ok(_) => {}
            Err(CommitError::Constraint(reason)) if reason.contains("running install job") => {
                return Err(ImportError::PackageInUse);
            }
            Err(CommitError::Constraint(_)) => return Err(ImportError::NoSuchPackage),
            Err(other) => return Err(ImportError::Commit(other)),
        }
        let shared = self
            .installs
            .artifact_by_digest(&artifact.archive_sha256)
            .await?;
        if shared.is_empty() {
            self.store.remove(&artifact.archive_sha256).await?;
        }
        Ok(artifact)
    }

    /// The startup sweep: deletes partial uploads and archives no row refers to.
    ///
    /// # Errors
    /// A store or I/O failure.
    pub async fn sweep(&self) -> Result<SweepReport, ImportError> {
        let _turn = self.serial.lock().await;
        let indexed: BTreeSet<ArchiveDigest> = self
            .installs
            .artifacts()
            .await?
            .into_iter()
            .map(|row| row.archive_sha256)
            .collect();
        Ok(self.store.sweep(&indexed).await?)
    }

    /// Writes `package.import_rejected` for a refusal when `ctx` asks for it, and passes the
    /// result through.
    async fn audited<T>(
        &self,
        ctx: &ImportContext,
        subject: &str,
        result: Result<T, ImportError>,
    ) -> Result<T, ImportError> {
        let Err(err) = &result else {
            return result;
        };
        if ctx.audit_rejections && err.is_rejection() {
            let draft = AuditDraft::new(
                ctx.actor.clone(),
                self.clock.now(),
                "package.import_rejected".to_owned(),
                subject.to_owned(),
                json!({ "code": err.code(), "message": err.to_string() }),
            );
            if let Err(audit_err) = self.uow.commit(Transaction::new().audit(draft)).await {
                tracing::warn!(error = %audit_err, "could not record a rejected package import");
            }
        }
        result
    }
}

fn subject_of(file: &domovoi_core::package::SignatureFile) -> String {
    format!("package:{}@{}", file.package, file.version)
}

fn subject_of_artifact(artifact: &PackageArtifact) -> String {
    format!("package:{}@{}", artifact.name, artifact.version)
}
