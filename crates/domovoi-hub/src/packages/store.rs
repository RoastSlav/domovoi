//! The file store for imported archives: `<data_dir>/packages/<aa>/<sha256>.tar.zst`.
//!
//! Paths are built only from a digest and fixed names, so nothing a caller sends can name a
//! file outside the store. Bytes are written to `.tmp/<uuid>.part`, flushed, then renamed, so
//! a file under its digest name is always whole. The store never opens an archive.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use domovoi_core::package::ArchiveDigest;
use tokio::fs;
use tokio::io::AsyncWriteExt as _;
use uuid::Uuid;

const SUFFIX: &str = ".tar.zst";

/// The directory of imported archives.
#[derive(Debug, Clone)]
pub struct PackageStore {
    root: PathBuf,
}

/// An archive being written under `.tmp`. Dropping it without [`StagedWriter::finish`] deletes
/// the partial file.
#[derive(Debug)]
pub struct StagedWriter {
    file: Option<fs::File>,
    path: PathBuf,
}

/// A complete archive waiting under `.tmp` to be placed or discarded. Dropping it deletes it.
#[derive(Debug)]
pub struct StagedFile {
    path: Option<PathBuf>,
}

/// What [`PackageStore::place`] did with a staged archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// The archive is now stored under its digest.
    New,
    /// A file with that digest was already stored; the staged copy was deleted.
    Existed,
}

/// What a startup sweep removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Partial uploads left in `.tmp`.
    pub tmp_files: Vec<PathBuf>,
    /// Archives no index row refers to.
    pub unindexed: Vec<PathBuf>,
}

/// How much the files on disk hold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Number of stored archives.
    pub files: u64,
    /// Their total size in bytes.
    pub bytes: u64,
}

impl PackageStore {
    /// Opens the store under `data_dir`, creating `packages/` and `packages/.tmp/` (mode 0700
    /// on Unix).
    ///
    /// # Errors
    /// The I/O error when a directory cannot be created.
    pub async fn open(data_dir: &Path) -> io::Result<Self> {
        let store = Self {
            root: data_dir.join("packages"),
        };
        make_dir(&store.tmp_dir()).await?;
        Ok(store)
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join(".tmp")
    }

    /// Where the archive with `digest` lives, whether or not it exists.
    #[must_use]
    pub fn path_of(&self, digest: &ArchiveDigest) -> PathBuf {
        let hex = digest.to_string();
        self.root.join(&hex[..2]).join(format!("{hex}{SUFFIX}"))
    }

    /// Whether the archive with `digest` is on disk.
    pub async fn contains(&self, digest: &ArchiveDigest) -> bool {
        fs::metadata(self.path_of(digest))
            .await
            .is_ok_and(|meta| meta.is_file())
    }

    /// Starts a new partial file in `.tmp` (created exclusively, mode 0600 on Unix).
    ///
    /// # Errors
    /// The I/O error when the file cannot be created.
    pub async fn begin(&self) -> io::Result<StagedWriter> {
        make_dir(&self.tmp_dir()).await?;
        let path = self.tmp_dir().join(format!("{}.part", Uuid::now_v7()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let file = options.open(&path).await?;
        Ok(StagedWriter {
            file: Some(file),
            path,
        })
    }

    /// Moves a staged archive to its digest name, or deletes it when that file exists, and
    /// flushes the directory so the rename survives a crash.
    ///
    /// # Errors
    /// The I/O error when the directory or the rename fails; the staged file is deleted.
    pub async fn place(
        &self,
        mut staged: StagedFile,
        digest: &ArchiveDigest,
    ) -> io::Result<Placed> {
        let Some(from) = staged.path.take() else {
            return Err(io::Error::other("the staged archive is already gone"));
        };
        let target = self.path_of(digest);
        if self.contains(digest).await {
            let _ = fs::remove_file(&from).await;
            return Ok(Placed::Existed);
        }
        let result = async {
            let dir = target.parent().unwrap_or(&self.root);
            make_dir(dir).await?;
            fs::rename(&from, &target).await?;
            sync_dir(dir).await
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&from).await;
        }
        result.map(|()| Placed::New)
    }

    /// Deletes the archive with `digest`. A missing file is not an error.
    ///
    /// # Errors
    /// The I/O error when the file exists but cannot be removed.
    pub async fn remove(&self, digest: &ArchiveDigest) -> io::Result<()> {
        match fs::remove_file(self.path_of(digest)).await {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        }
    }

    /// Counts the archives on disk and their bytes.
    ///
    /// # Errors
    /// The I/O error when a directory cannot be read.
    pub async fn usage(&self) -> io::Result<Usage> {
        let mut usage = Usage::default();
        for (path, _) in self.stored().await? {
            usage.files += 1;
            usage.bytes += fs::metadata(path).await?.len();
        }
        Ok(usage)
    }

    /// Deletes everything in `.tmp` and every archive whose digest is not in `indexed`, and
    /// logs each file. Files that do not look like archives are left alone and logged.
    ///
    /// # Errors
    /// The I/O error when a directory cannot be read or a file cannot be removed.
    pub async fn sweep(&self, indexed: &BTreeSet<ArchiveDigest>) -> io::Result<SweepReport> {
        let mut report = SweepReport::default();
        let mut tmp = fs::read_dir(self.tmp_dir()).await?;
        while let Some(entry) = tmp.next_entry().await? {
            let path = entry.path();
            tracing::warn!(file = %path.display(), "removing a partial package upload left by a crash");
            fs::remove_file(&path).await?;
            report.tmp_files.push(path);
        }
        for (path, digest) in self.stored().await? {
            if !indexed.contains(&digest) {
                tracing::warn!(file = %path.display(), "removing a package archive that no import refers to");
                fs::remove_file(&path).await?;
                report.unindexed.push(path);
            }
        }
        Ok(report)
    }

    /// Every file named like `<aa>/<sha256>.tar.zst` with its digest.
    async fn stored(&self) -> io::Result<Vec<(PathBuf, ArchiveDigest)>> {
        let mut found = Vec::new();
        let mut shards = fs::read_dir(&self.root).await?;
        while let Some(shard) = shards.next_entry().await? {
            let name = shard.file_name().to_string_lossy().into_owned();
            if name.len() != 2 || !shard.file_type().await?.is_dir() {
                continue;
            }
            let mut files = fs::read_dir(shard.path()).await?;
            while let Some(file) = files.next_entry().await? {
                let path = file.path();
                let file_name = file.file_name().to_string_lossy().into_owned();
                let digest = file_name
                    .strip_suffix(SUFFIX)
                    .filter(|hex| hex.starts_with(&name))
                    .and_then(|hex| {
                        hex.parse::<ArchiveDigest>()
                            .ok()
                            .filter(|d| d.to_string() == hex)
                    });
                match digest {
                    Some(digest) => found.push((path, digest)),
                    None => {
                        tracing::warn!(file = %path.display(), "leaving an unknown file in the package store")
                    }
                }
            }
        }
        Ok(found)
    }
}

impl StagedWriter {
    /// Appends `bytes`.
    ///
    /// # Errors
    /// The I/O error when the write fails.
    pub async fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.write_all(bytes).await,
            None => Err(io::Error::other("the staged archive is closed")),
        }
    }

    /// Flushes the file to disk and hands it over for placing.
    ///
    /// # Errors
    /// The I/O error when the flush fails; the partial file is deleted.
    pub async fn finish(mut self) -> io::Result<StagedFile> {
        if let Some(mut file) = self.file.take() {
            file.flush().await?;
            file.sync_all().await?;
        }
        Ok(StagedFile {
            path: Some(std::mem::take(&mut self.path)),
        })
    }
}

impl Drop for StagedWriter {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

impl StagedFile {
    /// Deletes the staged archive.
    pub async fn discard(mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path).await;
        }
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Creates `dir` and its parents, mode 0700 on Unix.
async fn make_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(dir).await
}

/// Flushes a directory's entries to disk. Windows cannot open a directory, so it does not.
async fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(dir).await?.sync_all().await
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}
