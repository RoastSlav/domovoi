//! Package import and installation: the keys the hub trusts, the file store, and the import
//! pipeline. The hub never opens an archive; it streams, hashes and stores bytes.

pub mod error;
mod files;
pub mod import;
pub mod keys;
pub mod store;
pub mod verify;

#[cfg(test)]
mod tests;

pub use error::ImportError;
pub use import::{
    ArchiveSource, ImportContext, ImportOutcome, PackageImporter, ReaderSource, StagedArchive,
};
pub use keys::PackageKeys;
pub use store::PackageStore;
